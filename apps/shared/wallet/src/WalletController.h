#pragma once

#include <QObject>
#include <QString>

#include "WalletProvider.h"

class QNetworkAccessManager;
class QTimer;
class WalletAccountModel;

struct WalletUiState {
    bool isWalletOpen = false;
    bool walletExists = false;
    QString configPath;
    QString storagePath;
    QString walletHome;
    int lastSyncedBlock = 0;
    int currentBlockHeight = 0;
    QString sequencerAddress;
    bool sequencerReachable = true;
    // The funded public account that pays transaction fees (LEZ charges public
    // transactions). Empty when the wallet is closed or no account is funded.
    QString feePayerId;
};

class WalletController final : public QObject {
    Q_OBJECT

public:
    // The provider must outlive the controller.
    explicit WalletController(WalletProvider& wallet,
                              QString settingsApplication,
                              QObject* parent = nullptr);
    ~WalletController() override;

    WalletAccountModel* accountModel() const { return m_accountModel; }
    const WalletUiState& state() const { return m_state; }

    void start();
    QString createAccount(bool isPublic);
    void refresh();
    QString balance(const QString& accountId, bool isPublic);
    // Selects the fee payer; only a funded public wallet account is accepted.
    // The choice persists per app across restarts.
    bool setFeePayer(const QString& accountId);
    QString createDefaultWallet(const QString& password);
    QString createWallet(const QString& configPath,
                         const QString& storagePath,
                         const QString& password);
    bool open();
    void disconnect();

signals:
    void stateChanged();

private:
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;

    void openOnStartup();
    void applySnapshot(const WalletSnapshot& snapshot);
    void selectFeePayer(const QVector<WalletAccount>& accounts);
    void checkReachability();

    WalletProvider& m_wallet;
    QString m_settingsApplication;
    WalletUiState m_state;
    WalletAccountModel* m_accountModel;
    QNetworkAccessManager* m_network;
    QTimer* m_reachabilityTimer;
    bool m_started = false;
    QVector<WalletAccount> m_accounts;
};
