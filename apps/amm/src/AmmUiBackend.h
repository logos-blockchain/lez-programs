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

#include "ActiveNetwork.h"
#include "WalletAccountModel.h"

class LogosAPI;
struct LogosModules;
class AmmClient;
class LogosWalletProvider;
class NewPositionRuntime;
class SwapRuntime;
class WalletController;

// Source-side implementation of the AmmUiBackend .rep interface.
// Inheriting from AmmUiBackendSimpleSource gives us the generated PROPs and
// SLOTs from AmmUiBackend.rep — all the simple ones flow over QtRO.
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
    QVariantMap submitNewPosition(QVariantMap request, QString quoteHash) override;
    // Return the new wallet's BIP39 mnemonic (empty string on failure) so the
    // UI can force a one-time seed-phrase backup step.
    QString createNewDefault(QString password) override;
    QString createNew(QString configPath, QString storagePath, QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;

    // AMM
    QVariantMap resolvePool(QString defAHex, QString defBHex) override;
    QString swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                            QString userOutputHoldingHex, QString amountInDecimal,
                            QString minOutDecimal, QString deadlineDecimal) override;
    // Reads the token list from TOKENS_CONFIG (see AmmUiBackend.cpp) so the
    // Swap UI's token picker is config-driven instead of hardcoded.
    QVariantList tokenList() override;

private:
    void syncWalletState();
    void publishNetworkContext();

    // Builds the new-position network context from the same sources the Swap
    // view uses: ammProgramId from $AMM_PROGRAM_BIN, tokenIds from
    // $TOKENS_CONFIG. status is "ready" once AMM_PROGRAM_BIN resolves, else
    // "config_missing". There is no separate network config or channel probe.
    ActiveNetworkSnapshot networkSnapshot();

    // 64-char lowercase-hex AMM program id derived from $AMM_PROGRAM_BIN (empty
    // if unset/unreadable); matches swapExactInput's program-id encoding.
    QString ammProgramIdHex();

    // Normalizes an account id given as either 64-char lowercase/uppercase hex
    // or base58 to lowercase hex. Returns an empty QString if `id` is neither
    // (or the base58 decode fails), so callers can detect and skip it.
    QString normalizeAccountId(const QString& id);

    // Returns the deployed AMM program-binary bytes (a RISC Zero ProgramBinary
    // .bin, not a raw ELF) from $AMM_PROGRAM_BIN, or an empty QByteArray (with a
    // qWarning) if the env var is unset/unreadable/empty.
    QByteArray loadAmmElf();

    LogosAPI* m_logosAPI;
    // Direct module handle for the AMM/swap path (resolvePool/swapExactInput/
    // tokenList). The shared wallet provider exposes only wallet-level ops, not
    // the raw account-id / get_account_public / send_generic_public_transaction
    // calls the AMM path needs, so keep a thin LogosModules over the same
    // LogosAPI as the wallet provider.
    std::unique_ptr<LogosModules> m_logos;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;
    std::unique_ptr<AmmClient> m_ammClient;
    std::unique_ptr<NewPositionRuntime> m_newPosition;
    std::unique_ptr<SwapRuntime> m_swap;

    QVariantMap m_newPositionHints;

    // Network context is derived from $AMM_PROGRAM_BIN + $TOKENS_CONFIG, which are
    // fixed for the process lifetime — resolve them once and cache. networkSnapshot()
    // runs on the hot path (every quote) and from inside runtime callbacks, and
    // tokenList() makes remote base58 conversions, so it must not recompute each call.
    bool m_networkResolved = false;
    QString m_ammProgramIdCache;
    QStringList m_tokenIdsCache;
};

#endif // AMM_UI_BACKEND_H
