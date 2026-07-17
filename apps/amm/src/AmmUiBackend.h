#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>

#include <QObject>
#include <QHash>
#include <QSet>
#include <QString>
#include <QStringList>
#include <QVariant>

#include "rep_AmmUiBackend_source.h"

#include "ActiveNetwork.h"
#include "WalletAccountModel.h"

class LogosAPI;
class AmmClient;
class LogosWalletProvider;
class NewPositionRuntime;
class QNetworkAccessManager;
class QTimer;
class SequencerClient;
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
    void requestNewPositionQuote(QVariantMap request,
                                 int requestId,
                                 bool forceRefresh) override;
    void requestNewPositionSubmit(QVariantMap request,
                                  QString quoteHash,
                                  int requestId) override;
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
    void watchTransaction(const QVariantMap& result);
    void pollTransactions();
    void refreshAffectedAccounts(const QStringList& accountIds, int attempt = 0);

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;
    std::unique_ptr<AmmClient> m_ammClient;
    std::unique_ptr<SequencerClient> m_sequencer;
    std::unique_ptr<NewPositionRuntime> m_newPosition;

    QNetworkAccessManager* m_net;
    QTimer* m_transactionTimer;
    QTimer* m_identityRetryTimer;

    ActiveNetwork m_network;
    QVariantMap m_newPositionHints;
    bool m_identityProbeInFlight = false;
    quint64 m_contextGeneration = 0;
    struct PendingTransaction {
        QString nativeHash;
        QStringList affectedAccountIds;
        qint64 deadlineMs = 0;
    };
    QHash<QString, PendingTransaction> m_pendingTransactions;
    QSet<QString> m_transactionPollsInFlight;
};

#endif // AMM_UI_BACKEND_H
