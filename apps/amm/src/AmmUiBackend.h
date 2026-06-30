#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <QObject>
#include <QJsonArray>
#include <QJsonObject>
#include <QString>
#include <QVariantList>
#include <QVariantMap>

#include "rep_AmmUiBackend_source.h"

#include "AccountModel.h"

class LogosAPI;
struct LogosModules;
class QNetworkAccessManager;
class QTimer;

// Source-side implementation of the AmmUiBackend .rep interface.
// Inheriting from AmmUiBackendSimpleSource gives us the generated PROPs and
// SLOTs from AmmUiBackend.rep — all the simple ones flow over QtRO. Talks to
// the core logos_execution_zone wallet module via LogosModules.
class AmmUiBackend : public AmmUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(AccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit AmmUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~AmmUiBackend() override;

    AccountModel* accountModel() const { return m_accountModel; }

public slots:
    // Overrides of the pure-virtual slots generated from the .rep.
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    void refreshAccounts() override;
    void refreshBalances() override;
    QString getBalance(QString accountIdHex, bool isPublic) override;
    QString submitSwap(QVariantMap snapshot) override;
    QString submitLiquidity(QVariantMap snapshot) override;
    QString createNewDefault(QString password) override;
    QString createNew(QString configPath, QString storagePath, QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;
    bool changeSequencerAddr(QString url) override;

private:
    // Canonical LEZ wallet home shared with the wallet UI and other apps.
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;

    void persistConfigPath(const QString& path);
    void persistStoragePath(const QString& path);
    QJsonArray listAccounts();
    void openOrAdoptWallet();
    bool adoptOpenWallet();
    void refreshBlockHeights();
    void refreshSequencerAddr();
    void loadDeploymentConfig();
    void selectDeploymentForNetwork(const QString& network);
    void selectDeploymentForChain(const QString& network,
                                  const QString& blockHash,
                                  const QString& blockSignature);
    void clearDeploymentSelection(const QString& network);
    void setDeploymentIdentityPendingIfNeeded(bool pending);
    void verifyDeploymentTransactions();
    void refreshDeploymentWalletState();
    void updateDeploymentNetworkMatched();
    QJsonObject configuredTokenDefinition(const QString& symbol, int fallbackIndex) const;
    QString accountIdHex(const QString& accountId) const;
    QStringList accountIdHexList(const QStringList& accountIds, QString* error) const;
    struct PoolChainState {
        double reserveA = 0;
        double reserveB = 0;
        double totalLpSupply = 0;
        double feeBps = 0;
        bool found = false;
    };
    PoolChainState poolChainState() const;
    struct WalletFungibleHolding {
        QString accountIdHex;
        double balance = 0;
        bool found = false;
        bool ambiguous = false;
    };
    WalletFungibleHolding walletFungibleHolding(const QString& definitionAccountId,
                                                const QString& accountIdFilterHex = {}) const;
    QString selectedWalletAccountIdHex(const QVariantMap& snapshot, QString* error) const;
    QString submitAmmTransaction(const QStringList& accountIds,
                                 const QVariantList& signingRequirements,
                                 const QVariantList& instruction);
    void saveWallet();

    // Probe the configured sequencer over HTTP and update sequencerReachable.
    void checkReachability();
    void probeChainIdentity(const QString& network);

    AccountModel* m_accountModel;

    LogosAPI* m_logosAPI;
    LogosModules* m_logos;
    QJsonArray m_tokenChains;
    QJsonArray m_ammChains;
    QJsonArray m_programChainGroups;
    QString m_activeDeploymentNetwork;
    bool m_activeDeploymentConfigured = false;
    bool m_activeDeploymentDeployed = false;
    bool m_identityProbeInFlight = false;
    QStringList m_requiredDeploymentTransactions;
    int m_pendingDeploymentChecks = 0;
    quint64 m_deploymentCheckGeneration = 0;
    quint64 m_reachabilityProbeGeneration = 0;
    quint64 m_chainIdentityProbeGeneration = 0;
    bool m_deploymentChecksFailed = false;
    QJsonArray m_tokenDefinitions;
    QJsonObject m_poolConfig;

    QNetworkAccessManager* m_net;
    QTimer* m_reachabilityTimer;
};

#endif // AMM_UI_BACKEND_H
