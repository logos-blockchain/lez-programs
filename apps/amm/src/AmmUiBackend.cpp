#include "AmmUiBackend.h"

#include <QDebug>
#include <QTimer>

#include "LogosWalletProvider.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

namespace {
    // The new-position context placeholder published before the module
    // connection is up (matches the module's "loading" contextState).
    QVariantMap loadingContext()
    {
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("loading") },
            { QStringLiteral("networkId"), QStringLiteral("lez") },
            { QStringLiteral("networkFingerprint"), QString() },
            { QStringLiteral("tokens"), QVariantList() },
            { QStringLiteral("feeTiers"), QVariantList() },
            { QStringLiteral("warnings"), QVariantList() },
        };
    }

    // A new-position error envelope (matches the module's publicError), for
    // the backend-side failure paths (e.g. LP-account creation failing).
    QVariantMap newPositionError(const QString& code)
    {
        return QVariantMap {
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("canSubmit"), false },
            { QStringLiteral("code"), code },
            { QStringLiteral("errors"), QVariantList { QVariantMap {
                { QStringLiteral("code"), code },
                { QStringLiteral("recoverable"), true },
                { QStringLiteral("blockingFields"), QVariantList() },
                { QStringLiteral("details"), QVariantMap() },
            } } },
            { QStringLiteral("warnings"), QVariantList() },
            { QStringLiteral("accountPreview"), QVariantList() },
        };
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
    if (!walletStateReady()) {
        setNewPositionContext(loadingContext());
        return;
    }
    setNewPositionContext(m_logos->amm_module.newPositionContext(
        request, isWalletOpen(), refreshWalletAccounts));
}

QVariantMap AmmUiBackend::quoteNewPosition(QVariantMap request)
{
    return m_logos->amm_module.quoteNewPosition(request, isWalletOpen());
}

QVariantMap AmmUiBackend::submitNewPosition(QVariantMap request, QString quoteHash)
{
    // First attempt with no LP account. If the module needs a fresh LP holding
    // it returns "requires_fresh_lp" without submitting; we own wallet-keyset
    // mutation, so create the account here (keeping the account model + on-disk
    // storage coherent) and resubmit with its id.
    QVariantMap result = m_logos->amm_module.submitNewPosition(
        request, quoteHash, isWalletOpen(), QString());

    if (result.value(QStringLiteral("status")).toString()
        == QStringLiteral("requires_fresh_lp")) {
        const QString lpId = m_walletController->createAccount(true);
        if (lpId.isEmpty())
            return newPositionError(QStringLiteral("wallet_submission_failed"));
        result = m_logos->amm_module.submitNewPosition(
            request, quoteHash, isWalletOpen(), lpId);
    }

    if (result.value(QStringLiteral("status")).toString() == QStringLiteral("submitted"))
        refreshBalances();
    return result;
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

    publishNetworkContext();
}

void AmmUiBackend::publishNetworkContext()
{
    if (!walletStateReady()) {
        setNewPositionContext(loadingContext());
        return;
    }
    setNewPositionContext(m_logos->amm_module.newPositionContext(
        m_newPositionHints, isWalletOpen(), false));
}

QVariantMap AmmUiBackend::resolvePool(QString defAHex, QString defBHex)
{
    return m_logos->amm_module.resolvePool(defAHex, defBHex);
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

QVariantMap AmmUiBackend::liquidityQuote(QVariantMap request)
{
    // Read-only create-pool preview — no wallet guard. The module prices the opening
    // LP and price server-side from the two deposit amounts.
    return m_logos->amm_module.liquidityQuote(request);
}

QVariantList AmmUiBackend::tokenHoldings()
{
    // Read-only list of the wallet's token holdings for the account selector. Gated
    // by this app's wallet-open state (a closed wallet has nothing to list).
    return m_logos->amm_module.tokenHoldings(isWalletOpen());
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
