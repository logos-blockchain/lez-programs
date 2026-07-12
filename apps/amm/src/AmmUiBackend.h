#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <QObject>
#include <QJsonArray>
#include <QJsonObject>
#include <QString>
#include <QStringList>
#include <QVariant>

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
    // The injected provider must outlive the backend.
    explicit AmmUiBackend(WalletProvider& wallet, QObject* parent = nullptr);
    ~AmmUiBackend() override;

    AccountModel* accountModel() const { return m_accountModel; }

public slots:
    // Overrides of the pure-virtual slots generated from the .rep.
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    void refreshAccounts() override;
    void refreshBalances() override;
    QString getBalance(QString accountIdHex, bool isPublic) override;
    QVariantMap refreshNewPositionContext(QVariantMap request) override;
    QVariantMap quoteNewPosition(QVariantMap request) override;
    QVariantMap submitNewPosition(QVariantMap request, QString quoteHash) override;
    // Return the new wallet's BIP39 mnemonic (empty string on failure) so the
    // UI can force a one-time seed-phrase backup step.
    QString createNewDefault(QString password) override;
    QString createNew(QString configPath, QString storagePath, QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;
    bool changeSequencerAddr(QString url) override;
    void copyToClipboard(QString text) override;

private:
    // Per-app wallet home (kept distinct from the wallet's canonical
    // ~/.lee/wallet so standalone instances stay isolated; Basecamp sharing
    // is handled by adopting an already-open shared wallet on startup).
    static QString defaultWalletHome();
    QString defaultConfigPath() const;
    QString defaultStoragePath() const;

    void persistConfigPath(const QString& path);
    void persistStoragePath(const QString& path);
    void openOrAdoptWallet();
    // True when the shared core already has a wallet open — including a freshly
    // created one with zero accounts. See the definition for why list_accounts()
    // alone is insufficient.
    bool sharedWalletIsOpen();
    void refreshBlockHeights();
    void refreshSequencerAddr();
    void saveWallet();
    bool loadNetworkConfig();
    void probeNetworkIdentity();
    QJsonObject readAccount(const QString& accountId) const;
    QJsonArray walletAccountReads() const;
    QJsonObject buildQuoteInput(const QVariantMap& request, QJsonObject* error) const;

    // Probe the configured sequencer over HTTP and update sequencerReachable.
    void checkReachability();

    AccountModel* m_accountModel;

    LogosAPI* m_logosAPI;
    LogosModules* m_logos;

    QNetworkAccessManager* m_net;
    QTimer* m_reachabilityTimer;

    QString m_networkId;
    QString m_networkStatus = QStringLiteral("config_missing");
    QString m_networkFingerprint;
    QString m_expectedNetworkIdentity;
    QString m_ammProgramId;
    QStringList m_configuredTokenIds;
    bool m_identityProbeInFlight = false;
    bool m_submitInFlight = false;
};

#endif // AMM_UI_BACKEND_H
