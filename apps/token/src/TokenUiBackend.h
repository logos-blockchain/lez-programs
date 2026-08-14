#ifndef TOKEN_UI_BACKEND_H
#define TOKEN_UI_BACKEND_H

#include <memory>

#include <QObject>
#include <QString>
#include <QVariantMap>
#include <QVariantList>

#include "rep_TokenUiBackend_source.h"

#include "WalletAccountModel.h"

class LogosAPI;
struct LogosModules;
class LogosWalletProvider;
class WalletController;

// Source-side implementation of the Token UI view contract. Wallet lifecycle
// stays in the reusable shared wallet classes; token reads and submissions are
// forwarded to the token_module core module.
class TokenUiBackend : public TokenUiBackendSimpleSource {
    Q_OBJECT
    Q_PROPERTY(WalletAccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit TokenUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr);
    ~TokenUiBackend() override;

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

    QVariantMap tokenProgramInfo() override;
    QVariantMap inspectDefinition(QString definitionId) override;
    QVariantMap inspectHolding(QString holdingId) override;
    QVariantMap inspectMetadata(QString metadataId) override;
    QVariantMap walletTokenAccounts() override;
    QVariantList walletDefinitions() override;

    QVariantMap createFungible(QString definitionTargetId, QString holdingTargetId,
                               QString name, QString totalSupplyRaw,
                               QString mintAuthority) override;
    QVariantMap createFungibleWithMetadata(QString definitionTargetId,
                                            QString holdingTargetId,
                                            QString metadataTargetId, QString name,
                                            QString totalSupplyRaw,
                                            QString mintAuthority,
                                            QString metadataStandard, QString uri,
                                            QString creators) override;
    QVariantMap createNonFungible(QString definitionTargetId,
                                  QString masterHoldingTargetId,
                                  QString metadataTargetId, QString name,
                                  QString printableSupplyRaw,
                                  QString metadataStandard, QString uri,
                                  QString creators) override;

    QVariantMap initializeHolding(QString definitionId, QString holdingTargetId) override;
    QVariantMap transfer(QString senderHoldingId, QString recipientHoldingId,
                          QString amountRaw) override;
    QVariantMap burn(QString definitionId, QString holdingId, QString amountRaw) override;
    QVariantMap mint(QString definitionId, QString holdingId, QString amountRaw) override;
    QVariantMap mintWithAuthority(QString definitionId, QString holdingId,
                                  QString authorityId, QString amountRaw) override;
    QVariantMap setAuthority(QString definitionId, QString newAuthority) override;
    QVariantMap setAuthorityWithAuthority(QString definitionId, QString authorityId,
                                          QString newAuthority) override;
    QVariantMap printNft(QString masterHoldingId, QString printedHoldingTargetId) override;

private:
    void syncWalletState();
    QVariantMap walletUnavailable() const;
    QVariantMap refreshAfterSubmit(QVariantMap result);

    LogosAPI* m_logosAPI;
    std::unique_ptr<LogosModules> m_logos;
    std::unique_ptr<LogosWalletProvider> m_wallet;
    std::unique_ptr<WalletController> m_walletController;
};

#endif // TOKEN_UI_BACKEND_H
