#ifndef AMM_UI_BACKEND_H
#define AMM_UI_BACKEND_H

#include <memory>
#include <optional>

#include <QHash>
#include <QString>
#include <QVariantList>

#include "rep_AmmUiBackend_source.h"

#include "ActiveNetwork.h"
#include "TokenDefinitionCache.h"
#include "WalletAccountModel.h"
#include "WalletIdlDecoder.h"

class LogosAPI;
class LogosWalletProvider;
class QNetworkAccessManager;
class WalletController;

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
    QString createAccountPublic() override;
    QString createAccountPrivate() override;
    void refreshAccounts() override;
    void refreshBalances() override;
    QString getBalance(QString accountIdHex, bool isPublic) override;
    QString createNewDefault(QString password) override;
    QString createNew(QString configPath, QString storagePath, QString password) override;
    bool openExisting() override;
    void disconnectWallet() override;
    bool setAccountAlias(QString accountId, QString alias) override;
    bool setPrimaryAccount(QString accountId) override;

private:
    struct TokenInfo {
        QString id;
        QString name;
        QString programOwner;
        QString status;
    };

    void syncWalletState();
    void publishNetworkState();
    void initialize();
    void probeNetworkIdentity();
    void refreshPortfolio();
    TokenDefinitionCacheKey definitionCacheKey(
        const ActiveNetworkSnapshot& network) const;
    void invalidateDefinitionCache();
    void applyDefinitions(quint64 generation,
                          const TokenDefinitionCacheKey& key,
                          const QVector<WalletAccountRead>& reads);
    void applyWalletPortfolio(quint64 generation);

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosWalletProvider> m_ownedWallet;
    WalletProvider* m_wallet;
    TokenDefinitionCache m_definitionCache;
    std::unique_ptr<WalletController> m_walletController;
    QNetworkAccessManager* m_networkManager;
    ActiveNetwork m_network;
    QByteArray m_tokenIdl;
    QByteArray m_ammIdl;
    WalletIdlRegistry m_idlRegistry;
    QVector<TokenInfo> m_tokens;
    QString m_tokenProgramId;
    std::optional<TokenDefinitionCacheKey> m_appliedDefinitionKey;
    bool m_identityProbeInFlight = false;
    quint64 m_portfolioGeneration = 0;
};

#endif // AMM_UI_BACKEND_H
