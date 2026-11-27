#include <QFile>
#include <QTemporaryDir>
#include <QtTest>

#include "LogosWalletProvider.h"
#include "logos_sdk.h"

namespace {
const QString account(64, QLatin1Char('a'));
const QString program(64, QLatin1Char('b'));
}

class LogosWalletActorTest : public QObject {
    Q_OBJECT
private slots:
    void opensAndCreatesWithPassword();
    void readsActorAccountAndFallsBackToBalance();
    void readsDecimalNonces_data();
    void readsDecimalNonces();
    void submitsActorTransaction();
};

void LogosWalletActorTest::opensAndCreatesWithPassword()
{
    QTemporaryDir dir;
    const WalletPaths paths { dir.filePath("config"), dir.filePath("storage"), {}, "secret" };
    QFile storage(paths.storage);
    QVERIFY(storage.open(QIODevice::WriteOnly));
    storage.close();
    LogosModules modules;
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect(paths).ok());
    QCOMPARE(modules.lez_core.openedPassword, QStringLiteral("secret"));
    QVERIFY(modules.lez_core.openedForceRpc);
    modules.lez_core.openResult = 25;
    QCOMPARE(provider.connect(paths).failure, WalletFailure::WrongPassword);
    modules.lez_core.openResult = 0;
    modules.lez_core.openedForceRpc = false;
    bool connected = false;
    provider.connectAsync(paths, [&connected](WalletSession session) { connected = session.ok(); });
    QTRY_VERIFY(connected);
    QVERIFY(modules.lez_core.openedForceRpc);
    provider.disconnect();
    QVERIFY(provider.createWallet(paths, "new-secret").ok());
    QCOMPARE(modules.lez_core.createdPassword, QStringLiteral("new-secret"));
    QVERIFY(modules.lez_core.createdForceRpc);
}

void LogosWalletActorTest::readsActorAccountAndFallsBackToBalance()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = "sequencer";
    modules.lez_core.accounts = { QVariantMap { {"account_id", account}, {"is_public", true} } };
    modules.lez_core.publicAccounts[account] = QStringLiteral(
        R"({"nonce":"0","actor_states":[{"program":")")
        + program + QStringLiteral(R"(","data":"00ff"}]})");
    modules.lez_core.balances[account] = "42";
    LogosWalletProvider provider(&modules);
    const auto session = provider.connect({});
    QVERIFY(session.ok());
    QVERIFY(provider.readPublicAccount(account).ok());
    QCOMPARE(session.snapshot.accounts.at(0).balance, QStringLiteral("42"));
    modules.lez_core.publicAccounts[account] = QStringLiteral(
        R"({"nonce":"0","actor_states":[{"program":"invalid","data":"00"}]})");
    QVERIFY(!provider.readPublicAccount(account).ok());
}

void LogosWalletActorTest::readsDecimalNonces_data()
{
    QTest::addColumn<QString>("nonce");
    QTest::addColumn<bool>("valid");
    QTest::newRow("max u128") << QStringLiteral("340282366920938463463374607431768211455") << true;
    QTest::newRow("overflow") << QStringLiteral("340282366920938463463374607431768211456") << false;
    QTest::newRow("hex") << QStringLiteral("ff") << false;
    QTest::newRow("empty") << QString() << false;
}

void LogosWalletActorTest::readsDecimalNonces()
{
    QFETCH(QString, nonce);
    QFETCH(bool, valid);
    LogosModules modules;
    modules.lez_core.sequencerAddress = "sequencer";
    modules.lez_core.publicAccounts[account] =
        QStringLiteral(R"({"nonce":"%1","actor_states":[]})").arg(nonce);
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());
    QCOMPARE(provider.readPublicAccount(account).ok(), valid);
}

void LogosWalletActorTest::submitsActorTransaction()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = "sequencer";
    modules.lez_core.transactionResponse = QStringLiteral(
        R"({"success":true,"tx_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})");
    LogosWalletProvider provider(&modules);
    QVERIFY(provider.connect({}).ok());
    WalletTransaction tx;
    tx.programId = program;
    tx.accountIds = { account, account };
    tx.signingRequirements = { true, false };
    tx.feePayerId = account;
    tx.rootMention = 1;
    tx.actorProgramIds = { program, program };
    tx.message = QByteArray::fromHex("00ff");
    QVERIFY(provider.submitPublicTransaction(tx).accepted());
    QCOMPARE(modules.lez_core.submittedRootMention, quint64(1));
    QCOMPARE(modules.lez_core.submittedActorPrograms, tx.actorProgramIds);
    QCOMPARE(modules.lez_core.submittedInstruction, tx.message);
    tx.rootMention = 2;
    QCOMPARE(provider.submitPublicTransaction(tx).failure, WalletFailure::InvalidRequest);
}

QTEST_GUILESS_MAIN(LogosWalletActorTest)
#include "LogosWalletActorTest.moc"
