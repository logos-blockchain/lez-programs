#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>

#include <QString>

#include "rep_AmmUiBackend_source.h"

#include "WalletAccountModel.h"

class LogosAPI;
class LogosWalletProvider;
class QNetworkAccessManager;
class QTimer;

class AmmUiBackend : public AmmUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit AmmUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~AmmUiBackend() override;

    WalletAccountModel* accountModel() const { return m_accountModel; }

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

private:
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;

    void persistConfigPath(const QString& path);
    void persistStoragePath(const QString& path);
    void openOrAdoptWallet();
    void applySnapshot(const WalletSnapshot& snapshot);
    void checkReachability();

    WalletAccountModel* m_accountModel;
    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    QNetworkAccessManager* m_net;
    QTimer* m_reachabilityTimer;
};

#endif // AMM_UI_BACKEND_H
