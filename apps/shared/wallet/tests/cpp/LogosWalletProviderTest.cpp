#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QHostAddress>
#include <QNetworkAccessManager>
#include <QSettings>
#include <QSignalSpy>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

#include <memory>
#include <utility>

#include "FakeWalletProvider.h"
#include "LogosWalletProvider.h"
#include "WalletAccountId.h"
#include "WalletAccountModel.h"
#include "WalletController.h"
#include "logos_sdk.h"

namespace {
const QString ACCOUNT_A(64, QLatin1Char('a'));
const QString ACCOUNT_B(64, QLatin1Char('b'));
const QString ACCOUNT_C(64, QLatin1Char('d'));
const QString PROGRAM_ID(64, QLatin1Char('c'));
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
    void opensConfiguredWalletWhenNoSharedSessionExists();
    void opensStoredWalletAsynchronouslyWithoutAccountProbe();
    void opensAndReadsAsynchronously();
    void avoidsSavingAfterUnchangedAsynchronousSnapshots();
    void createsAndPersistsWallet();
    void validatesCompletePublicAccountPayloads();
    void fallsBackToBalanceWhenPublicReadFails();
    void createsAndPersistsAccounts();
    void preservesCreatedAccountWhenPublicReadFails();
    void createdAccountDoesNotRescanWallet();
    void dispatchesExactGenericTransaction();
    void rejectsInvalidSubmissionResponses();
    void walletMutationsUseAsyncSdk();
    void staleAsyncMutationCannotCrossSession();
    void destroyedProviderIgnoresLateMutation();
    void exposesStableAccountModelRoles();
    void encodesAccountIdsForDisplay();
    void persistsHumanizedWalletPreferences();
    void fakeProviderImplementsConsumerContract();
    void controllerOwnsUiWalletFlow();
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
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.currentBlockHeight = 12;
    modules.logos_execution_zone.lastSyncedBlock = 11;
    modules.logos_execution_zone.accounts = {
        accountEntry(ACCOUNT_A, true),
        accountEntry(ACCOUNT_B, false),
    };
    modules.logos_execution_zone.publicAccounts.insert(
        ACCOUNT_A, publicAccountJson());
    modules.logos_execution_zone.balances.insert(ACCOUNT_B, QStringLiteral("42"));

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
    QCOMPARE(session.snapshot.accounts.at(1).readStatus, QStringLiteral("private"));
    QCOMPARE(session.snapshot.currentBlockHeight, quint64(12));
    QCOMPARE(session.snapshot.lastSyncedBlock, quint64(11));

    const int listCalls = modules.logos_execution_zone.listCalls;
    const int readCalls = modules.logos_execution_zone.publicReadCalls;
    const int saveCalls = modules.logos_execution_zone.saveCalls;
    QVERIFY(provider.snapshot().ok());
    QCOMPARE(modules.logos_execution_zone.listCalls, listCalls);
    QCOMPARE(modules.logos_execution_zone.publicReadCalls, readCalls);

    QVERIFY(provider.snapshot(true).ok());
    QVERIFY(modules.logos_execution_zone.listCalls > listCalls);
    QVERIFY(modules.logos_execution_zone.publicReadCalls > readCalls);
    QCOMPARE(modules.logos_execution_zone.saveCalls, saveCalls);

    modules.logos_execution_zone.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QString(32, QLatin1Char('f')));
    QCOMPARE(provider.snapshot(true).accounts.at(0).balance,
             QStringLiteral("340282366920938463463374607431768211455"));

    provider.clearSnapshot();
    const int afterRefresh = modules.logos_execution_zone.listCalls;
    QVERIFY(provider.snapshot().ok());
    QVERIFY(modules.logos_execution_zone.listCalls > afterRefresh);

    provider.disconnect();
    QCOMPARE(provider.snapshot().failure, WalletFailure::WalletUnavailable);
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
    });

    QVERIFY(session.ok());
    QVERIFY(!session.adopted);
    QCOMPARE(modules.logos_execution_zone.openCalls, 1);
    QCOMPARE(modules.logos_execution_zone.openedStorage, storage);
    QCOMPARE(modules.logos_execution_zone.listCalls, 1);

    LogosModules missingModules;
    LogosWalletProvider missingProvider(&missingModules);
    QCOMPARE(missingProvider.connect({ QStringLiteral("config"), QStringLiteral("missing") }).failure,
             WalletFailure::WalletMissing);
}

void LogosWalletProviderTest::opensStoredWalletAsynchronouslyWithoutAccountProbe()
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
    QCOMPARE(modules.logos_execution_zone.openCalls, 1);
    QCOMPARE(modules.logos_execution_zone.listCalls, 1);
}

void LogosWalletProviderTest::opensAndReadsAsynchronously()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.logos_execution_zone.publicAccounts.insert(
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

    bool batchRead = false;
    provider.readPublicAccountsAsync(
        { ACCOUNT_A, ACCOUNT_B },
        [&batchRead](QVector<WalletAccountRead> reads) {
            batchRead = reads.size() == 2
                && reads.at(0).ok()
                && !reads.at(1).ok();
        });
    QVERIFY(batchRead);
}

void LogosWalletProviderTest::avoidsSavingAfterUnchangedAsynchronousSnapshots()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.currentBlockHeight = 12;
    modules.logos_execution_zone.lastSyncedBlock = 12;
    LogosWalletProvider provider(&modules);

    bool connected = false;
    provider.connectAsync({}, [&connected](WalletSession session) {
        connected = session.ok();
    });
    QVERIFY(connected);
    QCOMPARE(modules.logos_execution_zone.saveCalls, 0);

    bool refreshed = false;
    provider.snapshotAsync(true, [&refreshed](WalletSnapshot snapshot) {
        refreshed = snapshot.ok();
    });
    QVERIFY(refreshed);
    QCOMPARE(modules.logos_execution_zone.saveCalls, 0);
}

void LogosWalletProviderTest::createsAndPersistsWallet()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    LogosModules modules;
    modules.logos_execution_zone.currentBlockHeight = 12;
    modules.logos_execution_zone.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    const WalletPaths paths {
        directory.filePath(QStringLiteral("config/wallet.json")),
        directory.filePath(QStringLiteral("state/storage.json")),
    };
    const WalletCreation creation = provider.createWallet(paths, QStringLiteral("secret"));

    QVERIFY(creation.ok());
    QCOMPARE(creation.mnemonic, modules.logos_execution_zone.mnemonic);
    QCOMPARE(modules.logos_execution_zone.createdConfig, paths.config);
    QCOMPARE(modules.logos_execution_zone.createdStorage, paths.storage);
    QCOMPARE(modules.logos_execution_zone.createdPassword, QStringLiteral("secret"));
    QVERIFY(modules.logos_execution_zone.saveCalls >= 1);
    QCOMPARE(modules.logos_execution_zone.syncCalls, 0);
    QCOMPARE(modules.logos_execution_zone.listCalls, 0);
    QCOMPARE(modules.logos_execution_zone.publicReadCalls, 0);

    LogosModules rejectedModules;
    rejectedModules.logos_execution_zone.mnemonic.clear();
    LogosWalletProvider rejectedProvider(&rejectedModules);
    QCOMPARE(rejectedProvider.createWallet(paths, QStringLiteral("secret")).failure,
             WalletFailure::CreateFailed);

    LogosModules unsavedModules;
    unsavedModules.logos_execution_zone.saveResult = 1;
    LogosWalletProvider unsavedProvider(&unsavedModules);
    const WalletCreation unsaved = unsavedProvider.createWallet(paths, QStringLiteral("secret"));
    QCOMPARE(unsaved.failure, WalletFailure::SaveFailed);
    QCOMPARE(unsaved.mnemonic, unsavedModules.logos_execution_zone.mnemonic);
}

void LogosWalletProviderTest::validatesCompletePublicAccountPayloads()
{
    LogosModules modules;
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);

    const WalletAccountRead valid = provider.readPublicAccount(ACCOUNT_A);
    QVERIFY(valid.ok());
    QCOMPARE(valid.accountId, ACCOUNT_A);
    QCOMPARE(valid.programOwner, PROGRAM_ID);
    QCOMPARE(valid.balanceHex, QStringLiteral("01000000000000000000000000000000"));
    QCOMPARE(valid.dataHex, QStringLiteral("00ff"));

    modules.logos_execution_zone.publicAccounts[ACCOUNT_A] = publicAccountJson(PROGRAM_ID.toUpper());
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.logos_execution_zone.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QStringLiteral("01"));
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.logos_execution_zone.publicAccounts[ACCOUNT_A] = publicAccountJson(
        PROGRAM_ID, QStringLiteral("01000000000000000000000000000000"),
        QString(32, QLatin1Char('0')), QStringLiteral("abc"));
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    modules.logos_execution_zone.publicAccounts[ACCOUNT_A] = QStringLiteral("[]");
    QVERIFY(!provider.readPublicAccount(ACCOUNT_A).ok());
    QVERIFY(!provider.readPublicAccount(QStringLiteral("invalid")).ok());
}

void LogosWalletProviderTest::fallsBackToBalanceWhenPublicReadFails()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.logos_execution_zone.balances.insert(ACCOUNT_A, QStringLiteral("42"));

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
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    const int savesBeforeCreate = modules.logos_execution_zone.saveCalls;
    const int publicReadsBeforeCreate = modules.logos_execution_zone.publicReadCalls;
    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.publicAccount.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QVERIFY(modules.logos_execution_zone.saveCalls > savesBeforeCreate);
    QCOMPARE(modules.logos_execution_zone.publicReadCalls, publicReadsBeforeCreate + 1);

    modules.logos_execution_zone.saveResult = 1;
    QCOMPARE(provider.createAccount(true).failure, WalletFailure::SaveFailed);
}

void LogosWalletProviderTest::preservesCreatedAccountWhenPublicReadFails()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.accounts = { accountEntry(ACCOUNT_A, true) };
    modules.logos_execution_zone.balances.insert(ACCOUNT_A, QStringLiteral("7"));
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
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    modules.logos_execution_zone.currentBlockHeight = 1;
    modules.logos_execution_zone.syncResult = 1;
    const int syncCalls = modules.logos_execution_zone.syncCalls;
    const WalletAccountCreation creation = provider.createAccount(true);

    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.snapshot.ok());
    QCOMPARE(modules.logos_execution_zone.syncCalls, syncCalls);
}

void LogosWalletProviderTest::dispatchesExactGenericTransaction()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"})");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    WalletTransaction transaction;
    transaction.programId = PROGRAM_ID;
    transaction.accountIds = { ACCOUNT_A, ACCOUNT_B };
    transaction.signingRequirements = { true, false };
    transaction.instruction = { 7, 0, 4294967295U };

    const WalletSubmission submission = provider.submitPublicTransaction(transaction);
    QVERIFY(submission.accepted());
    QCOMPARE(submission.nativeHash, QString(64, QLatin1Char('a')));
    QCOMPARE(modules.logos_execution_zone.submitCalls, 1);
    QCOMPARE(modules.logos_execution_zone.submittedProgramId, PROGRAM_ID);
    QCOMPARE(modules.logos_execution_zone.submittedAccountIds, transaction.accountIds);
    QCOMPARE(modules.logos_execution_zone.submittedSigningRequirements,
             QVariantList({ true, false }));
    QCOMPARE(modules.logos_execution_zone.submittedInstruction.toList(),
             QVariantList({ 7U, 0U, 4294967295U }));
}

void LogosWalletProviderTest::rejectsInvalidSubmissionResponses()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    WalletTransaction transaction {
        PROGRAM_ID,
        { ACCOUNT_A },
        { true },
        { 1 },
    };

    const QStringList invalidResponses {
        QStringLiteral("not-json"),
        QStringLiteral(R"({"success":false,"tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})"),
        QStringLiteral(R"({"success":true,"error":"rejected","tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})"),
        QStringLiteral(R"({"success":true,"tx_hash":"short"})"),
    };
    for (const QString& response : invalidResponses) {
        modules.logos_execution_zone.transactionResponse = response;
        QCOMPARE(provider.submitPublicTransaction(transaction).failure,
                 WalletFailure::SubmissionFailed);
    }

    transaction.signingRequirements.clear();
    QCOMPARE(provider.submitPublicTransaction(transaction).failure,
             WalletFailure::InvalidRequest);
}

void LogosWalletProviderTest::walletMutationsUseAsyncSdk()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    modules.logos_execution_zone.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"})");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    modules.logos_execution_zone.deferPublicAccountCreation = true;
    bool creationFinished = false;
    WalletAccountCreation creation;
    const int listCalls = modules.logos_execution_zone.listCalls;
    const int syncCalls = modules.logos_execution_zone.syncCalls;
    provider.createAccountAsync(true, [&](WalletAccountCreation result) {
        creation = std::move(result);
        creationFinished = true;
    });
    QVERIFY(!creationFinished);
    QVERIFY(modules.logos_execution_zone.pendingPublicAccountCreation);
    modules.logos_execution_zone.finishPublicAccountCreation();
    QVERIFY(creationFinished);
    QVERIFY(creation.ok());
    QVERIFY(creation.publicAccount.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QCOMPARE(modules.logos_execution_zone.listCalls, listCalls);
    QCOMPARE(modules.logos_execution_zone.syncCalls, syncCalls);

    WalletTransaction transaction {
        PROGRAM_ID,
        { ACCOUNT_A, ACCOUNT_B },
        { true, false },
        { 7, 0, 4294967295U },
    };
    modules.logos_execution_zone.deferSubmission = true;
    bool submissionFinished = false;
    WalletSubmission submission;
    provider.submitPublicTransactionAsync(
        transaction, [&](WalletSubmission result) {
            submission = std::move(result);
            submissionFinished = true;
        });
    QVERIFY(!submissionFinished);
    QCOMPARE(modules.logos_execution_zone.submittedProgramId, PROGRAM_ID);
    QCOMPARE(modules.logos_execution_zone.submittedAccountIds, transaction.accountIds);
    QCOMPARE(modules.logos_execution_zone.submittedSigningRequirements,
             QVariantList({ true, false }));
    QCOMPARE(modules.logos_execution_zone.submittedInstruction.toList(),
             QVariantList({ 7U, 0U, 4294967295U }));
    modules.logos_execution_zone.finishSubmission();
    QVERIFY(submissionFinished);
    QVERIFY(submission.accepted());
    QCOMPARE(submission.nativeHash, QString(64, QLatin1Char('a')));
}

void LogosWalletProviderTest::staleAsyncMutationCannotCrossSession()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    modules.logos_execution_zone.deferPublicAccountCreation = true;
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    int callbackCount = 0;
    WalletAccountCreation creation;
    provider.createAccountAsync(true, [&](WalletAccountCreation result) {
        ++callbackCount;
        creation = std::move(result);
    });
    provider.disconnect();
    modules.logos_execution_zone.finishPublicAccountCreation();

    QCOMPARE(callbackCount, 1);
    QCOMPARE(creation.failure, WalletFailure::WalletUnavailable);
    QCOMPARE(modules.logos_execution_zone.publicReadCalls, 0);
}

void LogosWalletProviderTest::destroyedProviderIgnoresLateMutation()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})");
    modules.logos_execution_zone.deferSubmission = true;
    int callbackCount = 0;
    {
        LogosWalletProvider provider(&modules);
        QVERIFY(provider.connect({}).ok());
        provider.submitPublicTransactionAsync(
            { PROGRAM_ID, { ACCOUNT_A }, { true }, { 1 } },
            [&](WalletSubmission) { ++callbackCount; });
    }

    modules.logos_execution_zone.finishSubmission();
    QCOMPARE(callbackCount, 0);
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
    QCOMPARE(model.data(model.index(1), WalletAccountModel::DisplayAddressRole).toString(),
             walletAccountIdToBase58(ACCOUNT_B));
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
        },
        {
            walletAccountIdToBase58(ACCOUNT_C),
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

void LogosWalletProviderTest::encodesAccountIdsForDisplay()
{
    QCOMPARE(walletAccountIdToBase58(
                 QStringLiteral("00fe99e4fbd4c71f92e47c384c6235244c8cce39b6d6367e1e338eca0ffe01cb")),
             QStringLiteral("14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La"));
    QCOMPARE(walletAccountIdFromBase58(
                 QStringLiteral("14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La")),
             QStringLiteral("00fe99e4fbd4c71f92e47c384c6235244c8cce39b6d6367e1e338eca0ffe01cb"));
    QCOMPARE(walletAccountIdToBase58(QString(64, QLatin1Char('0'))),
             QString(32, QLatin1Char('1')));
    QCOMPARE(walletAccountIdFromBase58(QString(32, QLatin1Char('1'))),
             QString(64, QLatin1Char('0')));
    QVERIFY(walletAccountIdToBase58(QStringLiteral("not-an-account-id")).isEmpty());
    QVERIFY(walletAccountIdFromBase58(QString(32, QLatin1Char('0'))).isEmpty());
    QVERIFY(walletAccountIdFromBase58(QString(45, QLatin1Char('1'))).isEmpty());
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
    WalletTransaction transaction { PROGRAM_ID, { ACCOUNT_A }, { true }, { 9 } };
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
    QCOMPARE(stateChanged.count(), 3);
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
    QCOMPARE(provider.snapshotCalls, 0);
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
    QCOMPARE(values.value(QStringLiteral("sequencer_addr")).toString(),
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

void LogosWalletProviderTest::completedAsyncSnapshotReleasesCallback()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
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
    QCOMPARE(modules.logos_execution_zone.saveCalls, 0);
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
