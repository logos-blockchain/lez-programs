#include "AmmUiBackend.h"

#include <algorithm>

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QUrl>

#include "LogosWalletProvider.h"
#include "WalletAccountId.h"
#include "WalletController.h"
#include "WalletIdlDecoder.h"
#include "logos_api.h"

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
      m_networkManager(new QNetworkAccessManager(this)),
      m_tokenIdl(resource(QStringLiteral(":/amm/idl/token-idl.json"))),
      m_ammIdl(resource(QStringLiteral(":/amm/idl/amm-idl.json")))
{
    setAssets({});
    setAssetStatus(QStringLiteral("idle"));
    setAssetError({});
    m_network.load();
    m_idlRegistry.registerProgram(
        m_network.snapshot().ammProgramId, QStringLiteral("AMM"), m_ammIdl);
    publishNetworkState();
    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    connect(m_walletController.get(), &WalletController::snapshotChanged,
            this, &AmmUiBackend::refreshPortfolio);
    syncWalletState();
    m_walletController->start();
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
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

QString AmmUiBackend::createNewDefault(QString password)
{
    return m_walletController->createDefaultWallet(password);
}

QString AmmUiBackend::createNew(QString configPath,
                                QString storagePath,
                                QString password)
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

bool AmmUiBackend::setAccountAlias(QString accountId, QString alias)
{
    return m_walletController->setAccountAlias(accountId, alias);
}

bool AmmUiBackend::setPrimaryAccount(QString accountId)
{
    return m_walletController->setPrimaryAccount(accountId);
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    const QString previousAddress = sequencerAddr();
    const bool wasReachable = sequencerReachable();
    setIsWalletOpen(state.isWalletOpen);
    setWalletStateReady(state.syncStatus != QStringLiteral("opening")
                        && state.syncStatus != QStringLiteral("syncing"));
    setWalletSyncStatus(state.syncStatus);
    setWalletSyncError(state.syncError);
    setWalletCanSubmit(state.canSubmit());
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

    const bool addressChanged = previousAddress != state.sequencerAddress;
    if (addressChanged)
        m_network.sequencerChanged(!state.sequencerAddress.isEmpty());
    if (addressChanged || wasReachable != state.sequencerReachable)
        m_network.reachabilityChanged(state.sequencerReachable, wasReachable);
    publishNetworkState();
    if (state.sequencerReachable && m_network.needsIdentityProbe())
        probeNetworkIdentity();
}

void AmmUiBackend::publishNetworkState()
{
    const ActiveNetworkSnapshot network = m_network.snapshot();
    setActiveNetwork(network.id);
    setNetworkStatus(network.status);
    setNetworkFingerprint(network.fingerprint);
}

void AmmUiBackend::probeNetworkIdentity()
{
    if (m_identityProbeInFlight || !m_network.isConfigured() || sequencerAddr().isEmpty())
        return;
    m_identityProbeInFlight = true;
    m_network.beginIdentityProbe();
    publishNetworkState();
    const QString address = sequencerAddr();
    const bool devnet = m_network.isDevnet();
    const QString method = devnet ? QStringLiteral("getChannelId")
                                  : QStringLiteral("getBlock");
    const QJsonArray params = devnet ? QJsonArray()
                                     : QJsonArray { CHECKPOINT_BLOCK_ID };
    QNetworkRequest request{QUrl(address)};
    request.setHeader(QNetworkRequest::ContentTypeHeader,
                      QStringLiteral("application/json"));
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_networkManager->post(request, jsonRpcBody(method, params));
    connect(reply, &QNetworkReply::finished, this, [this, reply, address, devnet]() {
        m_identityProbeInFlight = false;
        if (address != sequencerAddr()) {
            reply->deleteLater();
            probeNetworkIdentity();
            return;
        }
        const QByteArray payload = reply->readAll();
        const QString identity = devnet ? channelIdFromResponse(payload)
                                        : blockHashFromResponse(payload);
        m_network.finishIdentityProbe(identity);
        reply->deleteLater();
        publishNetworkState();
        refreshPortfolio();
    });
}

void AmmUiBackend::refreshPortfolio()
{
    const quint64 generation = ++m_portfolioGeneration;
    if (!m_walletController->state().isWalletOpen) {
        setAssets({});
        setAssetStatus(QStringLiteral("idle"));
        setAssetError({});
        return;
    }
    if (m_network.status() != QStringLiteral("ready")) {
        setAssets({});
        setAssetStatus(QStringLiteral("blocked"));
        setAssetError(m_network.status());
        return;
    }
    if (m_tokenIdl.isEmpty()) {
        setAssetStatus(QStringLiteral("error"));
        setAssetError(QStringLiteral("token_idl_missing"));
        return;
    }
    setAssetStatus(QStringLiteral("loading"));
    setAssetError({});
    m_wallet->readPublicAccountsAsync(
        m_network.snapshot().tokenIds,
        [this, generation](QVector<WalletAccountRead> reads) {
            applyDefinitions(generation, reads);
        });
}

void AmmUiBackend::applyDefinitions(
    quint64 generation,
    const QVector<WalletAccountRead>& reads)
{
    if (generation != m_portfolioGeneration)
        return;
    const ActiveNetworkSnapshot network = m_network.snapshot();
    const WalletDecodeResult decoded = WalletIdlDecoder::decode(m_tokenIdl, reads);
    if (!decoded.ok() || reads.size() != network.tokenIds.size()
        || decoded.accounts.size() != reads.size()) {
        setAssetStatus(QStringLiteral("error"));
        setAssetError(decoded.error.isEmpty()
                          ? QStringLiteral("definition_decode_failed")
                          : decoded.error);
        return;
    }

    m_tokens.clear();
    m_tokenProgramId.clear();
    int unavailable = 0;
    for (qsizetype index = 0; index < reads.size(); ++index) {
        const WalletAccountRead& read = reads.at(index);
        const WalletDecodedAccount& account = decoded.accounts.at(index);
        TokenInfo token;
        token.id = network.tokenIds.at(index);
        token.name = QStringLiteral("Unknown token");
        token.status = QStringLiteral("unavailable");
        const QJsonObject fungible = enumFields(account.value, QStringLiteral("Fungible"));
        if (read.ok() && account.status == QStringLiteral("decoded")
            && account.typeName == QStringLiteral("TokenDefinition")
            && !fungible.isEmpty() && read.programOwner != DEFAULT_PROGRAM_OWNER) {
            token.name = fungible.value(QStringLiteral("name")).toString().trimmed();
            if (token.name.isEmpty())
                token.name = QStringLiteral("Unnamed token");
            token.programOwner = read.programOwner;
            token.status = QStringLiteral("ready");
            if (m_tokenProgramId.isEmpty())
                m_tokenProgramId = read.programOwner;
            else if (m_tokenProgramId != read.programOwner) {
                setAssets({});
                setAssetStatus(QStringLiteral("error"));
                setAssetError(QStringLiteral("token_program_mismatch"));
                return;
            }
        } else {
            ++unavailable;
        }
        m_tokens.append(std::move(token));
    }
    if (m_tokenProgramId.isEmpty()) {
        setAssets({});
        setAssetStatus(QStringLiteral("error"));
        setAssetError(QStringLiteral("definitions_unavailable"));
        return;
    }
    m_idlRegistry.registerProgram(
        m_tokenProgramId, QStringLiteral("Token"), m_tokenIdl);
    setAssetError(unavailable > 0
                      ? QStringLiteral("some_definitions_unavailable")
                      : QString());
    applyWalletPortfolio(generation);
}

void AmmUiBackend::applyWalletPortfolio(quint64 generation)
{
    if (generation != m_portfolioGeneration)
        return;
    const WalletSnapshot snapshot = m_walletController->snapshot();
    QVector<WalletAccountRead> programReads;
    for (const WalletAccount& account : snapshot.accounts) {
        if (!account.isPublic || account.readStatus != QStringLiteral("ok"))
            continue;
        programReads.append(accountRead(account));
    }

    QHash<QString, QString> balances;
    QVector<WalletAccountPresentation> presentations;
    const QVector<WalletDecodedProgram> programs = m_idlRegistry.decode(programReads);
    for (const WalletDecodedProgram& program : programs) {
        for (const WalletDecodedAccount& account : program.result.accounts) {
            WalletAccountPresentation presentation;
            presentation.address = account.id;
            presentation.programName = program.programName;
            presentation.accountType = account.typeName;
            if (program.programId == m_tokenProgramId
                && account.typeName == QStringLiteral("TokenHolding")) {
                const QJsonObject fungible = enumFields(
                    account.value, QStringLiteral("Fungible"));
                if (fungible.isEmpty())
                    continue;
                const QString encodedId = fungible
                    .value(QStringLiteral("definition_id")).toString();
                const QString definitionId = account.accountIds.value(encodedId);
                const QString amount = fungible.value(QStringLiteral("balance")).toString();
                const QString current = balances.value(definitionId, QStringLiteral("0"));
                const QString total = decimalAdd(current, amount);
                if (!definitionId.isEmpty() && !total.isEmpty())
                    balances.insert(definitionId, total);
                presentation.kind = QStringLiteral("token_holding");
                presentation.definitionId = definitionId;
                presentation.hiddenFromAccounts = true;
                for (const TokenInfo& token : m_tokens) {
                    if (token.id == definitionId) {
                        presentation.semanticName = token.name + QStringLiteral(" holding");
                        break;
                    }
                }
            } else if (program.programId == m_tokenProgramId
                       && account.typeName == QStringLiteral("TokenDefinition")) {
                presentation.kind = QStringLiteral("token_definition");
                const QJsonObject fungible = enumFields(
                    account.value, QStringLiteral("Fungible"));
                presentation.semanticName = fungible.value(QStringLiteral("name")).toString();
            } else if (program.programId == m_tokenProgramId
                       && account.typeName == QStringLiteral("TokenMetadata")) {
                presentation.kind = QStringLiteral("token_metadata");
            } else {
                presentation.kind = QStringLiteral("program");
                presentation.semanticName = account.typeName;
            }
            presentations.append(std::move(presentation));
        }
    }
    m_walletController->applyAccountPresentations(presentations);

    QVariantList assets;
    QVariantList available;
    int unavailableCount = 0;
    for (const TokenInfo& token : m_tokens) {
        const QString balance = balances.value(token.id, QStringLiteral("0"));
        const bool positive = balance != QStringLiteral("0");
        QString displayDefinitionId = walletAccountIdToBase58(token.id);
        if (displayDefinitionId.isEmpty())
            displayDefinitionId = token.id;
        QVariantMap asset {
            { QStringLiteral("name"), token.name },
            { QStringLiteral("symbol"), token.name },
            { QStringLiteral("balance"), balance },
            { QStringLiteral("definitionId"), token.id },
            { QStringLiteral("displayDefinitionId"), displayDefinitionId },
            { QStringLiteral("programOwner"), token.programOwner },
            { QStringLiteral("status"), token.status },
            { QStringLiteral("section"), positive ? QStringLiteral("assets")
                                                    : QStringLiteral("available") },
        };
        if (positive)
            assets.append(std::move(asset));
        else
            available.append(std::move(asset));
        if (token.status != QStringLiteral("ready"))
            ++unavailableCount;
    }
    assets.append(available);
    setAssets(assets);
    setAssetStatus(unavailableCount > 0 ? QStringLiteral("partial")
                                        : QStringLiteral("ready"));
}
