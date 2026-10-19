#include <memory>
#include <utility>
#include <QJsonObject>
#include <QSettings>
#include <QTemporaryDir>
#include <QtTest>
#include "FakeWalletProvider.h"
#include "WalletController.h"
#include "WalletPortfolioService.h"

namespace {
const QString DEFINITION(64, QLatin1Char('a'));
const QString HOLDING(64, QLatin1Char('b'));
const QString PROGRAM(64, QLatin1Char('c'));
WalletSnapshot snapshot(const QString& balance)
{
    WalletSnapshot result;
    WalletAccountRead account;
    account.accountId = HOLDING;
    account.status = QStringLiteral("ok");
    account.programOwner = PROGRAM;
    account.dataHex = balance;
    result.publicAccountReads.append(account);
    WalletAccount holding;
    holding.address = HOLDING;
    holding.balance = QStringLiteral("0");
    holding.readStatus = QStringLiteral("ok");
    holding.programOwner = PROGRAM;
    holding.dataHex = balance;
    result.accounts.append(holding);
    return result;
}
}

WalletDecodeResult WalletIdlDecoder::decode(const QByteArray&,
                                            const QVector<WalletAccountRead>& reads)
{
    WalletDecodeResult result;
    result.status = QStringLiteral("ok");
    for (const WalletAccountRead& read : reads) {
        WalletDecodedAccount item;
        item.id = read.accountId;
        item.status = QStringLiteral("decoded");
        item.typeName = QStringLiteral("TokenHolding");
        item.value = QJsonObject {{ QStringLiteral("Fungible"), QJsonObject {
            { QStringLiteral("definition_id"), DEFINITION },
            { QStringLiteral("balance"), read.dataHex },
        } }};
        item.accountIds.insert(DEFINITION, DEFINITION);
        result.accounts.append(item);
    }
    return result;
}

struct ReadyNetworkProbe {
    struct Snapshot { QString status = QStringLiteral("ready"); } result;
    const Snapshot& snapshot() const { return result; }
};

// Only QtRO properties, the registry and the network probe are represented here.
// Both portfolio methods below are extracted verbatim from AmmUiBackend.cpp.
class AmmUiBackend : public QObject {
public:
    explicit AmmUiBackend(FakeWalletProvider& provider)
        : m_walletController(std::make_unique<WalletController>(
              provider, QStringLiteral("AmmPortfolioSyncTest"))),
          m_portfolio(std::make_unique<WalletPortfolioService>()),
          m_networkProbe(std::make_unique<ReadyNetworkProbe>())
    {
        connect(m_walletController.get(), &WalletController::stateChanged,
                this, [this]() { refreshPortfolio(); });
        connect(m_walletController.get(), &WalletController::snapshotChanged,
                this, [this]() { refreshPortfolio(); });
    }
    void refreshPortfolio();
    void applyPortfolio(WalletPortfolioResult result);
    bool walletStateReady() const
    {
        const auto& state = m_walletController->state();
        return state.syncStatus != QStringLiteral("opening")
            && state.syncStatus != QStringLiteral("syncing");
    }
    QStringList knownTokenIds() const { return { DEFINITION }; }
    bool resolveProgramIds() { ++resolveProgramCalls; return true; }
    QVariantList resolveTokens()
    {
        ++resolveTokenCalls;
        return { QVariantMap {
            { QStringLiteral("definitionId"), DEFINITION },
            { QStringLiteral("definitionIdHex"), DEFINITION },
            { QStringLiteral("name"), QStringLiteral("Test") },
        } };
    }
    void setAssets(QVariantList rows) { assets = std::move(rows); }
    void setAssetStatus(QString status)
    {
        publishedStatuses.append(status);
        assetStatus = std::move(status);
    }
    void setAssetError(QString error) { assetError = std::move(error); }
    std::unique_ptr<WalletController> m_walletController;
    std::unique_ptr<WalletPortfolioService> m_portfolio;
    std::unique_ptr<ReadyNetworkProbe> m_networkProbe;
    QString m_ammProgramIdCache;
    QString m_tokenProgramIdCache = PROGRAM;
    QByteArray m_tokenIdl = QByteArrayLiteral("fixture-token-idl");
    QByteArray m_ammIdl;
    QVariantList assets;
    QString assetStatus;
    QString assetError;
    QStringList publishedStatuses;
    int resolveProgramCalls = 0;
    int resolveTokenCalls = 0;
};
#include "portfolio-methods.inc"

class AmmPortfolioSyncTest : public QObject {
    Q_OBJECT
private slots:
    void failedSyncCannotPublishCachedBalances()
    {
        QTemporaryDir settings;
        QVERIFY(settings.isValid());
        QSettings::setPath(QSettings::NativeFormat, QSettings::UserScope, settings.path());
        qputenv("LEE_WALLET_HOME_DIR", settings.path().toLocal8Bit());
        FakeWalletProvider provider;
        provider.connectResult.snapshot = snapshot(QStringLiteral("25"));
        AmmUiBackend backend(provider);
        QVERIFY(backend.m_walletController->open());
        QCOMPARE(backend.assetStatus, QStringLiteral("ready"));
        QCOMPARE(backend.assets.size(), 1);
        QCOMPARE(backend.assets.first().toMap().value(QStringLiteral("balance")).toString(),
                 QStringLiteral("25"));

        provider.deferAsync = true;
        backend.m_walletController->refresh();
        QCOMPARE(backend.assetStatus, QStringLiteral("blocked"));
        QCOMPARE(backend.assetError, QStringLiteral("loading"));
        QVERIFY(backend.assets.isEmpty());
        const int resolved = backend.resolveTokenCalls;
        provider.snapshotResult.failure = WalletFailure::ReadFailed;
        provider.finishSnapshot();
        QCOMPARE(backend.m_walletController->state().syncStatus, QStringLiteral("error"));
        QVERIFY(backend.m_walletController->snapshot().ok());
        QCOMPARE(backend.assetStatus, QStringLiteral("error"));
        QCOMPARE(backend.assetError, QStringLiteral("read_failed"));
        QVERIFY(backend.assets.isEmpty());
        QCOMPARE(backend.resolveTokenCalls, resolved);

        // Registry changes and identity-probe signals reenter this same method.
        backend.refreshPortfolio();
        backend.refreshPortfolio();
        QCOMPARE(backend.assetStatus, QStringLiteral("error"));
        QCOMPARE(backend.assetError, QStringLiteral("read_failed"));
        QVERIFY(backend.assets.isEmpty());
        QCOMPARE(backend.resolveTokenCalls, resolved);

        provider.snapshotResult = snapshot(QStringLiteral("30"));
        backend.m_walletController->refresh();
        QCOMPARE(backend.assetStatus, QStringLiteral("blocked"));
        auto supersededRefresh = std::move(provider.pendingSnapshotCallback);
        QVERIFY(supersededRefresh);
        provider.createAccountResult.accountId = QString(64, QLatin1Char('d'));
        provider.createAccountResult.snapshot = snapshot(QStringLiteral("25"));
        WalletAccount created;
        created.address = provider.createAccountResult.accountId;
        created.balance = QStringLiteral("0");
        created.readStatus = QStringLiteral("ok");
        created.programOwner = QString(64, QLatin1Char('0'));
        provider.createAccountResult.snapshot.accounts.append(created);
        provider.snapshotResult.accounts.append(created);
        WalletAccountRead createdRead;
        createdRead.accountId = created.address;
        createdRead.status = created.readStatus;
        createdRead.programOwner = created.programOwner;
        provider.createAccountResult.snapshot.publicAccountReads.append(createdRead);
        provider.snapshotResult.publicAccountReads.append(createdRead);
        backend.publishedStatuses.clear();
        QCOMPARE(backend.m_walletController->createAccount(true), created.address);
        QCOMPARE(backend.m_walletController->state().syncStatus, QStringLiteral("syncing"));
        QCOMPARE(backend.assetStatus, QStringLiteral("blocked"));
        QVERIFY(!backend.publishedStatuses.contains(QStringLiteral("ready")));
        QVERIFY(provider.pendingSnapshotCallback);
        supersededRefresh(snapshot(QStringLiteral("99")));
        QCOMPARE(backend.assetStatus, QStringLiteral("blocked"));
        QVERIFY(backend.assets.isEmpty());
        provider.finishSnapshot();
        QCOMPARE(backend.assetStatus, QStringLiteral("ready"));
        QVERIFY(backend.assetError.isEmpty());
        QCOMPARE(backend.assets.size(), 1);
        QCOMPARE(backend.assets.first().toMap().value(QStringLiteral("balance")).toString(),
                 QStringLiteral("30"));

        backend.m_walletController->disconnect();
        QCOMPARE(backend.assetStatus, QStringLiteral("idle"));
        QVERIFY(backend.assetError.isEmpty());
        QVERIFY(backend.assets.isEmpty());
    }
};
QTEST_GUILESS_MAIN(AmmPortfolioSyncTest)
#include "AmmPortfolioSyncTest.moc"
