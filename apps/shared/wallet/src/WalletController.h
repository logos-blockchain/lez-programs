#pragma once

#include <QObject>
#include <QHash>
#include <QString>
#include <QVector>

#include "WalletProvider.h"

class QNetworkAccessManager;
class QNetworkReply;
class QTimer;
class WalletAccountModel;
struct WalletAccountPresentation;

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
    QString syncStatus = QStringLiteral("closed");
    QString syncError;
    QString primaryAccountAddress;
    QString primaryAccountName;

    bool canSubmit() const
    {
        return isWalletOpen && syncStatus == QStringLiteral("ready");
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
    QString createDefaultWallet(const QString& password);
    QString createWallet(const QString& configPath,
                         const QString& storagePath,
                         const QString& password);
    bool open();
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
    void applySnapshot(const WalletSnapshot& snapshot);
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
    bool m_started = false;
    quint64 m_operationGeneration = 0;
    quint64 m_reachabilityGeneration = 0;
};
