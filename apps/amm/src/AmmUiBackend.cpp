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
#include <QSettings>
#include <QStandardPaths>
#include <QTimer>

#include "LogosWalletProvider.h"
#include "RegistryLoader.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

namespace {
// Global (per-user) settings store, shared with WalletController's scope
// (QSettings("Logos", "AmmUI")). The registry URL is a per-user setting, not
// per-wallet, so it lives here rather than in the wallet home.
const char SETTINGS_ORG[] = "Logos";
const char SETTINGS_APP[] = "AmmUI";
const char REGISTRY_URL_KEY[] = "registryUrl";
}

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_logos(std::make_unique<LogosModules>(m_logosAPI)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("AmmUI"))),
      m_registry(std::make_unique<RegistryLoader>())
{
    setWalletStateReady(false);

    // Whenever the known-tokens / known-pools snapshot refreshes: adopt the active
    // network's AMM program id on the module (empty ⇒ falls back to AMM_PROGRAM_BIN)
    // so ops target that network without a bin, then bump registryRevision so QML
    // replicas re-fetch tokenList()/poolList()/resolveTokens().
    connect(m_registry.get(), &RegistryLoader::changed, this, [this]() {
        m_logos->amm_module.setAmmProgramId(QVariantMap{
            {QStringLiteral("ammProgramId"), m_registry->activeAmmProgramId()}});
        setRegistryRevision(m_registry->revision());
    });

    // Seed the configured registry URL from the persisted global setting so the
    // first refresh() and the config field both see it (AMM_REGISTRY_URL overrides).
    const QString configuredUrl = loadRegistryUrlSetting();
    setRegistryUrl(configuredUrl);
    m_registry->setConfiguredUrl(configuredUrl);

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    // Publishes an initial "loading" context (walletStateReady is still false,
    // so it does not yet reach the module).
    syncWalletState();
    m_walletController->start();
    QTimer::singleShot(0, this, [this]() {
        setWalletStateReady(true);
        syncWalletState();
        // Load the registry once the event loop is running (the remote source
        // fetches asynchronously). The changed() handler adopts the selected
        // network's program id on the module.
        m_registry->refresh();
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

QVariantMap AmmUiBackend::createPriceObservations(QVariantMap request)
{
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };
    // Oracle setup doesn't touch token balances — no refresh.
    return m_logos->amm_module.createPriceObservations(request);
}

QVariantMap AmmUiBackend::createOraclePriceAccount(QVariantMap request)
{
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };
    return m_logos->amm_module.createOraclePriceAccount(request);
}

QString AmmUiBackend::swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                      QString userOutputHoldingHex, QString amountInDecimal,
                                      QString minOutDecimal, QString deadlineDecimal)
{
    // This app's connected state is the authoritative submit guard. disconnectWallet()
    // only locks this UI and leaves the shared lez_core wallet open (another
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
    // Config-driven token list, read straight from TOKENS_CONFIG (like poolList
    // reads AMM_POOLS_CONFIG). Token discovery is an app concern, so this stays
    // in the backend rather than the amm_module; the swap/quote module methods
    // normalize the ids (base58 or hex) at their boundary. Served from the
    // RegistryLoader snapshot (re-fetched when registryRevision changes).
    return m_registry->tokens();
}

void AmmUiBackend::refreshRegistry()
{
    // Manual re-load of the known-tokens/known-pools source. The loader bumps
    // registryRevision and the UI re-fetches the lists.
    m_registry->refresh();
}

void AmmUiBackend::saveRegistryUrl(QString url)
{
    // Persist the user's registry URL (global setting), publish it to the config
    // field, and re-load from it. AMM_REGISTRY_URL still overrides on refresh().
    const QString trimmed = url.trimmed();
    storeRegistryUrlSetting(trimmed);
    setRegistryUrl(trimmed);
    m_registry->setConfiguredUrl(trimmed);
    m_registry->refresh();
}

QString AmmUiBackend::loadRegistryUrlSetting() const
{
    return QSettings(QString::fromLatin1(SETTINGS_ORG), QString::fromLatin1(SETTINGS_APP))
        .value(QString::fromLatin1(REGISTRY_URL_KEY))
        .toString();
}

void AmmUiBackend::storeRegistryUrlSetting(const QString& url) const
{
    QSettings settings(QString::fromLatin1(SETTINGS_ORG), QString::fromLatin1(SETTINGS_APP));
    if (url.isEmpty())
        settings.remove(QString::fromLatin1(REGISTRY_URL_KEY));
    else
        settings.setValue(QString::fromLatin1(REGISTRY_URL_KEY), url);
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

QVariantMap AmmUiBackend::removeLiquidityQuote(QVariantMap request)
{
    // Read-only remove-liquidity preview — no wallet guard, mirroring the add quote.
    // The module reads the pool and runs the guest's floor(reserve*lp/supply) math.
    return m_logos->amm_module.removeLiquidityQuote(request);
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
    // the backend rather than the amm_module. Served from the RegistryLoader snapshot.
    return m_registry->pools();
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
    const QVariantList configured = m_registry->tokens();
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
    QVariantList rows = m_logos->amm_module.resolveTokens(request, wallet_open);

    // The module resolves on-chain fields (definitionId/name/holding/balance) but
    // not the UI-only `symbol`, which lives in TOKENS_CONFIG. Re-attach it here so
    // the liquidity token picker derives the same colored avatars as the swap side
    // (TokenVisuals derives color/letter from the symbol). Custom ids not in the
    // config keep no symbol; the picker falls back to the name for those.
    for (QVariant& row : rows) {
        QVariantMap token = row.toMap();
        if (!token.value(QStringLiteral("symbol")).toString().isEmpty()) {
            row = token;
            continue;
        }
        const QString id = token.value(QStringLiteral("definitionId")).toString();
        for (const QVariant& entry : configured) {
            const QVariantMap cfg = entry.toMap();
            if (cfg.value(QStringLiteral("definitionId")).toString() == id) {
                token.insert(QStringLiteral("symbol"), cfg.value(QStringLiteral("symbol")));
                if (token.value(QStringLiteral("name")).toString().isEmpty())
                    token.insert(QStringLiteral("name"), cfg.value(QStringLiteral("name")));
                break;
            }
        }
        row = token;
    }
    return rows;
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

QVariantMap AmmUiBackend::removeLiquidity(QVariantMap request)
{
    // Same connected-state submit guard as createPool/addLiquidity — this app's lock is
    // authoritative even though the shared wallet may remain open elsewhere.
    if (!isWalletOpen())
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("error"), QStringLiteral("wallet_unavailable") },
        };

    // Every account involved already exists (the LP holding is burned from, the token
    // holdings receive), so unlike the add path there is nothing to create first —
    // forward and refresh balances once the withdrawal lands.
    const QVariantMap result = m_logos->amm_module.removeLiquidity(request);
    if (result.value(QStringLiteral("status")).toString() == QStringLiteral("ok")
        && !result.value(QStringLiteral("transactionId")).toString().isEmpty())
        refreshBalances();
    return result;
}
