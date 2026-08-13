#include "AmmUiBackend.h"

#include <QByteArray>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QStandardPaths>
#include <QTimer>

#include "LogosWalletProvider.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

namespace {
    // Absolute path to the JSON known-pools config consumed by poolList().
    // Mirrors TOKENS_CONFIG for the token list; produced by the AMM testnet
    // setup script (apps/amm/tests/testnet/setup-amm-testnet.sh).
    constexpr char POOLS_CONFIG_ENV[] = "AMM_POOLS_CONFIG";

    // Parses the AMM_POOLS_CONFIG JSON file into the QVariantList the Pools UI
    // renders. Fails soft (empty list) when the env var is unset, the file is
    // unreadable, or the payload is not a JSON array — one malformed entry is
    // skipped rather than dropping the whole list. tokenA/tokenB (display
    // symbols) and a numeric feeBps are required; the id fields pass through
    // when present so the entry can later be resolved on-chain.
    QVariantList readPoolsConfig()
    {
        QVariantList out;

        const QString path = qEnvironmentVariable(POOLS_CONFIG_ENV);
        if (path.isEmpty())
            return out;

        QFile file(path);
        if (!file.open(QIODevice::ReadOnly | QIODevice::Text))
            return out;

        const QJsonDocument doc = QJsonDocument::fromJson(file.readAll());
        if (!doc.isArray())
            return out;

        for (const QJsonValue& entry : doc.array()) {
            if (!entry.isObject())
                continue;
            const QJsonObject obj = entry.toObject();

            const QString tokenA = obj.value(QStringLiteral("tokenA")).toString();
            const QString tokenB = obj.value(QStringLiteral("tokenB")).toString();
            const QJsonValue feeBps = obj.value(QStringLiteral("feeBps"));
            if (tokenA.isEmpty() || tokenB.isEmpty() || !feeBps.isDouble())
                continue;

            QVariantMap pool;
            pool.insert(QStringLiteral("tokenA"), tokenA);
            pool.insert(QStringLiteral("tokenB"), tokenB);
            pool.insert(QStringLiteral("feeBps"), feeBps.toInt());
            pool.insert(QStringLiteral("poolId"),
                        obj.value(QStringLiteral("poolId")).toString());
            pool.insert(QStringLiteral("tokenADefinitionId"),
                        obj.value(QStringLiteral("tokenADefinitionId")).toString());
            pool.insert(QStringLiteral("tokenBDefinitionId"),
                        obj.value(QStringLiteral("tokenBDefinitionId")).toString());
            out.append(pool);
        }
        return out;
    }
}


AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_logos(std::make_unique<LogosModules>(m_logosAPI)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("AmmUI")))
{
    setWalletStateReady(false);

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    // Publishes an initial "loading" context (walletStateReady is still false,
    // so it does not yet reach the module).
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

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();

    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);
}

QVariantMap AmmUiBackend::resolvePoolAccount(QString defAHex, QString defBHex)
{
    return m_logos->amm_module.resolvePoolAccount(defAHex, defBHex);
}

QVariantMap AmmUiBackend::configAccount()
{
    return m_logos->amm_module.configAccount();
}

QVariantMap AmmUiBackend::transferOwnership(QVariantMap request)
{
    // Submit guard — this app's wallet-open state is authoritative even though the shared
    // wallet may remain open elsewhere (same guard as createPool / the swaps).
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };

    // No balance refresh — transferring admin authority doesn't touch token balances.
    return m_logos->amm_module.transferOwnership(request);
}

QString AmmUiBackend::swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                      QString userOutputHoldingHex, QString amountInDecimal,
                                      QString minOutDecimal, QString deadlineDecimal)
{
    // This app's connected state is the authoritative submit guard. disconnectWallet()
    // only locks this UI and leaves the shared logos_execution_zone wallet open (another
    // app may keep it open, or this app opened-then-disconnected), and the QML submit path
    // doesn't check isWalletOpen — so without this a swap could sign/submit while the UI
    // shows "Connect".
    if (!isWalletOpen())
        return {};

    const QString txHash = m_logos->amm_module.swapExactInput(
        defAHex, defBHex, userInputHoldingHex, userOutputHoldingHex,
        amountInDecimal, minOutDecimal, deadlineDecimal);
    if (!txHash.isEmpty())
        refreshBalances();
    return txHash;
}

QVariantMap AmmUiBackend::swapExactInQuote(QString tokenInHex, QString tokenOutHex,
                                            QString amountInDecimal, int slippageBps)
{
    // Read-only preview — no wallet guard. The module reads the pool and prices
    // the swap server-side; the returned envelope orients reserves and computes
    // expectedOut/minReceived/priceImpact via the same formula the chain uses.
    return m_logos->amm_module.swapExactInQuote(
        tokenInHex, tokenOutHex, amountInDecimal, slippageBps);
}

QVariantMap AmmUiBackend::swapExactOutQuote(QString tokenInHex, QString tokenOutHex,
                                             QString amountOutDecimal, int slippageBps)
{
    // Read-only preview — the exact-output counterpart of swapExactInQuote:
    // prices the input required for a desired output and its slippage ceiling.
    return m_logos->amm_module.swapExactOutQuote(
        tokenInHex, tokenOutHex, amountOutDecimal, slippageBps);
}

QString AmmUiBackend::swapExactOutput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                       QString userOutputHoldingHex, QString amountOutDecimal,
                                       QString maxInDecimal, QString deadlineDecimal)
{
    // Same connected-state submit guard as swapExactInput — this app's lock is
    // authoritative even though the shared wallet may remain open elsewhere.
    if (!isWalletOpen())
        return {};

    const QString txHash = m_logos->amm_module.swapExactOutput(
        defAHex, defBHex, userInputHoldingHex, userOutputHoldingHex,
        amountOutDecimal, maxInDecimal, deadlineDecimal);
    if (!txHash.isEmpty())
        refreshBalances();
    return txHash;
}

QVariantList AmmUiBackend::tokenList()
{
    return m_logos->amm_module.tokenList();
}

QVariantMap AmmUiBackend::createPoolQuote(QVariantMap request)
{
    // Read-only create-pool preview — no wallet guard. The module prices the opening
    // LP and price server-side from the two deposit amounts.
    return m_logos->amm_module.createPoolQuote(request);
}

QVariantMap AmmUiBackend::addLiquidityQuote(QVariantMap request)
{
    // Read-only add-liquidity preview — no wallet guard. The module reads the pool and
    // ratio-matches the deposit server-side from the two max amounts.
    return m_logos->amm_module.addLiquidityQuote(request);
}

QVariantList AmmUiBackend::tokenHoldings()
{
    // Read-only list of the wallet's token holdings for the account selector. Gated
    // by this app's wallet-open state (a closed wallet has nothing to list).
    return m_logos->amm_module.tokenHoldings(isWalletOpen());
}

QVariantList AmmUiBackend::poolList()
{
    // Config-driven known pools. Read straight from AMM_POOLS_CONFIG on every
    // call (the UI fetches this once on load); adding more pairs is a config
    // edit, no app change. Pool discovery is an app concern, so this stays in
    // the backend rather than the amm_module.
    return readPoolsConfig();
}

QVariantList AmmUiBackend::feeTiers()
{
    // Pure, input-free enumeration of the AMM's supported fee tiers (raw bps) —
    // no wallet or module connection state involved.
    return m_logos->amm_module.feeTiers();
}

QVariantList AmmUiBackend::resolveTokens()
{
    // The app owns the token set: the configured tokens (TOKENS_CONFIG) and the user's
    // persisted custom ids — the same "known list" shape the swap side shows. Tokens the
    // wallet merely holds are NOT auto-listed here; to provide liquidity with an unlisted
    // token the user adds it by id (addCustomToken). The module still annotates
    // holdingId/balance for whichever of these ids the wallet does hold.
    const bool wallet_open = isWalletOpen();

    QVariantList ids;
    const QVariantList configured = m_logos->amm_module.tokenList();
    for (const QVariant& entry : configured) {
        const QString id = entry.toMap().value(QStringLiteral("definitionId")).toString();
        if (!id.isEmpty())
            ids.append(id);
    }
    const QStringList custom = loadCustomTokenIds();
    for (const QString& id : custom)
        ids.append(id);

    QVariantMap request;
    request.insert(QStringLiteral("tokenIds"), ids);
    return m_logos->amm_module.resolveTokens(request, wallet_open);
}

QVariantMap AmmUiBackend::addCustomToken(QString tokenId)
{
    const QString id = tokenId.trimmed();
    if (id.isEmpty())
        return QVariantMap{{QStringLiteral("ok"), false},
                           {QStringLiteral("error"), QStringLiteral("unresolved")}};

    // Validate before persisting: resolve just this id and keep it only if it is a
    // real fungible token (a non-fungible / unreadable id yields no row).
    QVariantMap probe;
    probe.insert(QStringLiteral("tokenIds"), QVariantList{id});
    const QVariantList rows = m_logos->amm_module.resolveTokens(probe, isWalletOpen());
    if (rows.isEmpty())
        return QVariantMap{{QStringLiteral("ok"), false},
                           {QStringLiteral("error"), QStringLiteral("unresolved")}};

    const QVariantMap token = rows.first().toMap();
    const QString canonicalId = token.value(QStringLiteral("definitionId")).toString();
    if (canonicalId.isEmpty())
        return QVariantMap{{QStringLiteral("ok"), false},
                           {QStringLiteral("error"), QStringLiteral("unresolved")}};

    QStringList custom = loadCustomTokenIds();
    if (!custom.contains(canonicalId)) {
        custom.append(canonicalId);
        if (!saveCustomTokenIds(custom))
            return QVariantMap{{QStringLiteral("ok"), false},
                               {QStringLiteral("error"), QStringLiteral("backend_error")}};
    }
    return QVariantMap{{QStringLiteral("ok"), true}, {QStringLiteral("token"), token}};
}

QString AmmUiBackend::customTokenStorePath() const
{
    // A dedicated store path via CUSTOM_TOKEN_CONFIG (akin to the module's env-configured
    // TOKENS_CONFIG). Otherwise per-user app data — but in a QML plugin with no
    // QCoreApplication application name that can come back empty, so fall back to a fixed
    // dot-dir under HOME. Persistence must never silently no-op on an empty path.
    const QByteArray env = qgetenv("CUSTOM_TOKEN_CONFIG");
    if (!env.isEmpty())
        return QString::fromLocal8Bit(env);
    const QString appData = QStandardPaths::writableLocation(QStandardPaths::AppDataLocation);
    const QString dir = !appData.isEmpty()
        ? appData
        : QDir(QDir::homePath()).filePath(QStringLiteral(".logos-amm"));
    return QDir(dir).filePath(QStringLiteral("amm-custom-tokens.json"));
}

QStringList AmmUiBackend::loadCustomTokenIds() const
{
    const QString path = customTokenStorePath();
    if (path.isEmpty())
        return {};
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    const QByteArray bytes = file.readAll();
    file.close();

    QJsonParseError error{};
    const QJsonDocument doc = QJsonDocument::fromJson(bytes, &error);
    if (error.error != QJsonParseError::NoError || !doc.isArray())
        return {};

    QStringList ids;
    for (const QJsonValue& value : doc.array()) {
        const QString id = value.toString().trimmed();
        if (!id.isEmpty() && !ids.contains(id))
            ids.append(id);
    }
    return ids;
}

bool AmmUiBackend::saveCustomTokenIds(const QStringList& ids) const
{
    const QString path = customTokenStorePath();
    if (path.isEmpty()) {
        qWarning() << "AmmUiBackend: no custom-token store path; not persisting custom tokens";
        return false;
    }
    QDir().mkpath(QFileInfo(path).absolutePath());

    QJsonArray array;
    for (const QString& id : ids)
        array.append(id);

    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        qWarning() << "AmmUiBackend: cannot write custom-token store" << path << file.errorString();
        return false;
    }
    file.write(QJsonDocument(array).toJson(QJsonDocument::Compact));
    file.close();
    return true;
}

QVariantMap AmmUiBackend::createPool(QVariantMap request)
{
    // Same connected-state submit guard as the swaps — this app's lock is
    // authoritative even though the shared wallet may remain open elsewhere.
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };

    // The caller supplies the fresh LP holding in the request; the backend forwards to
    // the module (it creates no wallet accounts) and refreshes balances on a successful
    // submit.
    const QVariantMap result = m_logos->amm_module.createPool(request);
    if (result.value(QStringLiteral("status")).toString() == QStringLiteral("ok")
        && !result.value(QStringLiteral("transactionId")).toString().isEmpty())
        refreshBalances();
    return result;
}

QVariantMap AmmUiBackend::addLiquidity(QVariantMap request)
{
    // Same connected-state submit guard as createPool — this app's lock is authoritative
    // even though the shared wallet may remain open elsewhere.
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };

    // The caller supplies the fresh LP holding in the request; the backend forwards to the
    // module (it creates no wallet accounts) and refreshes balances on a successful submit.
    const QVariantMap result = m_logos->amm_module.addLiquidity(request);
    if (result.value(QStringLiteral("status")).toString() == QStringLiteral("ok")
        && !result.value(QStringLiteral("transactionId")).toString().isEmpty())
        refreshBalances();
    return result;
}
