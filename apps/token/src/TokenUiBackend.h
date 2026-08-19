#ifndef TOKEN_UI_BACKEND_H
#define TOKEN_UI_BACKEND_H

#include <memory>

#include <QObject>

#include "rep_TokenUiBackend_source.h"

#include "WalletAccountModel.h"

class LogosAPI;
class LogosWalletProvider;
class WalletController;

// Token UI backend. Wallet lifecycle and account state are delegated to the
// repository's shared WalletController; this class only adapts that state to
// the TokenUiBackend QtRO contract.
class TokenUiBackend : public TokenUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit TokenUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr)
        ;
    ~TokenUiBackend() override;

    WalletAccountModel* accountModel() const;

public slots:
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    QString createNewDefault(QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;

private:
    void syncWalletState();

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;
};

#endif // TOKEN_UI_BACKEND_H
