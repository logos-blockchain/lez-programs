#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>

#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include "rep_AmmUiBackend_source.h"

#include "WalletAccountModel.h"

class LogosAPI;
struct LogosModules;
class LogosWalletProvider;
class WalletController;

// Source-side implementation of the AmmUiBackend .rep interface.
// Inheriting from AmmUiBackendSimpleSource gives us the generated PROPs and
// SLOTs from AmmUiBackend.rep — all the simple ones flow over QtRO.
//
// The AMM business logic (pool resolution, swaps, add-liquidity) lives in the
// amm_module core Logos module; this backend owns only wallet-session lifecycle
// and forwards every AMM slot to modules().amm_module. The one exception is
// creating a fresh LP holding for add-liquidity: that mutates the wallet keyset,
// so it stays here (via the wallet provider, which keeps the account model and
// on-disk storage coherent) and its id is handed to the module's submit.
class AmmUiBackend : public AmmUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit AmmUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~AmmUiBackend() override;

    WalletAccountModel* accountModel() const;

public slots:
    // Overrides of the pure-virtual slots generated from the .rep.
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    void refreshAccounts() override;
    void refreshBalances() override;
    QString getBalance(QString accountIdHex, bool isPublic) override;
    void refreshNewPositionContext(QVariantMap request) override;
    QVariantMap quoteNewPosition(QVariantMap request) override;
    // Return the new wallet's BIP39 mnemonic (empty string on failure) so the
    // UI can force a one-time seed-phrase backup step.
    QString createNewDefault(QString password) override;
    QString createNew(QString configPath, QString storagePath, QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;

    // AMM — all forwarded to the amm_module core module.
    QVariantMap resolvePool(QString defAHex, QString defBHex) override;
    QString swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                            QString userOutputHoldingHex, QString amountInDecimal,
                            QString minOutDecimal, QString deadlineDecimal) override;
    QVariantMap swapExactInQuote(QString tokenInHex, QString tokenOutHex,
                                  QString amountInDecimal, int slippageBps) override;
    QVariantMap swapExactOutQuote(QString tokenInHex, QString tokenOutHex,
                                   QString amountOutDecimal, int slippageBps) override;
    QString swapExactOutput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                             QString userOutputHoldingHex, QString amountOutDecimal,
                             QString maxInDecimal, QString deadlineDecimal) override;
    // Reads the token list from TOKENS_CONFIG (via the module) so the Swap UI's
    // token picker is config-driven instead of hardcoded.
    QVariantList tokenList() override;
    // Create-pool preview (liquidityQuote, read-only) and submit (createPool). The caller
    // supplies lpHoldingId in the request — a fresh account it created via
    // createAccountPublic() — so createPool forwards to the module and creates no wallet
    // accounts here.
    QVariantMap liquidityQuote(QVariantMap request) override;
    QVariantMap createPool(QVariantMap request) override;
    // Add-liquidity submit. Forwards to the module; the flow supplies a fresh LP
    // holding in the request (the backend creates no wallet accounts here).
    QVariantMap addLiquidity(QVariantMap request) override;
    // Lists the wallet's fungible token holdings for the account selector.
    QVariantList tokenHoldings() override;

private:
    void syncWalletState();
    // Publishes the new-position context PROP: a local "loading" placeholder
    // until wallet state (and thus the module connection) is ready, then the
    // module's newPositionContext for the current hints.
    void publishNetworkContext();

    LogosAPI* m_logosAPI;
    // Handle for the amm_module core module (resolvePool / swapExactInput /
    // tokenList / new-position). The module wraps the amm_ffi brain and
    // reaches the shared wallet through its own logos_execution_zone dependency;
    // this backend keeps a thin LogosModules over the same LogosAPI as the
    // wallet provider so both resolve that one shared wallet instance.
    std::unique_ptr<LogosModules> m_logos;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;

    // Sticky new-position hints (recent/resolved token ids) so a bare
    // republish (wallet-state change) keeps the user's last selection.
    QVariantMap m_newPositionHints;
};

#endif // AMM_UI_BACKEND_H
