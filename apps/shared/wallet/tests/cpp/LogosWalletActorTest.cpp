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
    QVERIFY(!modules.lez_core.openedForceRpc);
    provider.disconnect();
    QVERIFY(provider.createWallet(paths, "new-secret").ok());
    QCOMPARE(modules.lez_core.createdPassword, QStringLiteral("new-secret"));
}

void LogosWalletActorTest::readsActorAccountAndFallsBackToBalance()
{
    LogosModules modules;
    modules.lez_core.sequencerAddress = "sequencer";
    modules.lez_core.accounts = { QVariantMap { {"account_id", account}, {"is_public", true} } };
    modules.lez_core.publicAccounts[account] = QStringLiteral(
        R"({"nonce":"00000000000000000000000000000000","actor_states":[{"program":")")
        + program + QStringLiteral(R"(","data":"00ff"}]})");
    modules.lez_core.balances[account] = "42";
    LogosWalletProvider provider(&modules);
    const auto session = provider.connect({});
    QVERIFY(session.ok());
    QVERIFY(provider.readPublicAccount(account).ok());
    QCOMPARE(session.snapshot.accounts.at(0).balance, QStringLiteral("42"));
    modules.lez_core.publicAccounts[account] = QStringLiteral(
        R"({"nonce":"00000000000000000000000000000000","actor_states":[{"program":"invalid","data":"00"}]})");
    QVERIFY(!provider.readPublicAccount(account).ok());
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
