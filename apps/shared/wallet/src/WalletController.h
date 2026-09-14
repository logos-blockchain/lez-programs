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
    bool syncProgressKnown = false;
    int syncCurrentBlock = 0;
    int syncTargetBlock = 0;
    int syncRemainingBlocks = 0;
    bool initialSync = false;
    QString sequencerAddress;
    bool sequencerReachable = true;
    QString syncStatus = QStringLiteral("closed");
    QString syncError;

    bool canSubmit() const
    {
        return isWalletOpen && !initialSync;
    }
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
    QString createDefaultWallet(const QString& password);
    QString createWallet(const QString& configPath,
                         const QString& storagePath,
                         const QString& password);
    bool open();
    void cancelSync();
    void disconnect();

signals:
    void stateChanged();

private:
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;

    void openOnStartup();
    bool beginOpen(const QString& config, const QString& storage);
    void applySnapshot(const WalletSnapshot& snapshot);
    void applySyncProgress(const WalletSyncProgress& progress);
    void pollSnapshot();
    void scheduleSnapshotPoll(bool retry);
    void checkReachability();

    WalletProvider& m_wallet;
    QString m_settingsApplication;
    WalletUiState m_state;
    WalletAccountModel* m_accountModel;
    QNetworkAccessManager* m_network;
    QTimer* m_reachabilityTimer;
    QTimer* m_snapshotPollTimer;
    int m_snapshotRetryDelayMs = 1000;
    bool m_started = false;
    quint64 m_operationGeneration = 0;
};
