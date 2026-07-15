#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>

#include <QString>
#include <QVariantList>
#include <QVariantMap>

#include "rep_AmmUiBackend_source.h"

#include "WalletAccountModel.h"

extern "C" {
#include "amm_client_ffi.h"
}

class LogosAPI;
struct LogosModules;
class LogosWalletProvider;
class WalletController;

class AmmUiBackend : public AmmUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit AmmUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~AmmUiBackend() override;

    WalletAccountModel* accountModel() const;

public slots:
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    void refreshAccounts() override;
    void refreshBalances() override;
    QString getBalance(QString accountIdHex, bool isPublic) override;
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
};

#endif // AMM_UI_BACKEND_H
