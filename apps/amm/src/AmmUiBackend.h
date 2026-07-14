#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>

#include <QObject>
#include <QString>
#include <QVariant>

#include "rep_AmmUiBackend_source.h"

#include "ActiveNetwork.h"
#include "WalletAccountModel.h"

class LogosAPI;
class AmmClient;
class LogosWalletProvider;
class NewPositionRuntime;
class QNetworkAccessManager;
class WalletController;

// Source-side implementation of the AmmUiBackend .rep interface.
// Inheriting from AmmUiBackendSimpleSource gives us the generated PROPs and
// SLOTs from AmmUiBackend.rep — all the simple ones flow over QtRO.
class AmmUiBackend : public AmmUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit AmmUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    // The injected provider must outlive the backend.
    explicit AmmUiBackend(WalletProvider& wallet, QObject* parent = nullptr);
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

private:
    void syncWalletState();
    void probeNetworkIdentity();
    void publishNetworkContext();

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;
    std::unique_ptr<AmmClient> m_ammClient;
    std::unique_ptr<NewPositionRuntime> m_newPosition;

    QNetworkAccessManager* m_net;

    ActiveNetwork m_network;
    QVariantMap m_newPositionHints;
    bool m_identityProbeInFlight = false;
};

#endif // AMM_UI_BACKEND_H
