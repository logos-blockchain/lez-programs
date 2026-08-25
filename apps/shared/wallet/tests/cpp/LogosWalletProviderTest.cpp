#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

#include "FakeWalletProvider.h"
#include "LogosWalletProvider.h"
#include "WalletAccountModel.h"
#include "WalletController.h"
#include "logos_sdk.h"

namespace {
const QString ACCOUNT_A(64, QLatin1Char('a'));
const QString ACCOUNT_B(64, QLatin1Char('b'));
const QString PROGRAM_ID(64, QLatin1Char('c'));

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
    void createsAndPersistsWallet();
    void validatesCompletePublicAccountPayloads();
    void fallsBackToBalanceWhenPublicReadFails();
    void createsAndPersistsAccounts();
    void preservesCreatedAccountWhenPublicReadFails();
    void preservesCreatedAccountWhenSnapshotRefreshFails();
    void dispatchesExactGenericTransaction();
    void rejectsInvalidSubmissionResponses();
    void exposesStableAccountModelRoles();
    void fakeProviderImplementsConsumerContract();
    void controllerOwnsUiWalletFlow();
    void controllerStopsReachabilityChecksAfterDisconnect();
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
    QCOMPARE(session.snapshot.currentBlockHeight, quint64(12));
    QCOMPARE(session.snapshot.lastSyncedBlock, quint64(11));

    const int listCalls = modules.lez_core.listCalls;
    const int readCalls = modules.lez_core.publicReadCalls;
    QVERIFY(provider.snapshot().ok());
    QCOMPARE(modules.lez_core.listCalls, listCalls);
    QCOMPARE(modules.lez_core.publicReadCalls, readCalls);

    QVERIFY(provider.snapshot(true).ok());
    QVERIFY(modules.lez_core.listCalls > listCalls);
    QVERIFY(modules.lez_core.publicReadCalls > readCalls);

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
    QCOMPARE(modules.lez_core.openCalls, 1);
    QCOMPARE(modules.lez_core.openedStorage, storage);

    LogosModules missingModules;
    LogosWalletProvider missingProvider(&missingModules);
    QCOMPARE(missingProvider.connect({ QStringLiteral("config"), QStringLiteral("missing") }).failure,
             WalletFailure::WalletMissing);
}

void LogosWalletProviderTest::createsAndPersistsWallet()
{
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    LogosModules modules;
    LogosWalletProvider provider(&modules);
    const WalletPaths paths {
        directory.filePath(QStringLiteral("config/wallet.json")),
        directory.filePath(QStringLiteral("state/storage.json")),
    };
    const WalletCreation creation = provider.createWallet(paths, QStringLiteral("secret"));

    QVERIFY(creation.ok());
    QCOMPARE(creation.mnemonic, modules.lez_core.mnemonic);
    QCOMPARE(modules.lez_core.createdConfig, paths.config);
    QCOMPARE(modules.lez_core.createdStorage, paths.storage);
    QCOMPARE(modules.lez_core.createdPassword, QStringLiteral("secret"));
    QVERIFY(modules.lez_core.saveCalls >= 1);

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
    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.publicAccount.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QVERIFY(modules.lez_core.saveCalls > savesBeforeCreate);

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

void LogosWalletProviderTest::preservesCreatedAccountWhenSnapshotRefreshFails()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = QStringLiteral("http://sequencer");
    modules.lez_core.publicAccountId = ACCOUNT_A;
    modules.lez_core.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    modules.lez_core.currentBlockHeight = 1;
    modules.lez_core.syncResult = 1;
    const WalletAccountCreation creation = provider.createAccount(true);

    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QCOMPARE(creation.snapshot.failure, WalletFailure::ReadFailed);
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

    const WalletSubmission submission = provider.submitPublicTransaction(transaction);
    QVERIFY(submission.accepted());
    QCOMPARE(submission.nativeHash, QString(64, QLatin1Char('a')));
    QCOMPARE(modules.lez_core.submitCalls, 1);
    QCOMPARE(modules.lez_core.submittedProgramId, PROGRAM_ID);
    QCOMPARE(modules.lez_core.submittedAccountIds, transaction.accountIds);
    QCOMPARE(modules.lez_core.submittedSigningRequirements,
             QVariantList({ true, false }));
    QCOMPARE(modules.lez_core.submittedInstruction.toList(),
             QVariantList({ 7U, 0U, 4294967295U }));
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

void LogosWalletProviderTest::exposesStableAccountModelRoles()
{
    WalletAccountModel model;
    QSignalSpy countChanged(&model, &WalletAccountModel::countChanged);
    model.replaceAccounts({
        { ACCOUNT_A, QStringLiteral("10"), true },
        { ACCOUNT_B, QStringLiteral("20"), false },
    });

    QCOMPARE(model.count(), 2);
    QCOMPARE(countChanged.count(), 1);
    QCOMPARE(model.roleNames().value(WalletAccountModel::NameRole), QByteArray("name"));
    QCOMPARE(model.data(model.index(0), WalletAccountModel::NameRole).toString(),
             QStringLiteral("Account 1"));
    QCOMPARE(model.data(model.index(1), WalletAccountModel::AddressRole).toString(), ACCOUNT_B);
    QCOMPARE(model.data(model.index(1), WalletAccountModel::BalanceRole).toString(),
             QStringLiteral("20"));
    QVERIFY(!model.data(model.index(1), WalletAccountModel::IsPublicRole).toBool());
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
    controller.disconnect();
    finished.clear();

    auto* timer = controller.findChild<QTimer*>();
    QVERIFY(timer);
    timer->setInterval(1);
    controller.start();
    QTest::qWait(50);
    QCOMPARE(finished.count(), 0);

    settings.clear();
}

QTEST_GUILESS_MAIN(LogosWalletProviderTest)

#include "LogosWalletProviderTest.moc"
