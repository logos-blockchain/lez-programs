#pragma once

#include <QObject>
#include <QHash>
#include <QString>
#include <QStringList>
#include <QVector>

#include "WalletProvider.h"

class QNetworkAccessManager;
class QNetworkReply;
class QTimer;
class WalletAccountModel;
struct WalletAccountPresentation;

struct WalletUiState {
    // Changes on connection lifecycle and observed account-identity changes.
    // The shared core exposes no notification for unobserved external restores.
    quint64 sessionRevision = 0;
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
    // The funded public account that pays transaction fees (LEZ charges public
    // transactions). Empty when the wallet is closed or no account is funded.
    QString primaryAccountAddress;
    QString primaryAccountName;
    QString feePayerId;
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
    const WalletSnapshot& snapshot() const { return m_snapshot; }

    void start();
    void setDefaultSequencerAddress(const QString& address);
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
    void cancelSync();
    void disconnect();
    bool setAccountAlias(const QString& address, const QString& alias);
    bool setPrimaryAccount(const QString& address);
    void applyAccountPresentations(
        const QVector<WalletAccountPresentation>& presentations);

signals:
    void stateChanged();
    void snapshotChanged();

private:
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;
    bool seedDefaultWalletConfig(const QString& configPath) const;

    void openOnStartup();
    bool beginOpen(const QString& config, const QString& storage);
    void applySnapshotData(const WalletSnapshot& snapshot, bool newSession = false);
    void applySnapshot(const WalletSnapshot& snapshot, bool newSession = false);
    void selectFeePayer(const QVector<WalletAccount>& accounts);
    void applySyncProgress(const WalletSyncProgress& progress);
    void refreshSnapshot();
    void pollSnapshot();
    void scheduleSnapshotPoll(bool retry);
    void checkReachability();
    void stopReachability();
    QString walletSettingsGroup() const;
    QHash<QString, QString> loadAliases() const;
    QString loadPrimaryAccount() const;
    void storeAliases(const QHash<QString, QString>& aliases) const;
    void storePrimaryAccount(const QString& address) const;
    void updatePrimaryState(const QString& address);

    WalletProvider& m_wallet;
    QString m_settingsApplication;
    WalletUiState m_state;
    WalletSnapshot m_snapshot;
    QHash<QString, QString> m_aliases;
    WalletAccountModel* m_accountModel;
    QNetworkAccessManager* m_network;
    QNetworkReply* m_reachabilityReply = nullptr;
    QString m_reachabilityEndpoint;
    QString m_defaultSequencerAddress;
    QTimer* m_reachabilityTimer;
    QTimer* m_snapshotPollTimer;
    int m_snapshotRetryDelayMs = 1000;
    int m_refreshFailures = 0;
    bool m_refreshInFlight = false;
    bool m_started = false;
    QVector<WalletAccount> m_accounts;
    QStringList m_accountIdentities;
    quint64 m_reachabilityGeneration = 0;
    quint64 m_operationGeneration = 0;
};
