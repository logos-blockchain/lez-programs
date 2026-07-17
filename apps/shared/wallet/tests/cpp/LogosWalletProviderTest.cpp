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
    void preservesCreatedAccountWhenSnapshotRefreshFails();
    void dispatchesExactGenericTransaction();
    void rejectsInvalidSubmissionResponses();
    void encodesAccountIdsForDisplay();
    void exposesStableAccountModelRoles();
    void persistsHumanizedWalletPreferences();
    void fakeProviderImplementsConsumerContract();
    void controllerOwnsUiWalletFlow();
    void controllerReportsCreationPersistenceAndRefreshFailures();
    void controllerStopsReachabilityChecksAfterDisconnect();
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
    QVERIFY(provider.snapshot().ok());
    QCOMPARE(modules.logos_execution_zone.listCalls, listCalls);
    QCOMPARE(modules.logos_execution_zone.publicReadCalls, readCalls);

    QVERIFY(provider.snapshot(true).ok());
    QVERIFY(modules.logos_execution_zone.listCalls > listCalls);
    QVERIFY(modules.logos_execution_zone.publicReadCalls > readCalls);

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

void LogosWalletProviderTest::preservesCreatedAccountWhenSnapshotRefreshFails()
{
    LogosModules modules;
    modules.logos_execution_zone.sequencerAddress = QStringLiteral("http://sequencer");
    modules.logos_execution_zone.publicAccountId = ACCOUNT_A;
    modules.logos_execution_zone.publicAccounts.insert(ACCOUNT_A, publicAccountJson());
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());

    modules.logos_execution_zone.currentBlockHeight = 1;
    modules.logos_execution_zone.syncResult = 1;
    const WalletAccountCreation creation = provider.createAccount(true);

    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QCOMPARE(creation.snapshot.failure, WalletFailure::ReadFailed);
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

    model.applyPresentations({ {
        ACCOUNT_C,
        QStringLiteral("token_holding"),
        QStringLiteral("TEST holding"),
        QStringLiteral("Token"),
        QStringLiteral("TokenHolding"),
        ACCOUNT_A,
        true,
    } });
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
}

void LogosWalletProviderTest::encodesAccountIdsForDisplay()
{
    QCOMPARE(walletAccountIdToBase58(
                 QStringLiteral("00fe99e4fbd4c71f92e47c384c6235244c8cce39b6d6367e1e338eca0ffe01cb")),
             QStringLiteral("14tAtixMByFyJrcZVyWibitnijLgd59PfyrjdnYzo8La"));
    QCOMPARE(walletAccountIdToBase58(QString(64, QLatin1Char('0'))),
             QString(32, QLatin1Char('1')));
    QVERIFY(walletAccountIdToBase58(QStringLiteral("not-an-account-id")).isEmpty());
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

void LogosWalletProviderTest::controllerReportsCreationPersistenceAndRefreshFailures()
{
    const QString settingsApplication = QStringLiteral("WalletCreationFailureTest");
    QSettings settings(QStringLiteral("Logos"), settingsApplication);
    settings.clear();

    FakeWalletProvider provider;
    WalletController controller(provider, settingsApplication);
    const WalletUiState baseline = controller.state();
    const int baselineAccountCount = controller.accountModel()->count();
    QSignalSpy snapshotChanged(&controller, &WalletController::snapshotChanged);

    provider.createWalletResult.mnemonic = QStringLiteral("alpha beta gamma");
    provider.createWalletResult.failure = WalletFailure::SaveFailed;
    provider.createWalletResult.snapshot.failure = WalletFailure::SaveFailed;
    QCOMPARE(controller.createWallet(QStringLiteral("config"), QStringLiteral("storage"),
                                     QStringLiteral("secret")),
             QString());
    QCOMPARE(controller.state().isWalletOpen, baseline.isWalletOpen);
    QCOMPARE(controller.state().walletExists, baseline.walletExists);
    QCOMPARE(controller.state().syncStatus, baseline.syncStatus);
    QCOMPARE(controller.state().syncError, baseline.syncError);
    QCOMPARE(controller.accountModel()->count(), baselineAccountCount);
    QCOMPARE(snapshotChanged.count(), 0);

    provider.createWalletResult.failure = WalletFailure::ReadFailed;
    provider.createWalletResult.snapshot.failure = WalletFailure::ReadFailed;
    QCOMPARE(controller.createWallet(QStringLiteral("config"), QStringLiteral("storage"),
                                     QStringLiteral("secret")),
             QStringLiteral("alpha beta gamma"));
    QVERIFY(controller.state().isWalletOpen);
    QVERIFY(controller.state().walletExists);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("read_failed"));
    QCOMPARE(controller.accountModel()->count(), baselineAccountCount);
    QCOMPARE(snapshotChanged.count(), 0);

    provider.createAccountResult.accountId = ACCOUNT_B;
    provider.createAccountResult.snapshot.failure = WalletFailure::ReadFailed;
    QCOMPARE(controller.createAccount(true), ACCOUNT_B);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("error"));
    QCOMPARE(controller.state().syncError, QStringLiteral("read_failed"));
    QCOMPARE(controller.accountModel()->count(), baselineAccountCount);
    QCOMPARE(snapshotChanged.count(), 0);

    provider.createAccountResult.snapshot = {};
    provider.createAccountResult.snapshot.accounts = {
        { ACCOUNT_A, QStringLiteral("5"), true, QStringLiteral("ok"), EOA_OWNER, {} },
        { ACCOUNT_B, QStringLiteral("3"), true, QStringLiteral("ok"), EOA_OWNER, {} },
    };
    QCOMPARE(controller.createAccount(true), ACCOUNT_B);
    QCOMPARE(controller.state().syncStatus, QStringLiteral("ready"));
    QVERIFY(controller.state().syncError.isEmpty());
    QCOMPARE(controller.accountModel()->count(), 2);
    QCOMPARE(snapshotChanged.count(), 1);

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
