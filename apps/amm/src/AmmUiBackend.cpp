#include "AmmUiBackend.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QTimer>
#include <QUrl>

#include "AmmClient.h"
#include "LogosWalletProvider.h"
#include "NewPositionRuntime.h"
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
      m_newPosition(std::make_unique<NewPositionRuntime>(m_wallet.get(), m_ammClient.get())),
      m_net(new QNetworkAccessManager(this))
{
    setWalletStateReady(false);
    m_network.load();
    setNewPositionContext(m_newPosition->context(
        QVariantMap(), m_network.snapshot(), false, false));

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    syncWalletState();
    m_walletController->start();
    QTimer::singleShot(0, this, [this]() {
        setWalletStateReady(true);
        syncWalletState();
    });
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString AmmUiBackend::createNewDefault(QString password)
{
    setWalletStateReady(false);
    const QString mnemonic = m_walletController->createDefaultWallet(password);
    setWalletStateReady(true);
    syncWalletState();
    return mnemonic;
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    setWalletStateReady(false);
    const QString mnemonic =
        m_walletController->createWallet(configPath, storagePath, password);
    setWalletStateReady(true);
    syncWalletState();
    return mnemonic;
}

bool AmmUiBackend::openExisting()
{
    setWalletStateReady(false);
    const bool opened = m_walletController->open();
    setWalletStateReady(true);
    syncWalletState();
    return opened;
}

void AmmUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
    setWalletStateReady(true);
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
    if (m_network.status() == QStringLiteral("network_unknown"))
        probeNetworkIdentity();
    setNewPositionContext(m_newPosition->context(
        request, m_network.snapshot(), isWalletOpen(), refreshWalletAccounts));
}

QVariantMap AmmUiBackend::quoteNewPosition(QVariantMap request)
{
    return m_newPosition->quote(request, m_network.snapshot(), isWalletOpen());
}

QVariantMap AmmUiBackend::submitNewPosition(QVariantMap request, QString quoteHash)
{
    return m_newPosition->submit(
        request, quoteHash, m_network.snapshot(), isWalletOpen());
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    const bool walletWasOpen = isWalletOpen();
    const bool wasReachable = sequencerReachable();
    const QString previousAddress = sequencerAddr();

    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);

    const bool addressChanged = previousAddress != state.sequencerAddress;
    if (addressChanged)
        m_network.sequencerChanged(!state.sequencerAddress.isEmpty());
    if (addressChanged || wasReachable != state.sequencerReachable) {
        m_network.reachabilityChanged(state.sequencerReachable, wasReachable);
    }
    if (walletWasOpen && !state.isWalletOpen)
        m_newPosition->clearWalletAccounts();

    publishNetworkContext();
    if (state.sequencerReachable && m_network.needsIdentityProbe())
        probeNetworkIdentity();
}

void AmmUiBackend::probeNetworkIdentity()
{
    if (m_identityProbeInFlight
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
        reply->deleteLater();
        publishNetworkContext();
    });
}

void AmmUiBackend::publishNetworkContext()
{
    setNewPositionContext(m_newPosition->context(
        m_newPositionHints, m_network.snapshot(), isWalletOpen(), false));
}
