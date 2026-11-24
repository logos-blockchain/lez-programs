#include <QByteArray>
#include <QFile>
#include <QFileInfo>
#include <QDir>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QHostAddress>
#include <QNetworkAccessManager>
#include <QScopeGuard>
#include <QSettings>
#include <QSignalSpy>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTemporaryDir>
#include <QTimer>
#include <QUrl>
#include <QtTest>

#include <memory>
#include <utility>

#include "FakeWalletProvider.h"
#include "LogosWalletProvider.h"
#include "WalletAccountModel.h"
#include "WalletController.h"
#include "logos_sdk.h"

namespace {
const QString ACCOUNT_A(64, QLatin1Char('a'));
const QString ACCOUNT_B(64, QLatin1Char('b'));
const QString ACCOUNT_C(64, QLatin1Char('d'));
const QString PROGRAM_ID(64, QLatin1Char('c'));
const QString ACCOUNT_D(64, QLatin1Char('d'));
const QString EOA_OWNER(64, QLatin1Char('0'));

class ScopedEnvironment final {
public:
    ScopedEnvironment(QByteArray name, QByteArray value)
        : m_name(std::move(name)),
          m_hadValue(qEnvironmentVariableIsSet(m_name.constData())),
          m_previous(qgetenv(m_name.constData()))
    {
        qputenv(m_name.constData(), value);
    }

    ~ScopedEnvironment()
    {
        if (m_hadValue)
            qputenv(m_name.constData(), m_previous);
        else
            qunsetenv(m_name.constData());
    }

private:
    QByteArray m_name;
    bool m_hadValue;
    QByteArray m_previous;
};

QString publicAccountJson(const QString& owner = PROGRAM_ID,
                          const QString& balance = QStringLiteral("01000000000000000000000000000000"),
                          const QString& nonce = QString(32, QLatin1Char('0')),
                          const QString& data = QStringLiteral("00ff"))
{
    return QString::fromUtf8(QJsonDocument(QJsonObject {
        { QStringLiteral("program_owner"), owner },
        { QStringLiteral("balance"), balance },
        { QStringLiteral("nonce"), nonce },
        { QStringLiteral("data"), data },
    }).toJson(QJsonDocument::Compact));
}

QVariantMap accountEntry(const QString& id, bool isPublic)
{
    return {
        { QStringLiteral("account_id"), id },
        { QStringLiteral("is_public"), isPublic },
    };
}

}

class LogosWalletProviderTest : public QObject {
    Q_OBJECT

private slots:
    void adoptsOpenWalletAndCachesSnapshots();
    void retriesCapabilityWarmupSynchronously();
    void retriesCapabilityWarmupBeforeReadingWallet();
    void boundsPersistentCapabilityWarmupFailure();
    void reportsProgressDuringAsyncSync();
    void releasesAsyncSyncProgress_data();
    void releasesAsyncSyncProgress();
    void opensConfiguredWalletWhenNoSharedSessionExists();
    void opensStoredWalletAfterCheckingSharedSession();
    void opensAndReadsAsynchronously();
    void avoidsSavingAfterUnchangedAsynchronousSnapshots();
    void deferredProviderCallbacksIgnoreDestruction_data();
    void deferredProviderCallbacksIgnoreDestruction();
    void creationSupersedesPendingProviderSnapshot();
    void controllerCreationSupersedesPendingSnapshot();
    void createdPublicAccountRetainsClassification();
    void createsAndPersistsWallet();
    void cancelsWalletCreationOnDestruction_data();
    void cancelsWalletCreationOnDestruction();
    void cancelsControllerCreationOnDestruction();
    void cancelsCapabilityRetryOnDestruction();
    void cancelsPendingReplyOnDestruction_data();
    void cancelsPendingReplyOnDestruction();
    void cancelsQueuedCompletionOnDestruction_data();
    void cancelsQueuedCompletionOnDestruction();
    void releasesCompletedSnapshotCallback_data();
    void releasesCompletedSnapshotCallback();
    void validatesCompletePublicAccountPayloads();
    void fallsBackToBalanceWhenPublicReadFails();
    void createsAndPersistsAccounts();
    void preservesCreatedAccountWhenPublicReadFails();
    void createdAccountDoesNotRescanWallet();
    void dispatchesExactGenericTransaction();
    void rejectsInvalidSubmissionResponses();
    void requiresSigningOrSeparateFeePayer();
    void exposesStableAccountModelRoles();
    void clearsStaleAccountPresentationsWithoutInvalidatingPrimary();
    void persistsHumanizedWalletPreferences();
    void fakeProviderImplementsConsumerContract();
    void controllerOwnsUiWalletFlow();
    void controllerPreservesStatisticsPathForWalletLifecycle();
    void controllerRejectsDuplicateOpenWhileStarting();
    void controllerCanRetryAfterOpenFailure();
    void controllerPollsSnapshotsAndRetriesAfterFailure();
    void controllerPollSkipsSyncWithoutNewBlocks();
    void controllerManualRetryReportsOutcome();
    void providerReadsBlockHeightWithoutSyncing();
    void controllerExposesProgressAndCancelsInitialSync();
    void controllerCreatesWalletBeforeAsyncSyncCompletes();
    void controllerSelectsFundedFeePayer();
    void controllerSeparatesSnapshotsFromCosmeticState();
    void controllerOpenDoesNotWaitForWalletSync();
    void controllerCreationDoesNotWaitForWalletSync();
    void controllerSeedsDefaultWalletConfigWithConfiguredEndpoint();
    void controllerPreservesExistingDefaultWalletConfig();
    void controllerStopsReachabilityChecksAfterDisconnect();
    void completedAsyncSnapshotReleasesCallback();
    void deferredCallbacksIgnoreDestroyedController();
    void newerReachabilityResultWins();
    void coalescesReachabilityChecksForSameEndpoint();
    void controllerReportsPartialWalletCreation();
};

void LogosWalletProviderTest::adoptsOpenWalletAndCachesSnapshots()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.currentBlockHeight = 12;
    modules.lez_core.lastSyncedBlock = 11;
    modules.lez_core.accounts = {
        accountEntry(ACCOUNT_A, true),
        accountEntry(ACCOUNT_B, false),
    };
    modules.lez_core.publicAccounts.insert(
        ACCOUNT_A, publicAccountJson());
    modules.lez_core.balances.insert(ACCOUNT_B, QStringLiteral("42"));

    LogosWalletProvider provider(&modules);
    const WalletSession session = provider.connect({ QStringLiteral("unused"), QStringLiteral("unused") });

    QVERIFY(session.ok());
    QVERIFY(session.adopted);
    QCOMPARE(session.snapshot.accounts.size(), 2);
    QCOMPARE(session.snapshot.accounts.at(0).balance, QStringLiteral("1"));
    QCOMPARE(session.snapshot.accounts.at(1).balance, QStringLiteral("42"));
    QCOMPARE(session.snapshot.publicAccountReads.size(), 1);
    QCOMPARE(session.snapshot.accounts.at(0).readStatus, QStringLiteral("ok"));
    QCOMPARE(session.snapshot.accounts.at(0).programOwner, PROGRAM_ID);
    QCOMPARE(session.snapshot.accounts.at(0).dataHex, QStringLiteral("00ff"));
    QCOMPARE(session.snapshot.accounts.at(0).displayAddress,
             QStringLiteral("base58-") + ACCOUNT_A);
    QCOMPARE(session.snapshot.accounts.at(1).readStatus, QStringLiteral("private"));
    QCOMPARE(session.snapshot.currentBlockHeight, quint64(12));
    QCOMPARE(session.snapshot.lastSyncedBlock, quint64(11));

    modules.lez_core.lastSyncedBlock = modules.lez_core.currentBlockHeight;
    const int listCalls = modules.lez_core.listCalls;
    const int readCalls = modules.lez_core.publicReadCalls;
    const int saveCalls = modules.lez_core.saveCalls;
    QVERIFY(provider.snapshot().ok());
    QCOMPARE(modules.lez_core.listCalls, listCalls);
    QCOMPARE(modules.lez_core.publicReadCalls, readCalls);

    QVERIFY(provider.snapshot(true).ok());
    QVERIFY(modules.lez_core.listCalls > listCalls);
    QVERIFY(modules.lez_core.publicReadCalls > readCalls);
    QCOMPARE(modules.lez_core.saveCalls, saveCalls);

    modules.lez_core.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QString(32, QLatin1Char('f')));
    QCOMPARE(provider.snapshot(true).accounts.at(0).balance,
             QStringLiteral("340282366920938463463374607431768211455"));

    provider.clearSnapshot();
    const int afterRefresh = modules.lez_core.listCalls;
    QVERIFY(provider.snapshot().ok());
    QVERIFY(modules.lez_core.listCalls > afterRefresh);

    provider.disconnect();
    QCOMPARE(provider.snapshot().failure, WalletFailure::WalletUnavailable);
}

void LogosWalletProviderTest::retriesCapabilityWarmupBeforeReadingWallet()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.versionFailuresRemaining = 2;

    LogosWalletProvider provider(&modules);
    bool completed = false;
    WalletSession result;
    provider.connectAsync({}, [&completed, &result](WalletSession session) {
        result = std::move(session);
        completed = true;
    });

    QTRY_VERIFY_WITH_TIMEOUT(modules.lez_core.versionCalls >= 2, 1000);
    QCOMPARE(modules.lez_core.listCalls, 0);
    QTRY_VERIFY_WITH_TIMEOUT(completed, 3000);
    QVERIFY(result.ok());
    QCOMPARE(modules.lez_core.versionCalls, 3);
    QCOMPARE(modules.lez_core.listCalls, 1);
}

void LogosWalletProviderTest::retriesCapabilityWarmupSynchronously()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.versionFailuresRemaining = 2;

    LogosWalletProvider provider(&modules);
    const WalletSession result = provider.connect({});

    QVERIFY(result.ok());
    QCOMPARE(modules.lez_core.versionCalls, 3);
    QCOMPARE(modules.lez_core.listCalls, 1);
}

void LogosWalletProviderTest::boundsPersistentCapabilityWarmupFailure()
{
    LogosModules modules;
    modules.lez_core.versionFailuresRemaining = -1;

    LogosWalletProvider provider(&modules);
    bool completed = false;
    WalletSession result;
    provider.connectAsync({}, [&completed, &result](WalletSession session) {
        result = std::move(session);
        completed = true;
    });

    QTRY_VERIFY_WITH_TIMEOUT(completed, 7000);
    QCOMPARE(result.failure, WalletFailure::CapabilityUnavailable);
    QVERIFY(modules.lez_core.versionCalls <= 100);
    QCOMPARE(modules.lez_core.listCalls, 0);
    QCOMPARE(modules.lez_core.openCalls, 0);
}

void LogosWalletProviderTest::reportsProgressDuringAsyncSync()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.currentBlockHeight = 1500;

    LogosWalletProvider provider(&modules);
    QVector<WalletSyncProgress> progress;
    bool completed = false;
    provider.connectAsync({},
        [&completed](WalletSession session) {
            completed = session.ok();
        },
        [&progress](WalletSyncProgress update) {
            progress.append(update);
        });

    QTRY_VERIFY_WITH_TIMEOUT(completed, 1000);
    QVERIFY(progress.size() >= 4);
    QVERIFY(progress.first().known);
    QCOMPARE(progress.first().currentBlock, quint64(0));
    QCOMPARE(progress.first().targetBlock, quint64(1500));
    QCOMPARE(progress.first().remainingBlocks, quint64(1500));
    QVERIFY(progress.last().known);
    QCOMPARE(progress.last().currentBlock, quint64(1500));
    QCOMPARE(progress.last().targetBlock, quint64(1500));
    QCOMPARE(progress.last().remainingBlocks, quint64(0));
    for (qsizetype index = 1; index < progress.size(); ++index)
        QVERIFY(progress.at(index - 1).currentBlock < progress.at(index).currentBlock);
}

void LogosWalletProviderTest::releasesAsyncSyncProgress_data()
{
    QTest::addColumn<bool>("deferred");
    QTest::addColumn<bool>("fails");
    QTest::addColumn<bool>("cancelled");
    QTest::newRow("immediate-success") << false << false << false;
    QTest::newRow("deferred-success") << true << false << false;
    QTest::newRow("deferred-error") << true << true << false;
    QTest::newRow("deferred-disconnect") << true << false << true;
}

void LogosWalletProviderTest::releasesAsyncSyncProgress()
{
    QFETCH(bool, deferred);
    QFETCH(bool, fails);
    QFETCH(bool, cancelled);

    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.currentBlockHeight = 1024;
    modules.lez_core.deferSync = deferred;
    modules.lez_core.syncResult = fails ? -1 : 0;

    std::weak_ptr<int> lifetime;
    int completed = 0;
    QVector<WalletSyncProgress> progress;
    WalletSession result;
    {
        LogosWalletProvider provider(&modules);
        auto token = std::make_shared<int>(42);
        lifetime = token;
        provider.connectAsync({},
            [&completed, &result](WalletSession session) {
                ++completed;
                result = std::move(session);
            },
            [token, &progress](WalletSyncProgress update) {
                progress.append(update);
            });
        token.reset();

        if (deferred) {
            QCOMPARE(completed, 0);
            QCOMPARE(modules.lez_core.syncCalls, 1);
            QVERIFY(!lifetime.expired());
            if (cancelled)
                provider.disconnect();
            modules.lez_core.finishSync();
            if (!fails && !cancelled) {
                QCOMPARE(completed, 0);
                QCOMPARE(modules.lez_core.syncCalls, 2);
                QVERIFY(!lifetime.expired());
                modules.lez_core.finishSync();
            }
        }

        QCOMPARE(completed, cancelled ? 0 : 1);
        QCOMPARE(modules.lez_core.syncCalls, fails || cancelled ? 1 : 2);
        QCOMPARE(progress.size(), fails || cancelled ? 1 : 3);
        if (!cancelled)
            QCOMPARE(result.failure, fails ? WalletFailure::ReadFailed : WalletFailure::None);
        // Only progress owns this token, isolating the recursive sync callback.
        QVERIFY(lifetime.expired());
        QVERIFY(!modules.lez_core.pendingSync);
    }
    QVERIFY(lifetime.expired());
}

void LogosWalletProviderTest::opensConfiguredWalletWhenNoSharedSessionExists()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString storage = directory.filePath(QStringLiteral("storage.json"));
    QFile file(storage);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.close();

    LogosModules modules;
    LogosWalletProvider provider(&modules);
    const WalletSession session = provider.connect({
        directory.filePath(QStringLiteral("wallet.json")),
        storage,
        directory.filePath(QStringLiteral("statistics.json")),
    });

    QVERIFY(session.ok());
    QVERIFY(!session.adopted);
    QCOMPARE(modules.lez_core.openCalls, 1);
    QCOMPARE(modules.lez_core.openedStorage, storage);
    QCOMPARE(modules.lez_core.openedStatistics,
             directory.filePath(QStringLiteral("statistics.json")));

    bool completed = false;
    provider.connectAsync({ QStringLiteral("config"), storage, QStringLiteral("statistics") },
                          [&completed](WalletSession result) { completed = result.ok(); });
    QVERIFY(completed);
    QCOMPARE(modules.lez_core.openedStatistics, QStringLiteral("statistics"));

    LogosModules missingModules;
    LogosWalletProvider missingProvider(&missingModules);
    QCOMPARE(missingProvider.connect({ QStringLiteral("config"), QStringLiteral("missing") }).failure,
             WalletFailure::WalletMissing);
}

void LogosWalletProviderTest::opensStoredWalletAfterCheckingSharedSession()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString storage = directory.filePath(QStringLiteral("storage.json"));
    QFile file(storage);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.close();

    LogosModules modules;
    LogosWalletProvider provider(&modules);
    bool connected = false;
    provider.connectAsync({ directory.filePath(QStringLiteral("wallet.json")), storage },
                          [&connected](WalletSession session) {
                              connected = session.ok() && !session.adopted;
                          });

    QVERIFY(connected);
    QCOMPARE(modules.lez_core.openCalls, 1);
    QCOMPARE(modules.lez_core.listCalls, 2);
}

void LogosWalletProviderTest::opensAndReadsAsynchronously()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.publicAccounts.insert(
        ACCOUNT_A, publicAccountJson(EOA_OWNER));
    LogosWalletProvider provider(&modules);

    bool connected = false;
    provider.connectAsync({}, [&connected](WalletSession session) {
        connected = session.ok() && session.snapshot.accounts.size() == 1;
    });
    QVERIFY(connected);

    bool refreshed = false;
    provider.snapshotAsync(true, [&refreshed](WalletSnapshot snapshot) {
        refreshed = snapshot.ok() && snapshot.accounts.at(0).programOwner == EOA_OWNER;
    });
    QVERIFY(refreshed);

}

void LogosWalletProviderTest::avoidsSavingAfterUnchangedAsynchronousSnapshots()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.currentBlockHeight = 12;
    modules.lez_core.lastSyncedBlock = 12;
    LogosWalletProvider provider(&modules);

    bool connected = false;
    provider.connectAsync({}, [&connected](WalletSession session) {
        connected = session.ok();
    });
    QVERIFY(connected);
    QCOMPARE(modules.lez_core.saveCalls, 0);

    bool refreshed = false;
    provider.snapshotAsync(true, [&refreshed](WalletSnapshot snapshot) {
        refreshed = snapshot.ok();
    });
    QVERIFY(refreshed);
    QCOMPARE(modules.lez_core.saveCalls, 0);
}

void LogosWalletProviderTest::deferredProviderCallbacksIgnoreDestruction_data()
{
    QTest::addColumn<QString>("operation");
    QTest::addColumn<bool>("connectFirst");
    QTest::addColumn<bool>("publicReadFails");
    QTest::newRow("connect-address") << QStringLiteral("get_sequencer_addr") << false << false;
    QTest::newRow("open") << QStringLiteral("open") << false << false;
    QTest::newRow("height") << QStringLiteral("get_current_block_height") << true << false;
    QTest::newRow("sync") << QStringLiteral("sync_to_block") << true << false;
    QTest::newRow("last-synced") << QStringLiteral("get_last_synced_block") << true << false;
    QTest::newRow("snapshot-address") << QStringLiteral("get_sequencer_addr") << true << false;
    QTest::newRow("list") << QStringLiteral("list_accounts") << true << false;
    QTest::newRow("public-read") << QStringLiteral("get_account_public") << true << false;
    QTest::newRow("failed-public-read") << QStringLiteral("get_account_public") << true << true;
    QTest::newRow("fallback-balance") << QStringLiteral("get_balance") << true << true;
}

void LogosWalletProviderTest::deferredProviderCallbacksIgnoreDestruction()
{
    QFETCH(QString, operation);
    QFETCH(bool, connectFirst);
    QFETCH(bool, publicReadFails);
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString storage = directory.filePath(QStringLiteral("wallet.json"));
    QFile storageFile(storage);
    QVERIFY(storageFile.open(QIODevice::WriteOnly));
    storageFile.close();

    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.currentBlockHeight = 12;
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.publicAccounts.insert(ACCOUNT_A,
        publicReadFails ? QString() : publicAccountJson(EOA_OWNER));
    auto provider = std::make_unique<LogosWalletProvider>(&modules);
    if (connectFirst)
        QVERIFY(provider->connect({}).ok());
    else if (operation == QStringLiteral("open")) {
        modules.lez_core.sequencerAddress.clear();
        modules.lez_core.accounts.clear();
    }
    modules.lez_core.deferredMethod = operation;
    bool completed = false;
    if (connectFirst)
        provider->snapshotAsync(true, [&completed](WalletSnapshot) { completed = true; });
    else
        provider->connectAsync({ {}, storage }, [&completed](WalletSession) { completed = true; });
    QCOMPARE(modules.lez_core.pendingReplies.size(), size_t(1));
    QVERIFY(!completed);
    const int readsBeforeDestroy = modules.lez_core.publicReadCalls;
    provider.reset();
    modules.lez_core.completePendingReplies();
    QVERIFY(!completed);
    QCOMPARE(modules.lez_core.publicReadCalls, readsBeforeDestroy);
    QVERIFY(modules.lez_core.pendingReplies.empty());
}

void LogosWalletProviderTest::creationSupersedesPendingProviderSnapshot()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson(EOA_OWNER));
    modules.lez_core.publicAccounts.insert(ACCOUNT_B, publicAccountJson(EOA_OWNER));
    modules.lez_core.publicAccountId = ACCOUNT_B;
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());
    modules.lez_core.deferredMethod = QStringLiteral("get_account_public");
    bool completed = false;
    provider.snapshotAsync(true, [&completed](WalletSnapshot) { completed = true; });
    QCOMPARE(modules.lez_core.pendingReplies.size(), size_t(1));

    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 2);
    modules.lez_core.completePendingReplies();
    QVERIFY(!completed);
    QCOMPARE(provider.snapshot().accounts.size(), 2);
    QCOMPARE(provider.snapshot().accounts.last().address, ACCOUNT_B);
}

void LogosWalletProviderTest::controllerCreationSupersedesPendingSnapshot()
{
    const QString settingsApplication = QStringLiteral("WalletCreationRefreshRaceTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();
    FakeWalletProvider provider;
    provider.connectResult.snapshot.accounts = { { ACCOUNT_A, QStringLiteral("5"), true } };
    provider.snapshotResult = provider.connectResult.snapshot;
    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    provider.deferAsync = true;
    controller.refresh();
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QVERIFY(provider.pendingSnapshotCallback);
    const auto olderSnapshot = std::move(provider.pendingSnapshotCallback);
    provider.createAccountResult.accountId = ACCOUNT_B;
    provider.createAccountResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
        { ACCOUNT_B, QStringLiteral("0"), true },
    };
    QCOMPARE(controller.createAccount(true), ACCOUNT_B);
    QCOMPARE(provider.snapshotCalls, 2);
    QVERIFY(provider.pendingSnapshotCallback);
    QCOMPARE(controller.accountModel()->count(), 2);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    olderSnapshot(provider.snapshotResult);
    QCOMPARE(controller.accountModel()->count(), 2);
    QCOMPARE(controller.snapshot().accounts.size(), 2);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    provider.snapshotResult = provider.createAccountResult.snapshot;
    provider.finishSnapshot();
    QCOMPARE(controller.accountModel()->count(), 2);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    settings.clear();
}

void LogosWalletProviderTest::createdPublicAccountRetainsClassification()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.publicAccountId = ACCOUNT_A;
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson(EOA_OWNER));
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());
    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    const WalletAccount& account = creation.snapshot.accounts.first();
    QCOMPARE(account.readStatus, QStringLiteral("ok"));
    QCOMPARE(account.programOwner, EOA_OWNER);
    QCOMPARE(account.dataHex, QStringLiteral("00ff"));
    WalletAccountModel model;
    model.replaceAccounts(creation.snapshot.accounts);
    QVERIFY(model.canBePrimary(ACCOUNT_A));
    QCOMPARE(model.firstAutomaticPrimary(), ACCOUNT_A);
}

void LogosWalletProviderTest::createsAndPersistsWallet()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    LogosModules modules;
    modules.lez_core.currentBlockHeight = 12;
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    const WalletPaths paths {
        directory.filePath(QStringLiteral("config/wallet.json")),
        directory.filePath(QStringLiteral("state/storage.json")),
        directory.filePath(QStringLiteral("state/statistics.json")),
    };
    const WalletCreation creation = provider.createWallet(paths, QStringLiteral("secret"));

    QVERIFY(creation.ok());
    QCOMPARE(creation.mnemonic, modules.lez_core.mnemonic);
    QCOMPARE(modules.lez_core.createAsyncCalls, 1);
    QCOMPARE(modules.lez_core.createdConfig, paths.config);
    QCOMPARE(modules.lez_core.createdStorage, paths.storage);
    QCOMPARE(modules.lez_core.createdStatistics, paths.statistics);
    QCOMPARE(modules.lez_core.createdPassword, QStringLiteral("secret"));
    QVERIFY(modules.lez_core.saveCalls >= 1);
    QCOMPARE(modules.lez_core.syncCalls, 0);
    QCOMPARE(modules.lez_core.listCalls, 0);
    QCOMPARE(modules.lez_core.publicReadCalls, 0);

    LogosModules rejectedModules;
    rejectedModules.lez_core.mnemonic.clear();
    LogosWalletProvider rejectedProvider(&rejectedModules);
    QCOMPARE(rejectedProvider.createWallet(paths, QStringLiteral("secret")).failure,
             WalletFailure::CreateFailed);

    LogosModules unsavedModules;
    unsavedModules.lez_core.saveResult = 1;
    LogosWalletProvider unsavedProvider(&unsavedModules);
    const WalletCreation unsaved = unsavedProvider.createWallet(paths, QStringLiteral("secret"));
    QCOMPARE(unsaved.failure, WalletFailure::SaveFailed);
    QCOMPARE(unsaved.mnemonic, unsavedModules.lez_core.mnemonic);
}

void LogosWalletProviderTest::cancelsWalletCreationOnDestruction_data()
{
    QTest::addColumn<bool>("lateReply");
    QTest::newRow("queued-reply") << false;
    QTest::newRow("reply-after-return") << true;
}

void LogosWalletProviderTest::cancelsWalletCreationOnDestruction()
{
    QFETCH(bool, lateReply);
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    LogosModules modules;
    if (lateReply)
        modules.lez_core.deferredMethod = QStringLiteral("create_new");
    auto provider = std::make_unique<LogosWalletProvider>(&modules);
    QTimer::singleShot(0, [&provider] { provider.reset(); });
    const WalletCreation creation = provider->createWallet({
        directory.filePath(QStringLiteral("config.json")),
        directory.filePath(QStringLiteral("storage.json")),
        directory.filePath(QStringLiteral("statistics.json")),
    }, QStringLiteral("secret"));
    QVERIFY(!provider);
    QCOMPARE(creation.failure, WalletFailure::WalletUnavailable);
    QVERIFY(creation.mnemonic.isEmpty());
    QCOMPARE(modules.lez_core.saveCalls, 0);
    // The SDK can deliver its reply after the canceled call has returned.
    QTest::qWait(1);
    if (lateReply) {
        QCOMPARE(modules.lez_core.pendingReplies.size(), size_t(1));
        modules.lez_core.completePendingReplies();
    }
}

void LogosWalletProviderTest::cancelsControllerCreationOnDestruction()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    LogosModules modules;
    LogosWalletProvider provider(&modules);
    auto controller = std::make_unique<WalletController>(
        provider, QStringLiteral("WalletCreationTeardownTest"));
    QTimer::singleShot(0, [&controller] { controller.reset(); });
    const QString mnemonic = controller->createWallet(
        directory.filePath(QStringLiteral("config.json")),
        directory.filePath(QStringLiteral("storage.json")),
        QStringLiteral("secret"));
    QVERIFY(!controller);
    QVERIFY(mnemonic.isEmpty());
    QCOMPARE(modules.lez_core.createAsyncCalls, 1);
    QCOMPARE(modules.lez_core.listCalls, 0);
}

void LogosWalletProviderTest::cancelsCapabilityRetryOnDestruction()
{
    LogosModules modules;
    modules.lez_core.versionFailuresRemaining = 1;
    bool completed = false;
    auto provider = std::make_unique<LogosWalletProvider>(&modules);
    provider->connectAsync({}, [&completed](WalletSession) { completed = true; });
    QCOMPARE(modules.lez_core.versionCalls, 1);
    provider.reset();
    QTest::qWait(100);
    QCOMPARE(modules.lez_core.versionCalls, 1);
    QVERIFY(!completed);
}

void LogosWalletProviderTest::cancelsPendingReplyOnDestruction_data()
{
    QTest::addColumn<QString>("method");
    QTest::addColumn<int>("occurrence");
    QTest::addColumn<bool>("isPublic");
    QTest::addColumn<bool>("validPublicRead");
    QTest::addColumn<bool>("sharedSession");
    for (const QString& method : { QStringLiteral("version"),
                                  QStringLiteral("get_sequencer_addr"),
                                  QStringLiteral("list_accounts"),
                                  QStringLiteral("open"),
                                  QStringLiteral("get_current_block_height"),
                                  QStringLiteral("sync_to_block"),
                                  QStringLiteral("get_last_synced_block"),
                                  QStringLiteral("save") }) {
        QTest::newRow(qPrintable(method)) << method << 1 << true << true << false;
    }
    QTest::newRow("snapshot-address") << QStringLiteral("get_sequencer_addr")
                                      << 2 << true << true << false;
    QTest::newRow("second-sync-chunk") << QStringLiteral("sync_to_block")
                                      << 2 << true << true << false;
    QTest::newRow("snapshot-last-synced") << QStringLiteral("get_last_synced_block")
                                         << 2 << true << true << false;
    QTest::newRow("snapshot-accounts") << QStringLiteral("list_accounts")
                                       << 2 << true << true << false;
    QTest::newRow("public-account") << QStringLiteral("get_account_public")
                                   << 1 << true << true << true;
    QTest::newRow("public-account-fallback") << QStringLiteral("get_account_public")
                                            << 1 << true << false << true;
    QTest::newRow("public-balance-fallback") << QStringLiteral("get_balance")
                                            << 1 << true << false << true;
    QTest::newRow("private-balance") << QStringLiteral("get_balance")
                                    << 1 << false << false << true;
}

void LogosWalletProviderTest::cancelsPendingReplyOnDestruction()
{
    QFETCH(QString, method);
    QFETCH(int, occurrence);
    QFETCH(bool, isPublic);
    QFETCH(bool, validPublicRead);
    QFETCH(bool, sharedSession);
    QTemporaryDir directory;
    QFile storage(directory.filePath(QStringLiteral("storage.json")));
    QVERIFY(storage.open(QIODevice::WriteOnly));
    storage.close();
    LogosModules modules;
    modules.lez_core.currentBlockHeight = 1024;
    modules.lez_core.deferredMethod = method;
    modules.lez_core.deferredOccurrence = occurrence;
    if (sharedSession) {
        modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
        modules.lez_core.accounts = { accountEntry(ACCOUNT_A, isPublic) };
        if (validPublicRead)
            modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
        modules.lez_core.balances.insert(ACCOUNT_A, QStringLiteral("1"));
    }
    bool completed = false;
    auto retained = std::make_shared<int>(0);
    const std::weak_ptr<int> weak = retained;
    auto retainedProgress = std::make_shared<int>(0);
    const std::weak_ptr<int> weakProgress = retainedProgress;
    auto provider = std::make_unique<LogosWalletProvider>(&modules);
    provider->connectAsync({ {}, storage.fileName(), {} },
                           [&completed, retained = std::move(retained)](WalletSession) {
        completed = true;
    }, [retainedProgress = std::move(retainedProgress)](WalletSyncProgress) {});
    QCOMPARE(modules.lez_core.pendingReplies.size(), size_t(1));
    QVERIFY(!completed);
    provider.reset();
    modules.lez_core.completePendingReplies();
    QVERIFY(!completed);
    QVERIFY(weak.expired());
    QVERIFY(weakProgress.expired());
    QVERIFY(modules.lez_core.pendingReplies.empty());
}

void LogosWalletProviderTest::cancelsQueuedCompletionOnDestruction_data()
{
    QTest::addColumn<int>("mode");
    QTest::newRow("missing-module") << 0;
    QTest::newRow("disconnected-snapshot") << 1;
    QTest::newRow("cached-snapshot") << 2;
}

void LogosWalletProviderTest::cancelsQueuedCompletionOnDestruction()
{
    QFETCH(int, mode);
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    auto provider = std::make_unique<LogosWalletProvider>(mode == 0 ? nullptr : &modules);
    bool completed = false;
    if (mode == 0) {
        provider->connectAsync({}, [&completed](WalletSession) { completed = true; });
    } else {
        if (mode == 2)
            QVERIFY(provider->connect({}).ok());
        provider->snapshotAsync(false, [&completed](WalletSnapshot) { completed = true; });
    }
    provider.reset();
    QTest::qWait(1);
    QVERIFY(!completed);
}

void LogosWalletProviderTest::releasesCompletedSnapshotCallback_data()
{
    QTest::addColumn<bool>("withAccounts");
    QTest::addColumn<int>("saveResult");
    QTest::newRow("empty") << false << 0;
    QTest::newRow("public-and-private") << true << 0;
    QTest::newRow("save-failure") << true << 1;
}

void LogosWalletProviderTest::releasesCompletedSnapshotCallback()
{
    QFETCH(bool, withAccounts);
    QFETCH(int, saveResult);
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    if (withAccounts) {
        modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true), accountEntry(ACCOUNT_B, false) };
        modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
        modules.lez_core.balances.insert(ACCOUNT_B, QStringLiteral("2"));
    }
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());
    modules.lez_core.saveResult = saveResult;
    for (int attempt = 0; attempt < 3; ++attempt) {
        modules.lez_core.currentBlockHeight = attempt + 1;
        auto retained = std::make_shared<int>(0);
        const std::weak_ptr<int> weak = retained;
        bool completed = false;
        provider.snapshotAsync(true, [&completed, saveResult,
                                      retained = std::move(retained)](WalletSnapshot snapshot) {
            QCOMPARE(snapshot.failure, saveResult == 0 ? WalletFailure::None : WalletFailure::SaveFailed);
            completed = true;
        });
        QVERIFY(completed);
        QVERIFY(weak.expired());
    }
}

void LogosWalletProviderTest::validatesCompletePublicAccountPayloads()
{
    LogosModules modules;
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);

    const WalletAccountRead valid = provider.readPublicAccount(ACCOUNT_A);
    QVERIFY(valid.ok());
    QCOMPARE(valid.accountId, ACCOUNT_A);
    QCOMPARE(valid.programOwner, PROGRAM_ID);
    QCOMPARE(valid.balanceHex, QStringLiteral("01000000000000000000000000000000"));
    QCOMPARE(valid.dataHex, QStringLiteral("00ff"));

    modules.lez_core.publicAccounts[ACCOUNT_A] = publicAccountJson(PROGRAM_ID.toUpper());
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.lez_core.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QStringLiteral("01"));
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.lez_core.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QStringLiteral("01000000000000000000000000000000"),
        QString(32, QLatin1Char('0')), QStringLiteral("abc"));
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.lez_core.publicAccounts[ACCOUNT_A] = QStringLiteral("[]");
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    QVERIFY(!provider.readPublicAccount(QStringLiteral("invalid")).ok());
}

void LogosWalletProviderTest::fallsBackToBalanceWhenPublicReadFails()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.balances.insert(ACCOUNT_A, QStringLiteral("42"));

    LogosWalletProvider provider(&modules);
    const WalletSession session = provider.connect({});

    QVERIFY(session.ok());
    QCOMPARE(session.snapshot.accounts.size(), 1);
    QCOMPARE(session.snapshot.accounts.at(0).balance, QStringLiteral("42"));
    QCOMPARE(session.snapshot.publicAccountReads.size(), 1);
    QVERIFY(!session.snapshot.publicAccountReads.at(0).ok());
}

void LogosWalletProviderTest::createsAndPersistsAccounts()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.publicAccountId = ACCOUNT_A;
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    const int savesBeforeCreate = modules.lez_core.saveCalls;
    const int publicReadsBeforeCreate = modules.lez_core.publicReadCalls;
    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.publicAccount.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QVERIFY(modules.lez_core.saveCalls > savesBeforeCreate);
    QCOMPARE(modules.lez_core.publicReadCalls, publicReadsBeforeCreate + 1);

    modules.lez_core.saveResult = 1;
    QCOMPARE(provider.createAccount(true).failure, WalletFailure::SaveFailed);
}

void LogosWalletProviderTest::preservesCreatedAccountWhenPublicReadFails()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.publicAccountId = ACCOUNT_A;
    modules.lez_core.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.lez_core.balances.insert(ACCOUNT_A, QStringLiteral("7"));
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    const WalletAccountCreation creation = provider.createAccount(true);

    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(!creation.publicAccount.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QCOMPARE(creation.snapshot.accounts.at(0).balance, QStringLiteral("7"));
}

void LogosWalletProviderTest::createdAccountDoesNotRescanWallet()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.publicAccountId = ACCOUNT_A;
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    modules.lez_core.currentBlockHeight = 1;
    modules.lez_core.syncResult = 1;
    const int syncCalls = modules.lez_core.syncCalls;
    const WalletAccountCreation creation = provider.createAccount(true);

    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.snapshot.ok());
    QCOMPARE(modules.lez_core.syncCalls, syncCalls);
}

void LogosWalletProviderTest::dispatchesExactGenericTransaction()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"})");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    WalletTransaction transaction;
    transaction.programId = PROGRAM_ID;
    transaction.accountIds = { ACCOUNT_A, ACCOUNT_B };
    transaction.signingRequirements = { true, false };
    transaction.instruction = { 7, 0, 4294967295U };
    transaction.feePayerId = ACCOUNT_D;

    const WalletSubmission submission = provider.submitPublicTransaction(transaction);
    QVERIFY(submission.accepted());
    QCOMPARE(submission.nativeHash, QString(64, QLatin1Char('a')));
    QCOMPARE(modules.lez_core.submitCalls, 1);
    QCOMPARE(modules.lez_core.submittedProgramId, PROGRAM_ID);
    QCOMPARE(modules.lez_core.submittedAccountIds, transaction.accountIds);
    QCOMPARE(modules.lez_core.submittedSigningRequirements,
             QList<bool>({ true, false }));
    QCOMPARE(modules.lez_core.submittedInstruction,
             QByteArray::fromHex("0700000000000000ffffffff"));
}

void LogosWalletProviderTest::rejectsInvalidSubmissionResponses()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    WalletTransaction transaction {
        PROGRAM_ID,
        { ACCOUNT_A },
        { true },
        { 1 },
        ACCOUNT_A,
    };

    const QStringList invalidResponses {
        QStringLiteral("not-json"),
        QStringLiteral(R"({"success":false,"tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})"),
        QStringLiteral(R"({"success":true,"error":"rejected","tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})"),
        QStringLiteral(R"({"success":true,"tx_hash":"short"})"),
    };
    for (const QString& response : invalidResponses) {
        modules.lez_core.transactionResponse = response;
        QCOMPARE(provider.submitPublicTransaction(transaction).failure,
                 WalletFailure::SubmissionFailed);
    }

    transaction.signingRequirements.clear();
    QCOMPARE(provider.submitPublicTransaction(transaction).failure,
             WalletFailure::InvalidRequest);
}

void LogosWalletProviderTest::requiresSigningOrSeparateFeePayer()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    // Fresh, unfunded signers (a token create flow) paid for by a separate account.
    WalletTransaction transaction { PROGRAM_ID, { ACCOUNT_A, ACCOUNT_B }, { true, false }, { 1 } };
    QCOMPARE(provider.submitPublicTransaction(transaction).failure,
             WalletFailure::InvalidRequest);
    transaction.feePayerId = ACCOUNT_B;
    QCOMPARE(provider.submitPublicTransaction(transaction).failure,
             WalletFailure::InvalidRequest);
    QCOMPARE(modules.lez_core.submitCalls, 0);

    transaction.feePayerId = ACCOUNT_A;
    QVERIFY(provider.submitPublicTransaction(transaction).accepted());
    QCOMPARE(modules.lez_core.submittedPayerAccountId, ACCOUNT_A);
    transaction.feePayerId = ACCOUNT_D;
    QVERIFY(provider.submitPublicTransaction(transaction).accepted());
    QCOMPARE(modules.lez_core.submittedPayerAccountId, ACCOUNT_D);
}

void LogosWalletProviderTest::exposesStableAccountModelRoles()
{
    WalletAccountModel model;
    QSignalSpy countChanged(&model, &WalletAccountModel::countChanged);
    model.replaceAccounts({
        { ACCOUNT_A, QStringLiteral("10"), true, QStringLiteral("ok"), EOA_OWNER, {} },
        { ACCOUNT_B, QStringLiteral("20"), false, QStringLiteral("private"), {}, {} },
        { ACCOUNT_C, QStringLiteral("30"), true, QStringLiteral("ok"), PROGRAM_ID, QStringLiteral("00") },
    }, { { ACCOUNT_A, QStringLiteral("Trading") } }, ACCOUNT_A);

    QCOMPARE(model.count(), 3);
    QCOMPARE(countChanged.count(), 1);
    QCOMPARE(model.roleNames().value(WalletAccountModel::NameRole), QByteArray("name"));
    QCOMPARE(model.data(model.index(0), WalletAccountModel::NameRole).toString(),
             QStringLiteral("Trading"));
    QCOMPARE(model.data(model.index(0), WalletAccountModel::KindRole).toString(),
             QStringLiteral("user"));
    QVERIFY(model.data(model.index(0), WalletAccountModel::CanBePrimaryRole).toBool());
    QVERIFY(model.data(model.index(0), WalletAccountModel::IsPrimaryRole).toBool());
    QCOMPARE(model.data(model.index(1), WalletAccountModel::AddressRole).toString(), ACCOUNT_B);
    QCOMPARE(model.roleNames().value(WalletAccountModel::DisplayAddressRole),
             QByteArray("displayAddress"));
    QCOMPARE(model.roleNames().value(WalletAccountModel::DecodedDataRole),
             QByteArray("decodedData"));
    QCOMPARE(model.data(model.index(1), WalletAccountModel::DisplayAddressRole).toString(),
             ACCOUNT_B);
    QCOMPARE(model.data(model.index(1), WalletAccountModel::BalanceRole).toString(),
             QStringLiteral("20"));
    QVERIFY(!model.data(model.index(1), WalletAccountModel::IsPublicRole).toBool());
    QCOMPARE(model.data(model.index(2), WalletAccountModel::KindRole).toString(),
             QStringLiteral("program"));
    QVERIFY(!model.data(model.index(2), WalletAccountModel::CanBePrimaryRole).toBool());

    QSignalSpy presentationsChanged(&model, &QAbstractItemModel::dataChanged);
    const QVector<WalletAccountPresentation> presentations {
        {
            ACCOUNT_A,
            QStringLiteral("program"),
            {},
            QStringLiteral("System"),
            QStringLiteral("UserAccount"),
            {},
            false,
            QStringLiteral("{\"public_key\":\"Public/test\"}"),
        },
        {
            ACCOUNT_C,
            QStringLiteral("token_holding"),
            QStringLiteral("TEST holding"),
            QStringLiteral("Token"),
            QStringLiteral("TokenHolding"),
            ACCOUNT_A,
            true,
        },
    };
    model.applyPresentations(presentations);
    QCOMPARE(presentationsChanged.count(), 1);
    QCOMPARE(model.data(model.index(2), WalletAccountModel::SectionRole).toString(),
             QStringLiteral("hidden"));
    QCOMPARE(model.data(model.index(2), WalletAccountModel::NameRole).toString(),
             QStringLiteral("TEST holding"));
    QCOMPARE(model.data(model.index(0), WalletAccountModel::DecodedDataRole).toString(),
             QStringLiteral("{\"public_key\":\"Public/test\"}"));
    model.setAlias(ACCOUNT_C, QStringLiteral("Reserve"));
    QCOMPARE(model.data(model.index(2), WalletAccountModel::NameRole).toString(),
             QStringLiteral("Reserve"));
    model.setAlias(ACCOUNT_C, {});
    QCOMPARE(model.data(model.index(2), WalletAccountModel::NameRole).toString(),
             QStringLiteral("TEST holding"));

    QSignalSpy redundantPresentation(&model, &QAbstractItemModel::dataChanged);
    QVERIFY(!model.applyPresentations(presentations));
    QCOMPARE(redundantPresentation.count(), 0);
}

void LogosWalletProviderTest::clearsStaleAccountPresentationsWithoutInvalidatingPrimary()
{
    const QString settingsApplication = QStringLiteral("WalletPresentationClearingTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.connectResult.adopted = true;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("10"), true, QStringLiteral("ok"), EOA_OWNER, {} },
        { ACCOUNT_C, QStringLiteral("20"), true, QStringLiteral("ok"), PROGRAM_ID,
          QStringLiteral("00ff") },
    };
    WalletController controller(provider, settingsApplication);

    QVERIFY(controller.open());
    QCOMPARE(controller.state().primaryAccountAddress, ACCOUNT_A);
    QCOMPARE(controller.state().primaryAccountName, QStringLiteral("User account"));

    controller.applyAccountPresentations({
        {
            ACCOUNT_C,
            QStringLiteral("token_holding"),
            QStringLiteral("TEST holding"),
            QStringLiteral("Token"),
            QStringLiteral("TokenHolding"),
            ACCOUNT_A,
            true,
            QStringLiteral("{\"amount\":\"20\"}"),
        },
    });

    WalletAccountModel* model = controller.accountModel();
    const QModelIndex holding = model->index(model->indexOf(ACCOUNT_C));
    QCOMPARE(model->data(holding, WalletAccountModel::KindRole).toString(),
             QStringLiteral("token_holding"));
    QCOMPARE(model->data(holding, WalletAccountModel::SectionRole).toString(),
             QStringLiteral("hidden"));
    QCOMPARE(model->data(holding, WalletAccountModel::NameRole).toString(),
             QStringLiteral("TEST holding"));
    QCOMPARE(model->data(holding, WalletAccountModel::DecodedDataRole).toString(),
             QStringLiteral("{\"amount\":\"20\"}"));

    controller.applyAccountPresentations({});

    QCOMPARE(model->data(holding, WalletAccountModel::KindRole).toString(),
             QStringLiteral("program"));
    QCOMPARE(model->data(holding, WalletAccountModel::SectionRole).toString(),
             QStringLiteral("advanced"));
    QCOMPARE(model->data(holding, WalletAccountModel::NameRole).toString(),
             QStringLiteral("Program account"));
    QCOMPARE(model->data(holding, WalletAccountModel::ProgramNameRole).toString(), QString());
    QCOMPARE(model->data(holding, WalletAccountModel::AccountTypeRole).toString(), QString());
    QCOMPARE(model->data(holding, WalletAccountModel::DefinitionIdRole).toString(), QString());
    QCOMPARE(model->data(holding, WalletAccountModel::DecodedDataRole).toString(), QString());
    QVERIFY(!model->data(holding, WalletAccountModel::CanBePrimaryRole).toBool());

    const QModelIndex primary = model->index(model->indexOf(ACCOUNT_A));
    QVERIFY(model->data(primary, WalletAccountModel::CanBePrimaryRole).toBool());
    QVERIFY(model->data(primary, WalletAccountModel::IsPrimaryRole).toBool());
    QCOMPARE(controller.state().primaryAccountAddress, ACCOUNT_A);
    QCOMPARE(controller.state().primaryAccountName, QStringLiteral("User account"));

    settings.clear();
}

void LogosWalletProviderTest::persistsHumanizedWalletPreferences()
{
    const QString application = QStringLiteral("HumanizedWalletPreferencesTest");
    QSettings settings(QStringLiteral("Logos"), application);
    settings.clear();
    FakeWalletProvider provider;
    provider.connectResult.adopted = true;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("10"), true, QStringLiteral("ok"), EOA_OWNER, {} },
        { ACCOUNT_B, QStringLiteral("20"), false, QStringLiteral("private"), {}, {} },
        { ACCOUNT_C, QStringLiteral("30"), true, QStringLiteral("ok"), PROGRAM_ID, {} },
    };

    {
        WalletController controller(provider, application);
        QVERIFY(controller.open());
        QCOMPARE(controller.state().primaryAccountAddress, ACCOUNT_A);
        QVERIFY(!controller.setPrimaryAccount(ACCOUNT_C));
        QVERIFY(controller.setAccountAlias(ACCOUNT_B, QStringLiteral("  Private savings  ")));
        QVERIFY(controller.setPrimaryAccount(ACCOUNT_B));
        QCOMPARE(controller.state().primaryAccountName, QStringLiteral("Private savings"));
        QVERIFY(!controller.setAccountAlias(ACCOUNT_A, QString(41, QLatin1Char('x'))));
    }

    WalletController reopened(provider, application);
    QVERIFY(reopened.open());
    QCOMPARE(reopened.state().primaryAccountAddress, ACCOUNT_B);
    QCOMPARE(reopened.state().primaryAccountName, QStringLiteral("Private savings"));
    settings.clear();
}

void LogosWalletProviderTest::fakeProviderImplementsConsumerContract()
{
    FakeWalletProvider provider;
    provider.snapshotResult.accounts = { { ACCOUNT_A, QStringLiteral("5"), true } };
    provider.submissionResult.nativeHash = QString(64, QLatin1Char('d'));

    QCOMPARE(provider.snapshot(true).accounts.size(), 1);
    QVERIFY(provider.lastForceRefresh);
    QCOMPARE(provider.readPublicAccount(ACCOUNT_B).accountId, ACCOUNT_B);
    QCOMPARE(provider.readCalls, 1);
    WalletTransaction transaction { PROGRAM_ID, { ACCOUNT_A }, { true }, { 9 }, ACCOUNT_A };
    QVERIFY(provider.submitPublicTransaction(transaction).accepted());
    QCOMPARE(provider.lastTransaction.instruction, transaction.instruction);
    provider.disconnect();
    QCOMPARE(provider.disconnectCalls, 1);
}

void LogosWalletProviderTest::controllerOwnsUiWalletFlow()
{
    const QString settingsApplication = QStringLiteral("WalletControllerTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    // walletExists checks the default storage, so don't depend on the host's wallet.
    QTemporaryDir walletHome;
    QVERIFY(walletHome.isValid());
    QFile storage(walletHome.filePath(QStringLiteral("storage.json")));
    QVERIFY(storage.open(QIODevice::WriteOnly));
    storage.close();
    const QByteArray previousHome = qgetenv("LEE_WALLET_HOME_DIR");
    const bool hadPreviousHome = qEnvironmentVariableIsSet("LEE_WALLET_HOME_DIR");
    qputenv("LEE_WALLET_HOME_DIR", walletHome.path().toLocal8Bit());
    const auto restoreHome = qScopeGuard([&]() {
        if (hadPreviousHome)
            qputenv("LEE_WALLET_HOME_DIR", previousHome);
        else
            qunsetenv("LEE_WALLET_HOME_DIR");
    });

    FakeWalletProvider provider;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    provider.connectResult.snapshot.lastSyncedBlock = 7;
    provider.connectResult.snapshot.currentBlockHeight = 8;

    WalletController controller(provider, settingsApplication);
    QSignalSpy stateChanged(&controller, &WalletController::stateChanged);

    QVERIFY(controller.open());
    QCOMPARE(provider.connectCalls, 1);
    QVERIFY(controller.state().isWalletOpen);
    QVERIFY(controller.state().walletExists);
    QCOMPARE(provider.lastPaths.statistics,
             QFileInfo(provider.lastPaths.config).absolutePath() + QStringLiteral("/statistics.json"));
    QCOMPARE(controller.state().lastSyncedBlock, 7);
    QCOMPARE(controller.state().currentBlockHeight, 8);
    QCOMPARE(controller.accountModel()->count(), 1);

    provider.snapshotResult.accounts = {
        { ACCOUNT_A, QStringLiteral("9"), true },
    };
    controller.refresh();
    QVERIFY(provider.lastForceRefresh);
    QCOMPARE(controller.balance(ACCOUNT_A, true), QStringLiteral("9"));

    provider.createAccountResult.accountId = ACCOUNT_B;
    provider.createAccountResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("9"), true },
        { ACCOUNT_B, QStringLiteral("3"), false },
    };
    QCOMPARE(controller.createAccount(false), ACCOUNT_B);
    QVERIFY(!provider.lastAccountWasPublic);
    QCOMPARE(controller.accountModel()->count(), 2);

    controller.disconnect();
    QCOMPARE(provider.disconnectCalls, 1);
    QVERIFY(!controller.state().isWalletOpen);
    QCOMPARE(controller.accountModel()->count(), 0);
    QVERIFY(stateChanged.count() >= 4);

    settings.clear();
}



void LogosWalletProviderTest::controllerPreservesStatisticsPathForWalletLifecycle()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString config = directory.filePath(QStringLiteral("wallet.json"));
    const QString storage = directory.filePath(QStringLiteral("storage.json"));
    const QString statistics = directory.filePath(QStringLiteral("statistics.json"));
    const QString application = QStringLiteral("WalletStatisticsPathTest");
    QSettings settings(QStringLiteral("Logos"), application);
    settings.clear();
    FakeWalletProvider provider;
    provider.createWalletResult.mnemonic = QStringLiteral("test mnemonic");
    WalletController controller(provider, application);

    QCOMPARE(controller.createWallet(QUrl::fromLocalFile(config).toString(),
                                     QUrl::fromLocalFile(storage).toString(), {}),
             QStringLiteral("test mnemonic"));
    QCOMPARE(provider.lastPaths.config, config);
    QCOMPARE(provider.lastPaths.storage, storage);
    QCOMPARE(provider.lastPaths.statistics, statistics);

    controller.disconnect();
    QVERIFY(controller.open());
    QCOMPARE(provider.lastPaths.config, config);
    QCOMPARE(provider.lastPaths.storage, storage);
    QCOMPARE(provider.lastPaths.statistics, statistics);
    settings.clear();
}

void LogosWalletProviderTest::controllerRejectsDuplicateOpenWhileStarting()
{
    const QString settingsApplication = QStringLiteral("WalletDuplicateOpenTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    WalletController controller(provider, settingsApplication);

    QVERIFY(controller.open());
    QCOMPARE(provider.connectCalls, 1);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("opening"));
    QVERIFY(!controller.open());
    QCOMPARE(provider.connectCalls, 1);

    provider.finishConnect();
    QTRY_COMPARE_WITH_TIMEOUT(controller.state().syncStatus,
                              QStringLiteral("ready"), 1000);
    QVERIFY(controller.state().canSubmit());
    QVERIFY(!controller.open());
    QCOMPARE(provider.connectCalls, 1);

    settings.clear();
}

void LogosWalletProviderTest::controllerCanRetryAfterOpenFailure()
{
    const QString settingsApplication = QStringLiteral("WalletRetryOpenTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.connectResult.failure = WalletFailure::CapabilityUnavailable;
    WalletController controller(provider, settingsApplication);

    QVERIFY(controller.open());
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("capability_unavailable"));

    provider.connectResult = {};
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    QVERIFY(controller.open());
    QVERIFY(controller.state().isWalletOpen);
    QCOMPARE(provider.connectCalls, 2);

    settings.clear();
}

void LogosWalletProviderTest::controllerPollsSnapshotsAndRetriesAfterFailure()
{
    const QString settingsApplication = QStringLiteral("WalletSnapshotPollingTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    provider.connectResult.snapshot.lastSyncedBlock = 7;
    provider.connectResult.snapshot.currentBlockHeight = 8;

    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    provider.finishConnect();
    QTRY_COMPARE_WITH_TIMEOUT(controller.state().syncStatus,
                              QStringLiteral("ready"), 1000);
    QStringList statuses;
    connect(&controller, &WalletController::stateChanged, this, [&]() {
        statuses.append(controller.state().syncStatus);
    });

    auto* pollTimer = controller.findChild<QTimer*>(
        QStringLiteral("walletSnapshotPollTimer"));
    QVERIFY(pollTimer);
    // Unknown height (-1): the poll falls back to a full refresh.
    pollTimer->start(1);
    QTRY_COMPARE_WITH_TIMEOUT(provider.snapshotCalls, 1, 1000);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QVERIFY(controller.state().canSubmit());

    pollTimer->start(1);
    QTest::qWait(20);
    QCOMPARE(provider.snapshotCalls, 1);

    // Isolated failures keep the wallet usable; the third in a row reports it.
    provider.snapshotResult.failure = WalletFailure::ReadFailed;
    for (int failure = 1; failure <= 3; ++failure) {
        provider.finishSnapshot();
        QVERIFY(pollTimer->isActive());
        if (failure < 3) {
            QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
            QVERIFY(controller.state().syncError.isEmpty());
        }
        pollTimer->setInterval(1);
        QTRY_COMPARE_WITH_TIMEOUT(provider.snapshotCalls, failure + 1, 1000);
    }
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("read_failed"));
    QVERIFY(controller.state().canSubmit());

    provider.snapshotResult = {};
    provider.snapshotResult.accounts = {
        { ACCOUNT_A, QStringLiteral("9"), true },
    };
    provider.finishSnapshot();
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QVERIFY(controller.state().syncError.isEmpty());
    QCOMPARE(controller.balance(ACCOUNT_A, true), QStringLiteral("9"));
    QVERIFY(!statuses.contains(QStringLiteral("syncing")));

    controller.disconnect();
    settings.clear();
}

void LogosWalletProviderTest::controllerPollSkipsSyncWithoutNewBlocks()
{
    const QString settingsApplication = QStringLiteral("WalletSnapshotHeightTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.connectResult.snapshot.lastSyncedBlock = 7;
    provider.connectResult.snapshot.currentBlockHeight = 7;

    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    auto* pollTimer = controller.findChild<QTimer*>(
        QStringLiteral("walletSnapshotPollTimer"));
    QVERIFY(pollTimer);

    provider.blockHeightResult = 7;
    pollTimer->start(1);
    QTRY_COMPARE_WITH_TIMEOUT(provider.blockHeightCalls, 1, 1000);
    QCOMPARE(provider.snapshotCalls, 0);
    QVERIFY(pollTimer->isActive());

    provider.blockHeightResult = 9;
    provider.snapshotResult.lastSyncedBlock = 9;
    provider.snapshotResult.currentBlockHeight = 9;
    pollTimer->start(1);
    QTRY_COMPARE_WITH_TIMEOUT(provider.snapshotCalls, 1, 1000);
    QCOMPARE(provider.blockHeightCalls, 2);
    QCOMPARE(controller.state().lastSyncedBlock, 9);

    // A manual refresh always syncs, whatever the height.
    controller.refresh();
    QCOMPARE(provider.snapshotCalls, 2);
    QCOMPARE(provider.blockHeightCalls, 2);

    controller.disconnect();
    settings.clear();
}

void LogosWalletProviderTest::controllerManualRetryReportsOutcome()
{
    const QString settingsApplication = QStringLiteral("WalletManualRetryTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");

    WalletController controller(provider, settingsApplication);
    controller.createDefaultWallet(QStringLiteral("secret"));
    QVERIFY(controller.state().initialSync);
    // Refreshes wait for the initial sync instead of replacing it.
    controller.refresh();
    QCOMPARE(provider.snapshotCalls, 1);
    provider.finishSnapshot();
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));

    provider.snapshotResult.failure = WalletFailure::ReadFailed;
    for (int failure = 1; failure <= 3; ++failure) {
        controller.refresh();
        provider.finishSnapshot();
    }
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));

    // A retry from the error state always changes syncStatus, so the UI can
    // tell when it finished, whether it failed again or recovered.
    QStringList statuses;
    connect(&controller, &WalletController::stateChanged, this, [&]() {
        statuses.append(controller.state().syncStatus);
    });
    controller.refresh();
    provider.finishSnapshot();
    QCOMPARE(statuses, QStringList({ QStringLiteral("syncing"), QStringLiteral("error") }));

    statuses.clear();
    provider.snapshotResult = {};
    controller.refresh();
    provider.finishSnapshot();
    QCOMPARE(statuses, QStringList({ QStringLiteral("syncing"), QStringLiteral("ready") }));

    controller.disconnect();
    settings.clear();
}

void LogosWalletProviderTest::providerReadsBlockHeightWithoutSyncing()
{
    qint64 height = 0;
    {
        LogosModules modules;
        LogosWalletProvider provider(&modules);
        provider.currentBlockHeightAsync([&height](qint64 value) { height = value; });
        QTRY_COMPARE_WITH_TIMEOUT(height, qint64(-1), 1000);
    }

    QTemporaryDir directory;
    QFile storage(directory.filePath(QStringLiteral("storage.json")));
    QVERIFY(storage.open(QIODevice::WriteOnly));
    storage.close();
    LogosModules modules;
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({ {}, storage.fileName(), {} }).ok());
    const int syncs = modules.lez_core.syncCalls;
    const int saves = modules.lez_core.saveCalls;
    modules.lez_core.currentBlockHeight = 12;
    height = 0;
    provider.currentBlockHeightAsync([&height](qint64 value) { height = value; });
    QTRY_COMPARE_WITH_TIMEOUT(height, qint64(12), 1000);
    QCOMPARE(modules.lez_core.syncCalls, syncs);
    QCOMPARE(modules.lez_core.saveCalls, saves);
}

void LogosWalletProviderTest::controllerExposesProgressAndCancelsInitialSync()
{
    const QString settingsApplication = QStringLiteral("WalletSyncProgressTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };

    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    QVERIFY(!controller.state().canSubmit());
    provider.reportConnectProgress({ true, 100, 500, 400 });
    QTRY_COMPARE_WITH_TIMEOUT(controller.state().syncCurrentBlock, 100, 1000);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("syncing"));
    QCOMPARE(controller.state().syncTargetBlock, 500);
    QCOMPARE(controller.state().syncRemainingBlocks, 400);
    QVERIFY(controller.state().syncProgressKnown);

    controller.cancelSync();
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("sync_cancelled"));
    QVERIFY(!controller.state().canSubmit());
    QVERIFY(!controller.state().syncProgressKnown);
    provider.reportConnectProgress({ true, 200, 500, 300 });
    provider.finishConnect();
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QVERIFY(!controller.state().isWalletOpen);

    controller.disconnect();

    settings.clear();
}

void LogosWalletProviderTest::controllerCreatesWalletBeforeAsyncSyncCompletes()
{
    const QString settingsApplication = QStringLiteral("WalletCreationSyncProgressTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");

    WalletController controller(provider, settingsApplication);
    QCOMPARE(controller.createDefaultWallet(QStringLiteral("secret")),
             QStringLiteral("one two three"));
    QCOMPARE(controller.state().syncStatus, QStringLiteral("syncing"));
    QVERIFY(controller.state().isWalletOpen);
    provider.reportSnapshotProgress({ true, 8, 20, 12 });
    QTRY_COMPARE_WITH_TIMEOUT(controller.state().syncCurrentBlock, 8, 1000);

    provider.finishSnapshot();
    QTRY_COMPARE_WITH_TIMEOUT(controller.state().syncStatus,
                              QStringLiteral("ready"), 1000);
    QVERIFY(!controller.state().syncProgressKnown);

    controller.disconnect();
    settings.clear();
}

void LogosWalletProviderTest::controllerStopsReachabilityChecksAfterDisconnect()
{
    const QString settingsApplication = QStringLiteral("WalletReachabilityTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.connectResult.snapshot.sequencerAddress = QStringLiteral("http://127.0.0.1:1");
    WalletController controller(provider, settingsApplication);
    auto* network = controller.findChild<QNetworkAccessManager*>();
    QVERIFY(network);
    QSignalSpy finished(network, &QNetworkAccessManager::finished);

    QVERIFY(controller.open());
    QTRY_VERIFY_WITH_TIMEOUT(!finished.isEmpty(), 1000);
    auto* timer = controller.findChild<QTimer*>();
    QVERIFY(timer);
    QVERIFY(timer->isActive());
    controller.disconnect();
    QVERIFY(!timer->isActive());
    finished.clear();

    timer->setInterval(1);
    controller.start();
    QTest::qWait(50);
    QVERIFY(!timer->isActive());
    QCOMPARE(finished.count(), 0);

    settings.clear();
}

void LogosWalletProviderTest::controllerSelectsFundedFeePayer()
{
    const QString settingsApplication = QStringLiteral("WalletFeePayerTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    // u128 balances beyond 64 bits, a private account, and one with a positive
    // balance below the minimum fee reserve.
    FakeWalletProvider provider;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("16000007"), true },
        { ACCOUNT_B, QStringLiteral("99999999999999999999"), true },
        { ACCOUNT_D, QStringLiteral("100000000000000000000"), true },
        { PROGRAM_ID, QStringLiteral("900000000000000000000"), false },
    };
    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    QCOMPARE(controller.state().feePayerId, ACCOUNT_D);

    // The account model exposes the same eligibility to the QML payer picker.
    const WalletAccountModel* model = controller.accountModel();
    const auto payable = [model](int row) {
        return model->data(model->index(row), WalletAccountModel::CanPayFeesRole).toBool();
    };
    QVERIFY(!payable(0));
    QVERIFY(payable(1));
    QVERIFY(payable(2));
    QVERIFY(!payable(3));

    QVERIFY(!controller.setFeePayer(ACCOUNT_A));
    QVERIFY(!controller.setFeePayer(PROGRAM_ID));
    QVERIFY(controller.setFeePayer(ACCOUNT_B.toUpper()));
    QCOMPARE(controller.state().feePayerId, ACCOUNT_B);

    // The choice survives a restart. A saved payer with a positive balance below
    // the fee reserve is passed over for a funded account, without forgetting it.
    WalletController restarted(provider, settingsApplication);
    QVERIFY(restarted.open());
    QCOMPARE(restarted.state().feePayerId, ACCOUNT_B);
    provider.snapshotResult.accounts = {
        { ACCOUNT_B, QStringLiteral("1"), true },
        { ACCOUNT_D, QStringLiteral("1000000000"), true },
    };
    restarted.refresh();
    QCOMPARE(restarted.state().feePayerId, ACCOUNT_D);

    // Exactly the reserve qualifies, so the saved choice comes back once refunded.
    provider.snapshotResult.accounts = {
        { ACCOUNT_B, QStringLiteral("16000008"), true },
        { ACCOUNT_D, QStringLiteral("1000000000"), true },
    };
    restarted.refresh();
    QCOMPARE(restarted.state().feePayerId, ACCOUNT_B);

    provider.snapshotResult.accounts = { { ACCOUNT_A, QStringLiteral("16000007"), true } };
    restarted.refresh();
    QVERIFY(restarted.state().feePayerId.isEmpty());

    restarted.disconnect();
    QVERIFY(restarted.state().feePayerId.isEmpty());

    settings.clear();
}

void LogosWalletProviderTest::controllerSeparatesSnapshotsFromCosmeticState()
{
    const QString settingsApplication = QStringLiteral("WalletSnapshotSignalTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    provider.snapshotResult = provider.connectResult.snapshot;
    WalletController controller(provider, settingsApplication);
    QSignalSpy stateChanged(&controller, &WalletController::stateChanged);
    QSignalSpy snapshotChanged(&controller, &WalletController::snapshotChanged);

    QVERIFY(controller.open());
    stateChanged.clear();
    snapshotChanged.clear();

    QVERIFY(controller.setAccountAlias(ACCOUNT_A, QStringLiteral("Spending")));
    QCOMPARE(stateChanged.count(), 1);
    QCOMPARE(snapshotChanged.count(), 0);

    controller.refresh();
    QCOMPARE(stateChanged.count(), 2);
    QCOMPARE(snapshotChanged.count(), 1);
    settings.clear();
}

void LogosWalletProviderTest::controllerOpenDoesNotWaitForWalletSync()
{
    const QString settingsApplication = QStringLiteral("WalletAsyncOpenTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.connectResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    WalletController controller(provider, settingsApplication);

    QVERIFY(controller.open());
    QCOMPARE(provider.connectCalls, 1);
    QVERIFY(!controller.state().isWalletOpen);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("opening"));
    QCOMPARE(controller.accountModel()->count(), 0);

    provider.finishConnect();
    QVERIFY(controller.state().isWalletOpen);
    QVERIFY(controller.state().canSubmit());
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QCOMPARE(controller.accountModel()->count(), 1);
    settings.clear();
}

void LogosWalletProviderTest::controllerCreationDoesNotWaitForWalletSync()
{
    const QString settingsApplication = QStringLiteral("WalletAsyncCreationTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");
    provider.snapshotResult.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true },
    };
    WalletController controller(provider, settingsApplication);

    QCOMPARE(controller.createDefaultWallet(QStringLiteral("secret")),
             provider.createWalletResult.mnemonic);
    QCOMPARE(provider.createWalletCalls, 1);
    QVERIFY(controller.state().isWalletOpen);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("syncing"));
    QVERIFY(!controller.state().canSubmit());
    QCOMPARE(controller.accountModel()->count(), 0);

    QTRY_COMPARE(provider.snapshotCalls, 1);
    QVERIFY(provider.lastForceRefresh);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("syncing"));
    provider.finishSnapshot();

    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QVERIFY(controller.state().canSubmit());
    QCOMPARE(controller.accountModel()->count(), 1);
    settings.clear();
}

void LogosWalletProviderTest::controllerSeedsDefaultWalletConfigWithConfiguredEndpoint()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString walletHome = directory.filePath(QStringLiteral("wallet"));
    ScopedEnvironment walletHomeEnvironment(
        QByteArrayLiteral("LEE_WALLET_HOME_DIR"), walletHome.toLocal8Bit());
    const QString settingsApplication = QStringLiteral("WalletDefaultEndpointTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");
    WalletController controller(provider, settingsApplication);
    controller.setDefaultSequencerAddress(QStringLiteral("https://testnet.lez.logos.co/"));

    QCOMPARE(controller.createDefaultWallet(QStringLiteral("secret")),
             provider.createWalletResult.mnemonic);
    const QString configPath = walletHome + QStringLiteral("/wallet_config.json");
    QCOMPARE(provider.lastPaths.config, configPath);

    QFile config(configPath);
    QVERIFY(config.open(QIODevice::ReadOnly));
    const QJsonDocument document = QJsonDocument::fromJson(config.readAll());
    QVERIFY(document.isObject());
    const QJsonObject values = document.object();
    const QJsonArray sequencers = values.value(QStringLiteral("sequencers")).toArray();
    QCOMPARE(sequencers.size(), 1);
    QCOMPARE(sequencers.first().toObject()
                 .value(QStringLiteral("sequencer_addr"))
                 .toString(),
             QStringLiteral("https://testnet.lez.logos.co/"));
    QCOMPARE(values.value(QStringLiteral("seq_poll_timeout")).toString(),
             QStringLiteral("12s"));
    QCOMPARE(values.value(QStringLiteral("seq_tx_poll_max_blocks")).toInt(), 5);
    QCOMPARE(values.value(QStringLiteral("seq_poll_max_retries")).toInt(), 5);
    QCOMPARE(values.value(QStringLiteral("seq_block_poll_max_amount")).toInt(), 100);
    settings.clear();
}

void LogosWalletProviderTest::controllerPreservesExistingDefaultWalletConfig()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString walletHome = directory.filePath(QStringLiteral("wallet"));
    QVERIFY(QDir().mkpath(walletHome));
    const QString configPath = walletHome + QStringLiteral("/wallet_config.json");
    const QByteArray existingConfig = QByteArrayLiteral(
        "{\"sequencer_addr\":\"http://127.0.0.1:3040/\",\"custom\":true}");
    QFile config(configPath);
    QVERIFY(config.open(QIODevice::WriteOnly));
    QCOMPARE(config.write(existingConfig), qint64(existingConfig.size()));
    config.close();

    ScopedEnvironment walletHomeEnvironment(
        QByteArrayLiteral("LEE_WALLET_HOME_DIR"), walletHome.toLocal8Bit());
    const QString settingsApplication = QStringLiteral("WalletExistingEndpointTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");
    WalletController controller(provider, settingsApplication);
    controller.setDefaultSequencerAddress(QStringLiteral("https://testnet.lez.logos.co/"));

    QCOMPARE(controller.createDefaultWallet(QStringLiteral("secret")),
             provider.createWalletResult.mnemonic);
    QVERIFY(config.open(QIODevice::ReadOnly));
    QCOMPARE(config.readAll(), existingConfig);
    settings.clear();
}

void LogosWalletProviderTest::completedAsyncSnapshotReleasesCallback()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    bool completed = false;
    std::weak_ptr<int> callbackLifetime;
    {
        auto lifetime = std::make_shared<int>(1);
        callbackLifetime = lifetime;
        provider.snapshotAsync(true,
            [lifetime = std::move(lifetime), &completed](WalletSnapshot snapshot) {
                QVERIFY(snapshot.ok());
                completed = true;
            });
    }

    QVERIFY(completed);
    QVERIFY(callbackLifetime.expired());
    QCOMPARE(modules.lez_core.saveCalls, 0);
}

void LogosWalletProviderTest::deferredCallbacksIgnoreDestroyedController()
{
    const QString settingsApplication = QStringLiteral("WalletDestroyedCallbackTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.deferAsync = true;
    {
        auto controller = std::make_unique<WalletController>(provider, settingsApplication);
        QVERIFY(controller->open());
    }
    provider.finishConnect();

    provider.deferAsync = false;
    {
        auto controller = std::make_unique<WalletController>(provider, settingsApplication);
        QVERIFY(controller->open());
        provider.deferAsync = true;
        controller->refresh();
    }
    provider.finishSnapshot();
    settings.clear();
}

void LogosWalletProviderTest::newerReachabilityResultWins()
{
    const QString settingsApplication = QStringLiteral("WalletReachabilityOrderTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    QTcpServer firstServer;
    QTcpServer secondServer;
    QVERIFY(firstServer.listen(QHostAddress::LocalHost));
    QVERIFY(secondServer.listen(QHostAddress::LocalHost));
    const QString firstEndpoint = QStringLiteral("http://127.0.0.1:%1")
        .arg(firstServer.serverPort());
    const QString secondEndpoint = QStringLiteral("http://127.0.0.1:%1")
        .arg(secondServer.serverPort());

    FakeWalletProvider provider;
    provider.connectResult.snapshot.sequencerAddress = firstEndpoint;
    WalletController controller(provider, settingsApplication);
    auto* network = controller.findChild<QNetworkAccessManager*>();
    QVERIFY(network);
    QSignalSpy finished(network, &QNetworkAccessManager::finished);

    QVERIFY(controller.open());
    QTRY_VERIFY(firstServer.hasPendingConnections());
    QTcpSocket* first = firstServer.nextPendingConnection();
    QVERIFY(first);

    provider.createAccountResult.accountId = ACCOUNT_B;
    provider.createAccountResult.snapshot.sequencerAddress = secondEndpoint;
    QCOMPARE(controller.createAccount(true), ACCOUNT_B);
    QTRY_VERIFY(secondServer.hasPendingConnections());
    QTcpSocket* second = secondServer.nextPendingConnection();
    QVERIFY(second);

    second->write("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    second->disconnectFromHost();
    QTRY_COMPARE(finished.size(), 1);
    QVERIFY(controller.state().sequencerReachable);

    first->disconnectFromHost();
    QTRY_COMPARE(finished.size(), 2);
    QVERIFY(controller.state().sequencerReachable);
    settings.clear();
}

void LogosWalletProviderTest::coalescesReachabilityChecksForSameEndpoint()
{
    const QString settingsApplication = QStringLiteral("WalletReachabilityCoalesceTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    QTcpServer server;
    QVERIFY(server.listen(QHostAddress::LocalHost));
    const QString endpoint = QStringLiteral("http://127.0.0.1:%1").arg(server.serverPort());

    FakeWalletProvider provider;
    provider.connectResult.snapshot.sequencerAddress = endpoint;
    WalletController controller(provider, settingsApplication);
    QVERIFY(controller.open());
    QTRY_VERIFY(server.hasPendingConnections());
    QTcpSocket* request = server.nextPendingConnection();
    QVERIFY(request);

    provider.createAccountResult.accountId = ACCOUNT_B;
    provider.createAccountResult.snapshot.sequencerAddress = endpoint;
    QCOMPARE(controller.createAccount(true), ACCOUNT_B);
    QTest::qWait(50);
    QVERIFY(!server.hasPendingConnections());

    request->write("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    request->disconnectFromHost();
    settings.clear();
}

void LogosWalletProviderTest::controllerReportsPartialWalletCreation()
{
    const QString settingsApplication = QStringLiteral("WalletPartialCreationTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    provider.createWalletResult.mnemonic = QStringLiteral("one two three");
    provider.createWalletResult.failure = WalletFailure::SaveFailed;
    WalletController controller(provider, settingsApplication);

    QCOMPARE(controller.createDefaultWallet(QStringLiteral("secret")),
             provider.createWalletResult.mnemonic);
    QVERIFY(!controller.state().isWalletOpen);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("save_failed"));
    settings.clear();
}

QTEST_GUILESS_MAIN(LogosWalletProviderTest)

#include "LogosWalletProviderTest.moc"
