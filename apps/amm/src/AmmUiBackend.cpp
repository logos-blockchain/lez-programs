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

#include <utility>

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
    setAssets({});
    setAssetStatus(QStringLiteral("idle"));
    setAssetError({});
    m_transactionTimer->setInterval(5000);
    connect(m_transactionTimer, &QTimer::timeout,
            this, &AmmUiBackend::pollTransactions);
    m_identityRetryTimer->setSingleShot(true);
    connect(m_identityRetryTimer, &QTimer::timeout,
            this, &AmmUiBackend::probeNetworkIdentity);
    m_network.load();
    m_walletController->setDefaultSequencerAddress(m_network.snapshot().sequencerAddress);

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    connect(m_walletController.get(), &WalletController::snapshotChanged,
            this, [this]() {
                if (m_walletController->state().isWalletOpen)
                    m_walletSnapshotPending = true;
            });
    syncWalletState();
    m_walletController->start();
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString AmmUiBackend::createNewDefault(QString password)
{
    return m_walletController->createDefaultWallet(password);
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    return m_walletController->createWallet(configPath, storagePath, password);
}

bool AmmUiBackend::openExisting()
{
    return m_walletController->open();
}

void AmmUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
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

bool AmmUiBackend::setAccountAlias(QString accountId, QString alias)
{
    return m_walletController->setAccountAlias(accountId, alias);
}

bool AmmUiBackend::setPrimaryAccount(QString accountId)
{
    return m_walletController->setPrimaryAccount(accountId);
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
                publishWalletAssets(result);
                setNewPositionContext(std::move(result));
            }
        });
}

void AmmUiBackend::requestNewPositionQuote(QVariantMap request,
                                           int requestId,
                                           bool forceRefresh,
                                           bool isPoolProbe)
{
    m_newPosition->quoteAsync(
        request, m_network.snapshot(), isWalletOpen(), forceRefresh, isPoolProbe,
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
    const bool walletSnapshotApplied = std::exchange(m_walletSnapshotPending, false);
    const bool walletWasOpen = isWalletOpen();
    const bool walletCouldSubmit = walletCanSubmit();
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
    setPrimaryAccountAddress(state.primaryAccountAddress);
    setPrimaryAccountName(state.primaryAccountName);

    m_sequencer->configure(state.configPath, state.sequencerAddress);

    const bool addressChanged = previousAddress != state.sequencerAddress;
    if ((walletCouldSubmit && !state.canSubmit()) || addressChanged)
        m_newPosition->cancelSubmit();
    if (addressChanged) {
        m_identityRetryTimer->stop();
        m_pendingTransactions.clear();
        m_transactionPollsInFlight.clear();
        m_transactionTimer->stop();
        m_network.sequencerChanged(!state.sequencerAddress.isEmpty());
    }
    const bool reachabilityChanged = wasReachable != state.sequencerReachable;
    const bool walletClosed = walletWasOpen && !state.isWalletOpen;
    if (addressChanged || reachabilityChanged) {
        m_identityRetryTimer->stop();
        m_network.reachabilityChanged(state.sequencerReachable, wasReachable);
    }
    if (walletClosed) {
        m_newPosition->clearWalletAccounts();
        setAssets({});
        setAssetStatus(QStringLiteral("idle"));
        setAssetError({});
    }
    if (state.canSubmit() && walletSnapshotApplied) {
        const WalletSnapshot snapshot = m_wallet->snapshot();
        if (snapshot.ok())
            m_newPosition->setWalletAccounts(snapshot.accounts);
    }

    const bool refreshContext = !m_hasPublishedNetworkContext
        || walletSnapshotApplied
        || walletClosed
        || addressChanged
        || reachabilityChanged;
    publishNetworkContext(refreshContext);
    m_hasPublishedNetworkContext = true;
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

        const QVariant statusValue = reply->attribute(
            QNetworkRequest::HttpStatusCodeAttribute);
        const int status = statusValue.toInt();
        const bool successfulResponse = reply->error() == QNetworkReply::NoError
            && statusValue.isValid()
            && status >= 200
            && status < 300;
        const QByteArray payload = successfulResponse ? reply->readAll() : QByteArray();
        const QString actual = successfulResponse
            ? (devnet ? channelIdFromResponse(payload) : blockHashFromResponse(payload))
            : QString();
        m_network.finishIdentityProbe(actual);
        const int retryDelay = m_network.identityRetryDelayMs();
        if (retryDelay > 0)
            m_identityRetryTimer->start(retryDelay);
        reply->deleteLater();
        publishNetworkContext();
    });
}

void AmmUiBackend::publishNetworkContext(bool refreshContext)
{
    const ActiveNetworkSnapshot network = m_network.snapshot();
    setActiveNetwork(network.id);
    setNetworkStatus(network.status);
    setNetworkFingerprint(network.fingerprint);
    if (refreshContext)
        refreshNewPositionContext(m_newPositionHints);
}

void AmmUiBackend::publishWalletAssets(const QVariantMap& context)
{
    const QString contextStatus = context.value(QStringLiteral("status")).toString();
    if (contextStatus == QStringLiteral("no_wallet")) {
        setAssets({});
        setAssetStatus(QStringLiteral("idle"));
        setAssetError({});
        return;
    }
    if (contextStatus != QStringLiteral("ready")) {
        const QString code = context.value(QStringLiteral("code")).toString();
        setAssets({});
        setAssetStatus(contextStatus == QStringLiteral("error")
                           ? QStringLiteral("error")
                           : QStringLiteral("blocked"));
        setAssetError(code.isEmpty() ? contextStatus : code);
        return;
    }

    QVariantList assets;
    QVariantList available;
    QVector<WalletAccountPresentation> presentations;
    bool hasUnavailableToken = false;
    for (const QVariant& value : context.value(QStringLiteral("tokens")).toList()) {
        const QVariantMap token = value.toMap();
        const QString tokenStatus = token.value(QStringLiteral("status")).toString();
        const bool ready = tokenStatus == QStringLiteral("available");
        const QString balance = token.value(QStringLiteral("balanceRaw")).toString();
        const bool hasBalance = ready && !balance.isEmpty() && balance != QStringLiteral("0");
        const QString definitionId = token.value(QStringLiteral("definitionId")).toString();
        QString name = token.value(QStringLiteral("name")).toString().trimmed();
        if (name.isEmpty())
            name = QStringLiteral("Unknown token");
        QVariantMap asset {
            { QStringLiteral("name"), name },
            { QStringLiteral("symbol"), name },
            { QStringLiteral("balance"), balance.isEmpty() ? QStringLiteral("0") : balance },
            { QStringLiteral("definitionId"), definitionId },
            { QStringLiteral("displayDefinitionId"), definitionId },
            { QStringLiteral("programOwner"), token.value(QStringLiteral("ownerProgramId")) },
            { QStringLiteral("status"), ready ? QStringLiteral("ready")
                                                 : QStringLiteral("unavailable") },
            { QStringLiteral("section"), hasBalance ? QStringLiteral("assets")
                                                       : QStringLiteral("available") },
        };
        if (hasBalance)
            assets.append(std::move(asset));
        else
            available.append(std::move(asset));
        for (const QVariant& holdingValue : token.value(QStringLiteral("holdings")).toList()) {
            const QString holdingId = holdingValue.toMap().value(
                QStringLiteral("holdingId")).toString();
            if (holdingId.isEmpty())
                continue;
            presentations.append({
                holdingId,
                QStringLiteral("token_holding"),
                name + QStringLiteral(" holding"),
                QStringLiteral("Token"),
                QStringLiteral("TokenHolding"),
                definitionId,
                true,
            });
        }
        hasUnavailableToken = hasUnavailableToken || !ready;
    }
    for (QVariant& asset : available)
        assets.append(std::move(asset));
    setAssets(assets);
    setAssetStatus(hasUnavailableToken ? QStringLiteral("partial")
                                       : QStringLiteral("ready"));
    setAssetError(hasUnavailableToken
                      ? QStringLiteral("some_definitions_unavailable")
                      : QString());
    m_walletController->applyAccountPresentations(presentations);
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
        if (m_transactionPollsInFlight.contains(transaction.nativeHash))
            continue;
        m_transactionPollsInFlight.insert(transaction.nativeHash);
        m_sequencer->queryTransaction(transaction.nativeHash,
            [this, transaction](bool ok, bool included) {
                m_transactionPollsInFlight.remove(transaction.nativeHash);
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
