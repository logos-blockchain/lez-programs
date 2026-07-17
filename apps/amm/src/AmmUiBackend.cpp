#include "AmmUiBackend.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QDateTime>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QTimer>
#include <QUrl>

#include "AmmClient.h"
#include "LogosWalletProvider.h"
#include "NewPositionRuntime.h"
#include "SequencerClient.h"
#include "WalletController.h"
#include "logos_api.h"

namespace {
    const int CHECKPOINT_BLOCK_ID = 10;
    const int BLOCK_HASH_OFFSET = 40;
    const int BLOCK_HASH_SIZE = 32;

    QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
    {
        return QJsonDocument(QJsonObject {
            { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
            { QStringLiteral("id"), 1 },
            { QStringLiteral("method"), method },
            { QStringLiteral("params"), params },
        }).toJson(QJsonDocument::Compact);
    }

    QString blockHashFromResponse(const QByteArray& payload)
    {
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(payload, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject())
            return {};
        const QByteArray block =
            QByteArray::fromBase64(document.object().value(QStringLiteral("result")).toString().toLatin1());
        if (block.size() < BLOCK_HASH_OFFSET + BLOCK_HASH_SIZE)
            return {};
        return QString::fromLatin1(block.mid(BLOCK_HASH_OFFSET, BLOCK_HASH_SIZE).toHex());
    }

    QString channelIdFromResponse(const QByteArray& payload)
    {
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(payload, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject())
            return {};
        const QString channel = document.object().value(QStringLiteral("result")).toString();
        return ActiveNetwork::isValidIdentity(channel) ? channel : QString();
    }

}

namespace {
constexpr int CHECKPOINT_BLOCK_ID = 10;
constexpr int BLOCK_HASH_OFFSET = 40;
constexpr int BLOCK_HASH_SIZE = 32;
const QString DEFAULT_PROGRAM_OWNER(64, QLatin1Char('0'));

QByteArray resource(const QString& path)
{
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
}

QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
        { QStringLiteral("id"), 1 },
        { QStringLiteral("method"), method },
        { QStringLiteral("params"), params },
    }).toJson(QJsonDocument::Compact);
}

QString blockHashFromResponse(const QByteArray& payload)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(payload, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return {};
    const QByteArray block = QByteArray::fromBase64(
        document.object().value(QStringLiteral("result")).toString().toLatin1());
    if (block.size() < BLOCK_HASH_OFFSET + BLOCK_HASH_SIZE)
        return {};
    return QString::fromLatin1(block.mid(BLOCK_HASH_OFFSET, BLOCK_HASH_SIZE).toHex());
}

QString channelIdFromResponse(const QByteArray& payload)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(payload, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return {};
    const QString channel = document.object().value(QStringLiteral("result")).toString();
    return ActiveNetwork::isValidIdentity(channel) ? channel : QString();
}

QString decimalAdd(const QString& left, const QString& right)
{
    if (left.isEmpty() || right.isEmpty())
        return {};
    if (!std::all_of(left.cbegin(), left.cend(), [](QChar value) { return value.isDigit(); })
        || !std::all_of(right.cbegin(), right.cend(), [](QChar value) { return value.isDigit(); })) {
        return {};
    }
    QString result;
    result.reserve(std::max(left.size(), right.size()) + 1);
    qsizetype leftIndex = left.size();
    qsizetype rightIndex = right.size();
    int carry = 0;
    while (leftIndex > 0 || rightIndex > 0 || carry > 0) {
        const int leftDigit = leftIndex > 0
            ? left.at(--leftIndex).digitValue() : 0;
        const int rightDigit = rightIndex > 0
            ? right.at(--rightIndex).digitValue() : 0;
        const int sum = leftDigit + rightDigit + carry;
        result.prepend(QChar(QLatin1Char('0').unicode() + sum % 10));
        carry = sum / 10;
    }
    while (result.size() > 1 && result.startsWith(QLatin1Char('0')))
        result.remove(0, 1);
    return result;
}

QJsonObject enumFields(const QJsonValue& value, const QString& variant)
{
    return value.toObject().value(variant).toObject();
}

WalletAccountRead accountRead(const WalletAccount& account)
{
    WalletAccountRead read;
    read.accountId = account.address;
    read.status = account.readStatus;
    read.programOwner = account.programOwner;
    read.dataHex = account.dataHex;
    return read;
}
}

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("AmmUI"))),
      m_ammClient(std::make_unique<BundledAmmClient>()),
      m_sequencer(std::make_unique<SequencerClient>(m_ammClient.get(), this)),
      m_newPosition(std::make_unique<NewPositionRuntime>(
          m_wallet.get(), m_ammClient.get(), m_sequencer.get())),
      m_net(new QNetworkAccessManager(this)),
      m_transactionTimer(new QTimer(this)),
      m_identityRetryTimer(new QTimer(this))
{
    setNewPositionQuoteResult({});
    setNewPositionSubmitResult({});
    m_transactionTimer->setInterval(5000);
    connect(m_transactionTimer, &QTimer::timeout,
            this, &AmmUiBackend::pollTransactions);
    m_identityRetryTimer->setSingleShot(true);
    connect(m_identityRetryTimer, &QTimer::timeout,
            this, &AmmUiBackend::probeNetworkIdentity);
    m_network.load();

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    syncWalletState();
    refreshNewPositionContext({});
    m_walletController->start();
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString AmmUiBackend::createNewDefault(QString password)
{
    const QString mnemonic = m_walletController->createDefaultWallet(password);
    syncWalletState();
    return mnemonic;
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    const QString mnemonic =
        m_walletController->createWallet(configPath, storagePath, password);
    syncWalletState();
    return mnemonic;
}

bool AmmUiBackend::openExisting()
{
    const bool opened = m_walletController->open();
    syncWalletState();
    return opened;
}

void AmmUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
    m_newPosition->clearWalletAccounts();
    refreshNewPositionContext(QVariantMap());
}

QString AmmUiBackend::createAccountPublic()
{
    return m_walletController->createAccount(true);
}

QString AmmUiBackend::createAccountPrivate()
{
    return m_walletController->createAccount(false);
}

void AmmUiBackend::refreshAccounts()
{
    m_walletController->refresh();
}

void AmmUiBackend::refreshBalances()
{
    m_walletController->refresh();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_walletController->balance(accountIdHex, isPublic);
}

void AmmUiBackend::refreshNewPositionContext(QVariantMap request)
{
    const bool refreshWalletAccounts =
        request.take(QStringLiteral("refreshWalletAccounts")).toBool();
    if (request.contains(QStringLiteral("recentTokenIds"))
        || request.contains(QStringLiteral("resolvedTokenIds"))) {
        m_newPositionHints = request;
    }
    else {
        request = m_newPositionHints;
    }
    const quint64 generation = ++m_contextGeneration;
    m_newPosition->contextAsync(
        request, m_network.snapshot(), isWalletOpen(), refreshWalletAccounts,
        [this, generation](QVariantMap result) {
            if (generation == m_contextGeneration) {
                result.insert(QStringLiteral("requestId"), generation);
                setNewPositionContext(std::move(result));
            }
        });
}

void AmmUiBackend::requestNewPositionQuote(QVariantMap request,
                                           int requestId,
                                           bool forceRefresh)
{
    m_newPosition->quoteAsync(
        request, m_network.snapshot(), isWalletOpen(), forceRefresh,
        [this, requestId](QVariantMap result) {
            result.insert(QStringLiteral("requestId"), requestId);
            setNewPositionQuoteResult(std::move(result));
        });
}

void AmmUiBackend::requestNewPositionSubmit(QVariantMap request,
                                            QString quoteHash,
                                            int requestId)
{
    m_newPosition->submitAsync(
        request, quoteHash, m_network.snapshot(), walletCanSubmit(),
        [this, requestId](QVariantMap result) {
            result.insert(QStringLiteral("requestId"), requestId);
            setNewPositionSubmitResult(result);
            watchTransaction(result);
        });
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    const bool walletWasOpen = isWalletOpen();
    const bool wasReachable = sequencerReachable();
    const QString previousAddress = sequencerAddr();

    setIsWalletOpen(state.isWalletOpen);
    setWalletSyncStatus(state.syncStatus);
    setWalletSyncError(state.syncError);
    setWalletCanSubmit(state.canSubmit());
    setWalletStateReady(state.syncStatus != QStringLiteral("opening")
                        && state.syncStatus != QStringLiteral("syncing"));
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);

    m_sequencer->configure(state.configPath);

    const bool addressChanged = previousAddress != state.sequencerAddress;
    if (addressChanged) {
        m_identityRetryTimer->stop();
        m_pendingTransactions.clear();
        m_transactionTimer->stop();
        m_network.sequencerChanged(!state.sequencerAddress.isEmpty());
    }
    if (addressChanged || wasReachable != state.sequencerReachable) {
        m_identityRetryTimer->stop();
        m_network.reachabilityChanged(state.sequencerReachable, wasReachable);
    }
    if (walletWasOpen && !state.isWalletOpen)
        m_newPosition->clearWalletAccounts();
    if (state.canSubmit()) {
        const WalletSnapshot snapshot = m_wallet->snapshot();
        if (snapshot.ok())
            m_newPosition->setWalletAccounts(snapshot.accounts);
    }

    publishNetworkContext();
    if (state.sequencerReachable && m_network.needsIdentityProbe())
        probeNetworkIdentity();
}

void AmmUiBackend::probeNetworkIdentity()
{
    if (m_identityProbeInFlight
        || m_identityRetryTimer->isActive()
        || !m_network.isConfigured()
        || sequencerAddr().isEmpty()) {
        return;
    }
    m_identityProbeInFlight = true;
    m_network.beginIdentityProbe();
    publishNetworkContext();
    const QString address = sequencerAddr();
    const bool devnet = m_network.isDevnet();
    const QString method = devnet
        ? QStringLiteral("getChannelId")
        : QStringLiteral("getBlock");
    const QJsonArray params = devnet
        ? QJsonArray()
        : QJsonArray { CHECKPOINT_BLOCK_ID };

    QNetworkRequest request{QUrl(address)};
    request.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));
    request.setTransferTimeout(4000);
    m_sequencer->applyAuthorization(request);
    QNetworkReply* reply = m_net->post(request, jsonRpcBody(method, params));
    connect(reply, &QNetworkReply::finished, this, [this, reply, address, devnet]() {
        m_identityProbeInFlight = false;
        if (address != sequencerAddr()) {
            reply->deleteLater();
            probeNetworkIdentity();
            return;
        }
        if (!sequencerReachable()) {
            reply->deleteLater();
            return;
        }

        const QByteArray payload = reply->readAll();
        const QString actual = devnet
            ? channelIdFromResponse(payload)
            : blockHashFromResponse(payload);
        m_network.finishIdentityProbe(actual);
        const int retryDelay = m_network.identityRetryDelayMs();
        if (retryDelay > 0)
            m_identityRetryTimer->start(retryDelay);
        reply->deleteLater();
        publishNetworkContext();
    });
}

void AmmUiBackend::publishNetworkContext()
{
    refreshNewPositionContext(m_newPositionHints);
}

void AmmUiBackend::watchTransaction(const QVariantMap& result)
{
    if (result.value(QStringLiteral("status")).toString()
            != QStringLiteral("submitted")) {
        return;
    }
    const QString nativeHash = result.value(
        QStringLiteral("nativeTransactionHash")).toString();
    bool deadlineValid = false;
    const qint64 deadline = result.value(QStringLiteral("deadlineMs"))
        .toString().toLongLong(&deadlineValid);
    QStringList affected;
    for (const QVariant& value : result.value(
             QStringLiteral("affectedAccountIds")).toList()) {
        affected.append(value.toString());
    }
    if (nativeHash.isEmpty() || !deadlineValid || affected.isEmpty())
        return;
    m_pendingTransactions.insert(nativeHash, {
        nativeHash,
        affected,
        deadline,
    });
    if (!m_transactionTimer->isActive())
        m_transactionTimer->start();
    QTimer::singleShot(0, this, &AmmUiBackend::pollTransactions);
}

void AmmUiBackend::pollTransactions()
{
    const qint64 now = QDateTime::currentMSecsSinceEpoch();
    const QList<PendingTransaction> pending = m_pendingTransactions.values();
    for (const PendingTransaction& transaction : pending) {
        if (now >= transaction.deadlineMs) {
            m_pendingTransactions.remove(transaction.nativeHash);
            continue;
        }
        m_sequencer->queryTransaction(transaction.nativeHash,
            [this, transaction](bool ok, bool included) {
                if (!ok || !included
                    || !m_pendingTransactions.contains(transaction.nativeHash)) {
                    return;
                }
                m_pendingTransactions.remove(transaction.nativeHash);
                refreshAffectedAccounts(transaction.affectedAccountIds);
                if (m_pendingTransactions.isEmpty())
                    m_transactionTimer->stop();
            });
    }
    if (m_pendingTransactions.isEmpty())
        m_transactionTimer->stop();
}

void AmmUiBackend::refreshAffectedAccounts(const QStringList& accountIds, int attempt)
{
    m_sequencer->readAccounts(accountIds, true,
        [this, accountIds, attempt](QVector<WalletAccountRead> reads) {
            QStringList failed;
            for (qsizetype index = 0; index < reads.size(); ++index) {
                if (!reads.at(index).ok())
                    failed.append(accountIds.value(index));
            }
            if (!failed.isEmpty() && attempt < 2) {
                QTimer::singleShot(1000, this,
                    [this, failed, attempt]() {
                        refreshAffectedAccounts(failed, attempt + 1);
                    });
                return;
            }
            refreshNewPositionContext({});
        });
}
