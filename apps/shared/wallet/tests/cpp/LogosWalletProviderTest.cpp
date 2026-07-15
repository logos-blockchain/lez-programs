#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

#include "FakeWalletProvider.h"
#include "LogosWalletProvider.h"
#include "WalletAccountModel.h"
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
    const WalletAccountCreation creation = provider.createAccount(true);
    QVERIFY(creation.ok());
    QCOMPARE(creation.accountId, ACCOUNT_A);
    QVERIFY(creation.publicAccount.ok());
    QCOMPARE(creation.snapshot.accounts.size(), 1);
    QVERIFY(modules.logos_execution_zone.saveCalls > savesBeforeCreate);

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
    WalletTransaction transaction { PROGRAM_ID, { ACCOUNT_A }, { true }, { 9 } };
    QVERIFY(provider.submitPublicTransaction(transaction).accepted());
    QCOMPARE(provider.lastTransaction.instruction, transaction.instruction);
    provider.disconnect();
    QCOMPARE(provider.disconnectCalls, 1);
}

QTEST_GUILESS_MAIN(LogosWalletProviderTest)

#include "LogosWalletProviderTest.moc"
