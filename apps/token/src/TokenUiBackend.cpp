#include "TokenUiBackend.h"

#include <QMap>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include "LogosWalletProvider.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

namespace {

QVariantMap errorResult(const QString& error)
{
    return {
        { QStringLiteral("status"), QStringLiteral("error") },
        { QStringLiteral("error"), error },
    };
}

bool succeeded(const QVariantMap& result)
{
    return result.value(QStringLiteral("status")).toString() == QStringLiteral("ok");
}

QString displayMetadataStandard(const QString& standard)
{
    if (standard.isEmpty())
        return {};
    QString display = standard.toLower();
    display[0] = display.at(0).toUpper();
    return display;
}

QVariantMap normalizeHolding(const QVariantMap& account)
{
    const QString kind = account.value(QStringLiteral("kind")).toString();
    QVariantMap holding {
        { QStringLiteral("id"), account.value(QStringLiteral("accountId")) },
        { QStringLiteral("wallet"), QStringLiteral("connected wallet") },
        { QStringLiteral("role"), kind },
    };

    if (kind == QStringLiteral("fungible")) {
        const QString balance = account.value(QStringLiteral("balanceRaw")).toString();
        holding.insert(QStringLiteral("rawBalance"), balance);
        holding.insert(QStringLiteral("displayBalance"), balance);
    } else if (kind == QStringLiteral("nftMaster")) {
        holding.insert(QStringLiteral("printBalance"),
                      account.value(QStringLiteral("printBalanceRaw")).toString());
    } else if (kind == QStringLiteral("nftPrintedCopy")) {
        holding.insert(QStringLiteral("owned"), account.value(QStringLiteral("owned")));
    }

    return holding;
}

}

TokenUiBackend::TokenUiBackend(LogosAPI* logosAPI, QObject* parent)
    : TokenUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("token_ui", this)),
      m_logos(std::make_unique<LogosModules>(m_logosAPI)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("TokenUI")))
{
    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &TokenUiBackend::syncWalletState);
    syncWalletState();
    m_walletController->start();
}

TokenUiBackend::~TokenUiBackend() = default;

WalletAccountModel* TokenUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString TokenUiBackend::createAccountPublic()
{
    return m_walletController->createAccount(true);
}

QString TokenUiBackend::createAccountPrivate()
{
    return m_walletController->createAccount(false);
}

void TokenUiBackend::refreshAccounts()
{
    m_walletController->refresh();
}

void TokenUiBackend::refreshBalances()
{
    m_walletController->refresh();
}

QString TokenUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_walletController->balance(accountIdHex, isPublic);
}

QString TokenUiBackend::createNewDefault(QString password)
{
    const QString mnemonic = m_walletController->createDefaultWallet(password);
    syncWalletState();
    return mnemonic;
}

QString TokenUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    const QString mnemonic = m_walletController->createWallet(
        configPath, storagePath, password);
    syncWalletState();
    return mnemonic;
}

bool TokenUiBackend::openExisting()
{
    const bool opened = m_walletController->open();
    syncWalletState();
    return opened;
}

void TokenUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
    syncWalletState();
}

void TokenUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);
}

QVariantMap TokenUiBackend::walletUnavailable() const
{
    return errorResult(QStringLiteral("wallet_unavailable"));
}

QVariantMap TokenUiBackend::refreshAfterSubmit(QVariantMap result)
{
    if (succeeded(result)
        && !result.value(QStringLiteral("transactionId")).toString().isEmpty()) {
        m_walletController->refresh();
    }
    return result;
}

QVariantMap TokenUiBackend::tokenProgramInfo()
{
    return m_logos->token_module.programInfo();
}

QVariantMap TokenUiBackend::inspectDefinition(QString definitionId)
{
    return m_logos->token_module.inspectDefinition(definitionId);
}

QVariantMap TokenUiBackend::inspectHolding(QString holdingId)
{
    return m_logos->token_module.inspectHolding(holdingId);
}

QVariantMap TokenUiBackend::inspectMetadata(QString metadataId)
{
    return m_logos->token_module.inspectMetadata(metadataId);
}

QVariantMap TokenUiBackend::walletTokenAccounts()
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return m_logos->token_module.walletTokenAccounts();
}

QVariantList TokenUiBackend::walletDefinitions()
{
    if (!m_walletController->state().isWalletOpen)
        return {};

    const QVariantMap accountsResult = m_logos->token_module.walletTokenAccounts();
    if (!succeeded(accountsResult))
        return {};

    QMap<QString, QString> definitionIds;
    QMap<QString, QVariantList> holdingsByDefinition;
    const QVariantList accounts = accountsResult.value(QStringLiteral("accounts")).toList();
    for (const QVariant& value : accounts) {
        const QVariantMap account = value.toMap();
        const QString accountType = account.value(QStringLiteral("accountType")).toString();
        const QString accountId = account.value(QStringLiteral("accountId")).toString();
        const QString accountHex = account.value(QStringLiteral("accountIdHex")).toString();

        if (accountType == QStringLiteral("definition")) {
            const QString key = accountHex.isEmpty() ? accountId : accountHex;
            if (!key.isEmpty())
                definitionIds.insert(key, accountId);
            continue;
        }

        if (accountType != QStringLiteral("holding"))
            continue;
        const QString definitionId = account.value(QStringLiteral("definitionId")).toString();
        const QString definitionHex = account.value(QStringLiteral("definitionIdHex")).toString();
        const QString key = definitionHex.isEmpty() ? definitionId : definitionHex;
        if (key.isEmpty())
            continue;
        if (!definitionIds.contains(key))
            definitionIds.insert(key, definitionId);
        holdingsByDefinition[key].append(account);
    }

    QVariantList records;
    for (auto definition = definitionIds.cbegin(); definition != definitionIds.cend(); ++definition) {
        const QVariantMap inspected = m_logos->token_module.inspectDefinition(definition.value());
        if (!succeeded(inspected))
            continue;

        const QVariantMap raw = inspected.value(QStringLiteral("definition")).toMap();
        if (raw.isEmpty())
            continue;

        const QString kind = raw.value(QStringLiteral("kind")).toString();
        const bool fungible = kind == QStringLiteral("fungible");
        const QString id = raw.value(QStringLiteral("accountId")).toString();
        const QString idHex = raw.value(QStringLiteral("accountIdHex")).toString();
        const QString metadataId = raw.value(QStringLiteral("metadataId")).toString();
        const QString authority = raw.value(QStringLiteral("mintAuthorityId")).toString();

        QVariantMap record {
            { QStringLiteral("id"), id },
            { QStringLiteral("name"), raw.value(QStringLiteral("name")) },
            { QStringLiteral("symbol"), QString() },
            { QStringLiteral("type"), kind },
            { QStringLiteral("definitionId"), id },
            { QStringLiteral("definitionHex"), idHex },
            { QStringLiteral("metadataId"), metadataId },
            { QStringLiteral("source"), QStringLiteral("network") },
        };

        if (fungible) {
            const QString authorityMode = authority.isEmpty()
                ? QStringLiteral("fixed")
                : authority == id ? QStringLiteral("self") : QStringLiteral("external");
            const QString supply = raw.value(QStringLiteral("totalSupplyRaw")).toString();
            record.insert(QStringLiteral("rawSupply"), supply);
            record.insert(QStringLiteral("displaySupply"), supply);
            record.insert(QStringLiteral("inferredDecimals"), QString());
            record.insert(QStringLiteral("authorityMode"), authorityMode);
            record.insert(QStringLiteral("authority"), authority);
            record.insert(QStringLiteral("authorityLabel"), QString());
            record.insert(QStringLiteral("printableCopies"), QVariant());
            record.insert(QStringLiteral("masterHolding"), QVariant());
            record.insert(QStringLiteral("instruction"), metadataId.isEmpty()
                              ? QStringLiteral("createFungible")
                              : QStringLiteral("createFungibleWithMetadata"));
        } else {
            const QString printableSupply =
                raw.value(QStringLiteral("printableSupplyRaw")).toString();
            record.insert(QStringLiteral("rawSupply"), QVariant());
            record.insert(QStringLiteral("displaySupply"), QVariant());
            record.insert(QStringLiteral("inferredDecimals"), QVariant());
            record.insert(QStringLiteral("authorityMode"), QStringLiteral("masterHolding"));
            record.insert(QStringLiteral("authority"), QVariant());
            record.insert(QStringLiteral("authorityLabel"), QString());
            record.insert(QStringLiteral("printableCopies"), printableSupply);
            record.insert(QStringLiteral("instruction"), QStringLiteral("createNonFungible"));
        }

        QVariantMap normalizedDefinition {
            { QStringLiteral("id"), id },
            { QStringLiteral("hex"), idHex },
            { QStringLiteral("name"), raw.value(QStringLiteral("name")) },
            { QStringLiteral("type"), kind },
            { QStringLiteral("metadataId"), metadataId },
        };
        if (fungible) {
            normalizedDefinition.insert(QStringLiteral("totalSupplyRaw"),
                                         raw.value(QStringLiteral("totalSupplyRaw")));
            normalizedDefinition.insert(QStringLiteral("mintAuthority"), authority);
        } else {
            normalizedDefinition.insert(QStringLiteral("printableSupply"),
                                         raw.value(QStringLiteral("printableSupplyRaw")));
        }
        record.insert(QStringLiteral("definition"), normalizedDefinition);

        if (!metadataId.isEmpty()) {
            const QVariantMap metadataResult = m_logos->token_module.inspectMetadata(metadataId);
            if (succeeded(metadataResult)) {
                const QVariantMap rawMetadata = metadataResult.value(QStringLiteral("metadata")).toMap();
                const QString standard = displayMetadataStandard(
                    rawMetadata.value(QStringLiteral("standard")).toString());
                QVariantMap metadata {
                    { QStringLiteral("id"), rawMetadata.value(QStringLiteral("accountId")) },
                    { QStringLiteral("standard"), standard },
                    { QStringLiteral("uri"), rawMetadata.value(QStringLiteral("uri")) },
                    { QStringLiteral("creators"), rawMetadata.value(QStringLiteral("creators")) },
                };
                record.insert(QStringLiteral("metadata"), metadata);
                record.insert(QStringLiteral("metadataStandard"), standard);
                record.insert(QStringLiteral("metadataUri"),
                              rawMetadata.value(QStringLiteral("uri")));
                record.insert(QStringLiteral("creators"),
                              rawMetadata.value(QStringLiteral("creators")));
            }
        }

        QVariantList normalizedHoldings;
        const QVariantList holdings = holdingsByDefinition.value(definition.key());
        for (const QVariant& holdingValue : holdings)
            normalizedHoldings.append(normalizeHolding(holdingValue.toMap()));
        record.insert(QStringLiteral("holdings"), normalizedHoldings);

        if (!normalizedHoldings.isEmpty()) {
            const QVariantMap firstHolding = normalizedHoldings.first().toMap();
            record.insert(QStringLiteral("holding"), firstHolding);
            record.insert(QStringLiteral("holdingId"), firstHolding.value(QStringLiteral("id")));
            if (!fungible && firstHolding.value(QStringLiteral("role")).toString()
                    == QStringLiteral("nftMaster")) {
                record.insert(QStringLiteral("masterHolding"),
                              firstHolding.value(QStringLiteral("id")));
            }
        } else {
            record.insert(QStringLiteral("holdingId"), QString());
        }

        records.append(record);
    }

    return records;
}

QVariantMap TokenUiBackend::createFungible(QString definitionTargetId,
                                            QString holdingTargetId, QString name,
                                            QString totalSupplyRaw, QString mintAuthority)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.createFungible(
        definitionTargetId, holdingTargetId, name,
        QVariant::fromValue(totalSupplyRaw), mintAuthority));
}

QVariantMap TokenUiBackend::createFungibleWithMetadata(
    QString definitionTargetId, QString holdingTargetId, QString metadataTargetId,
    QString name, QString totalSupplyRaw, QString mintAuthority,
    QString metadataStandard, QString uri, QString creators)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.createFungibleWithMetadata(
        definitionTargetId, holdingTargetId, metadataTargetId, name,
        QVariant::fromValue(totalSupplyRaw), mintAuthority, metadataStandard, uri,
        creators));
}

QVariantMap TokenUiBackend::createNonFungible(
    QString definitionTargetId, QString masterHoldingTargetId,
    QString metadataTargetId, QString name, QString printableSupplyRaw,
    QString metadataStandard, QString uri, QString creators)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.createNonFungible(
        definitionTargetId, masterHoldingTargetId, metadataTargetId, name,
        QVariant::fromValue(printableSupplyRaw), metadataStandard, uri, creators));
}

QVariantMap TokenUiBackend::initializeHolding(QString definitionId, QString holdingTargetId)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(
        m_logos->token_module.initializeHolding(definitionId, holdingTargetId));
}

QVariantMap TokenUiBackend::transfer(QString senderHoldingId, QString recipientHoldingId,
                                     QString amountRaw)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.transfer(
        senderHoldingId, recipientHoldingId, QVariant::fromValue(amountRaw)));
}

QVariantMap TokenUiBackend::burn(QString definitionId, QString holdingId, QString amountRaw)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.burn(
        definitionId, holdingId, QVariant::fromValue(amountRaw)));
}

QVariantMap TokenUiBackend::mint(QString definitionId, QString holdingId, QString amountRaw)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.mint(
        definitionId, holdingId, QVariant::fromValue(amountRaw)));
}

QVariantMap TokenUiBackend::mintWithAuthority(QString definitionId, QString holdingId,
                                               QString authorityId, QString amountRaw)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.mintWithAuthority(
        definitionId, holdingId, authorityId, QVariant::fromValue(amountRaw)));
}

QVariantMap TokenUiBackend::setAuthority(QString definitionId, QString newAuthority)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(
        m_logos->token_module.setAuthority(definitionId, newAuthority));
}

QVariantMap TokenUiBackend::setAuthorityWithAuthority(QString definitionId,
                                                       QString authorityId,
                                                       QString newAuthority)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(m_logos->token_module.setAuthorityWithAuthority(
        definitionId, authorityId, newAuthority));
}

QVariantMap TokenUiBackend::printNft(QString masterHoldingId,
                                     QString printedHoldingTargetId)
{
    if (!m_walletController->state().isWalletOpen)
        return walletUnavailable();
    return refreshAfterSubmit(
        m_logos->token_module.printNft(masterHoldingId, printedHoldingTargetId));
}
