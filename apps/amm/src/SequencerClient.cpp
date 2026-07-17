#include "SequencerClient.h"

#include <algorithm>
#include <array>
#include <cstddef>
#include <memory>

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QTimer>
#include <libbase58.h>

#include "AmmClient.h"

namespace {
constexpr int MAX_CONCURRENT_READS = 8;
constexpr qsizetype ACCOUNT_ID_HEX_SIZE = 64;
constexpr std::size_t BASE58_BUFFER_SIZE = 45;

bool isLowerHex(const QString& value, qsizetype size)
{
    if (value.size() != size)
        return false;
    return std::all_of(value.cbegin(), value.cend(), [](QChar character) {
        return character.isDigit()
            || (character >= QLatin1Char('a') && character <= QLatin1Char('f'));
    });
}

QString accountIdBase58(const QString& accountId)
{
    if (!isLowerHex(accountId, ACCOUNT_ID_HEX_SIZE))
        return {};
    const QByteArray bytes = QByteArray::fromHex(accountId.toLatin1());
    std::array<char, BASE58_BUFFER_SIZE> encoded {};
    std::size_t size = encoded.size();
    if (!b58enc(encoded.data(), &size, bytes.constData(),
                static_cast<std::size_t>(bytes.size()))) {
        return {};
    }
    return QString::fromLatin1(encoded.data(), static_cast<qsizetype>(size - 1));
}

QByteArray rpcBody(const QString& method, const QJsonArray& params)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
        { QStringLiteral("id"), 1 },
        { QStringLiteral("method"), method },
        { QStringLiteral("params"), params },
    }).toJson(QJsonDocument::Compact);
}

WalletAccountRead accountRead(const QJsonObject& value, const QString& fallbackId)
{
    WalletAccountRead read;
    read.accountId = value.value(QStringLiteral("id")).toString(fallbackId);
    read.status = value.value(QStringLiteral("status")).toString(
        QStringLiteral("read_failed"));
    const QJsonObject account = value.value(QStringLiteral("account")).toObject();
    if (read.ok()) {
        read.programOwner = account.value(QStringLiteral("program_owner")).toString();
        read.balanceHex = account.value(QStringLiteral("balance")).toString();
        read.nonceHex = account.value(QStringLiteral("nonce")).toString();
        read.dataHex = account.value(QStringLiteral("data")).toString();
    }
    return read;
}
}

SequencerClient::SequencerClient(AmmClient* client, QObject* parent)
    : QObject(parent),
      m_client(client),
      m_network(new QNetworkAccessManager(this))
{
}

bool SequencerClient::configure(const QString& configPath)
{
    QUrl endpoint;
    QByteArray authorization;
    QFile file(configPath);
    bool valid = file.open(QIODevice::ReadOnly);
    if (valid) {
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
        const QJsonObject config = document.object();
        endpoint = QUrl(config.value(QStringLiteral("sequencer_addr")).toString());
        const QString scheme = endpoint.scheme().toLower();
        valid = document.isObject() && endpoint.isValid() && !endpoint.host().isEmpty()
            && (scheme == QStringLiteral("http") || scheme == QStringLiteral("https"));
        if (valid) {
            const QJsonObject basicAuth = config.value(QStringLiteral("basic_auth")).toObject();
            const QString username = basicAuth.value(QStringLiteral("username")).toString();
            const QString password = basicAuth.value(QStringLiteral("password")).toString();
            if (!username.isEmpty()) {
                authorization = QByteArrayLiteral("Basic ")
                    + (username + QLatin1Char(':') + password).toUtf8().toBase64();
            }
        }
    }
    if (!valid) {
        endpoint = QUrl();
        authorization.clear();
    }
    if (endpoint == m_endpoint && authorization == m_authorization)
        return valid;

    ++m_generation;
    cancelPendingReads();
    clear();
    m_endpoint = endpoint;
    m_authorization = authorization;
    return valid;
}

void SequencerClient::readAccounts(const QStringList& accountIds,
                                   bool forceRefresh,
                                   AccountsCallback callback)
{
    if (accountIds.isEmpty()) {
        QTimer::singleShot(0, this, [callback = std::move(callback)]() mutable {
            callback({});
        });
        return;
    }

    struct Batch {
        QVector<WalletAccountRead> reads;
        qsizetype remaining = 0;
        AccountsCallback callback;
    };
    auto batch = std::make_shared<Batch>();
    batch->reads.resize(accountIds.size());
    batch->remaining = accountIds.size();
    batch->callback = std::move(callback);
    for (qsizetype index = 0; index < accountIds.size(); ++index) {
        readAccount(accountIds.at(index), forceRefresh,
            [batch, index](WalletAccountRead read) mutable {
                batch->reads[index] = std::move(read);
                if (--batch->remaining == 0)
                    batch->callback(std::move(batch->reads));
            });
    }
}

void SequencerClient::readAccount(const QString& accountId,
                                  bool forceRefresh,
                                  AccountCallback callback)
{
    if (!isConfigured() || !isLowerHex(accountId, ACCOUNT_ID_HEX_SIZE)) {
        QTimer::singleShot(0, this,
            [callback = std::move(callback), accountId]() mutable {
                callback(WalletAccountRead { accountId });
            });
        return;
    }
    if (forceRefresh)
        m_cache.remove(accountId);
    if (!forceRefresh && m_cache.contains(accountId)) {
        const WalletAccountRead cached = m_cache.value(accountId);
        const quint64 generation = m_generation;
        QTimer::singleShot(0, this,
            [this, callback = std::move(callback), cached, accountId,
             generation]() mutable {
                callback(generation == m_generation
                    ? cached : WalletAccountRead { accountId });
            });
        return;
    }
    if (forceRefresh && m_activeReadIds.contains(accountId)) {
        m_forcedWaiters[accountId].append(std::move(callback));
        return;
    }

    const bool alreadyPending = m_waiters.contains(accountId);
    m_waiters[accountId].append(std::move(callback));
    if (!alreadyPending)
        m_pending.enqueue({ accountId });
    startPendingReads();
}

void SequencerClient::startPendingReads()
{
    while (m_activeReads < MAX_CONCURRENT_READS && !m_pending.isEmpty()) {
        const PendingRead pending = m_pending.dequeue();
        ++m_activeReads;
        m_activeReadIds.insert(pending.accountId);
        startRead(pending.accountId);
    }
}

void SequencerClient::startRead(const QString& accountId)
{
    const quint64 generation = m_generation;
    const QString encodedId = accountIdBase58(accountId);
    if (encodedId.isEmpty()) {
        completeRead(accountId, WalletAccountRead { accountId });
        return;
    }

    QNetworkReply* reply = m_network->post(
        request(), rpcBody(QStringLiteral("getAccount"), QJsonArray { encodedId }));
    connect(reply, &QNetworkReply::finished, this,
        [this, reply, accountId, generation]() {
            if (generation != m_generation) {
                reply->deleteLater();
                return;
            }
            WalletAccountRead read { accountId };
            const int status = reply->attribute(
                QNetworkRequest::HttpStatusCodeAttribute).toInt();
            const QByteArray payload = reply->readAll();
            if (reply->error() == QNetworkReply::NoError
                && status >= 200 && status < 300) {
                const AmmClientResult normalized =
                    m_client->normalizeAccountRpc(QJsonObject {
                        { QStringLiteral("accountId"), accountId },
                        { QStringLiteral("response"), QString::fromUtf8(payload) },
                    });
                if (normalized.ok)
                    read = accountRead(normalized.value, accountId);
            }
            reply->deleteLater();
            completeRead(accountId, read);
        });
}

void SequencerClient::completeRead(const QString& accountId,
                                   const WalletAccountRead& read)
{
    if (read.ok())
        m_cache.insert(accountId, read);
    const QVector<AccountCallback> callbacks = m_waiters.take(accountId);
    const QVector<AccountCallback> forcedCallbacks = m_forcedWaiters.take(accountId);
    m_activeReadIds.remove(accountId);
    --m_activeReads;
    if (!forcedCallbacks.isEmpty()) {
        m_cache.remove(accountId);
        m_waiters.insert(accountId, forcedCallbacks);
        m_pending.enqueue({ accountId });
    }
    for (const AccountCallback& callback : callbacks)
        callback(read);
    startPendingReads();
}

void SequencerClient::queryTransaction(const QString& nativeHash,
                                       TransactionCallback callback)
{
    if (!isConfigured() || !isLowerHex(nativeHash, ACCOUNT_ID_HEX_SIZE)) {
        QTimer::singleShot(0, this,
            [callback = std::move(callback)]() mutable { callback(false, false); });
        return;
    }
    const quint64 generation = m_generation;
    QNetworkReply* reply = m_network->post(
        request(), rpcBody(QStringLiteral("getTransaction"), QJsonArray { nativeHash }));
    connect(reply, &QNetworkReply::finished, this,
        [this, reply, generation, callback = std::move(callback)]() mutable {
            if (generation != m_generation) {
                reply->deleteLater();
                callback(false, false);
                return;
            }
            const int status = reply->attribute(
                QNetworkRequest::HttpStatusCodeAttribute).toInt();
            const QByteArray payload = reply->readAll();
            QJsonParseError error;
            const QJsonDocument document = QJsonDocument::fromJson(payload, &error);
            const QJsonObject envelope = document.object();
            const bool ok = reply->error() == QNetworkReply::NoError
                && status >= 200 && status < 300
                && error.error == QJsonParseError::NoError
                && document.isObject()
                && (!envelope.contains(QStringLiteral("error"))
                    || envelope.value(QStringLiteral("error")).isNull())
                && envelope.contains(QStringLiteral("result"));
            const bool included = ok
                && !envelope.value(QStringLiteral("result")).isNull();
            reply->deleteLater();
            callback(ok, included);
        });
}

void SequencerClient::clear()
{
    m_cache.clear();
}

void SequencerClient::cancelPendingReads()
{
    decltype(m_waiters) waiters;
    waiters.swap(m_waiters);
    for (auto iterator = m_forcedWaiters.cbegin();
         iterator != m_forcedWaiters.cend(); ++iterator) {
        waiters[iterator.key()].append(iterator.value());
    }
    m_forcedWaiters.clear();
    m_pending.clear();
    m_activeReadIds.clear();
    m_activeReads = 0;
    for (auto iterator = waiters.cbegin(); iterator != waiters.cend(); ++iterator) {
        const WalletAccountRead failed { iterator.key() };
        for (const AccountCallback& callback : iterator.value()) {
            QTimer::singleShot(0, this, [callback, failed]() {
                callback(failed);
            });
        }
    }
}

void SequencerClient::applyAuthorization(QNetworkRequest& request) const
{
    if (!m_authorization.isEmpty())
        request.setRawHeader(QByteArrayLiteral("Authorization"), m_authorization);
}

QNetworkRequest SequencerClient::request() const
{
    QNetworkRequest request(m_endpoint);
    request.setHeader(QNetworkRequest::ContentTypeHeader,
                      QStringLiteral("application/json"));
    request.setTransferTimeout(4000);
    applyAuthorization(request);
    return request;
}
