#include "AmmClient.h"
#include "NewPositionRuntime.h"
#include "WalletProvider.h"

#include <QDateTime>

namespace {
    bool expect(bool condition, const char* message)
    {
        if (!condition)
            qCritical("%s", message);
        return condition;
    }

    class FakeWallet final : public WalletProvider {
    public:
        WalletSession connect(const WalletPaths&) override
        {
            return {};
        }

        WalletCreation createWallet(const WalletPaths&, const QString&) override
        {
            return {};
        }

        WalletSnapshot snapshot(bool) override
        {
            return {};
        }

        void clearSnapshot() override {}

        WalletAccountCreation createAccount(bool isPublic) override
        {
            ++createdAccounts;
            WalletAccountCreation creation;
            creation.accountId = QStringLiteral("fresh-lp");
            if (isPublic)
                creation.publicAccount = readPublicAccount(creation.accountId);
            return creation;
        }

        WalletAccountRead readPublicAccount(const QString& accountId) const override
        {
            WalletAccountRead read;
            read.accountId = accountId;
            read.status = QStringLiteral("ok");
            return read;
        }

        WalletSubmission submitPublicTransaction(const WalletTransaction& transaction) override
        {
            ++submissions;
            submitted = transaction;
            return { WalletFailure::None, transactionHash };
        }

        void disconnect() override {}

        QString transactionHash = QStringLiteral(
            "000102030405060708090a0b0c0d0e0f"
            "101112131415161718191a1b1c1d1e1f");
        int createdAccounts = 0;
        int submissions = 0;
        WalletTransaction submitted;
    };

    class FakeAmmClient final : public AmmClient {
    public:
        AmmClientResult configId(const QJsonObject&) const override
        {
            return success({ { QStringLiteral("configId"), QStringLiteral("config") } });
        }

        AmmClientResult tokenIds(const QJsonObject&) const override
        {
            return success({ { QStringLiteral("status"), QStringLiteral("ok") } });
        }

        AmmClientResult pairIds(const QJsonObject&) const override
        {
            return success({
                { QStringLiteral("status"), QStringLiteral("ok") },
                { QStringLiteral("tokenAId"), QStringLiteral("token-a") },
                { QStringLiteral("tokenBId"), QStringLiteral("token-b") },
                { QStringLiteral("poolId"), QStringLiteral("pool") },
                { QStringLiteral("vaultAId"), QStringLiteral("vault-a") },
                { QStringLiteral("vaultBId"), QStringLiteral("vault-b") },
                { QStringLiteral("lpDefinitionId"), QStringLiteral("lp") },
                { QStringLiteral("lpLockHoldingId"), QStringLiteral("lp-lock") },
                { QStringLiteral("currentTickId"), QStringLiteral("tick") },
                { QStringLiteral("clockId"), QStringLiteral("clock") },
            });
        }

        AmmClientResult context(const QJsonObject&) const override
        {
            return success({});
        }

        AmmClientResult quote(const QJsonObject&) const override
        {
            return success({
                { QStringLiteral("schema"), QStringLiteral("new-position.v1") },
                { QStringLiteral("status"), QStringLiteral("ok") },
                { QStringLiteral("canSubmit"), true },
                { QStringLiteral("quoteHash"), quoteHash },
                { QStringLiteral("requiresFreshLp"), requiresFreshLp },
            });
        }

        AmmClientResult plan(const QJsonObject& request) const override
        {
            sawFreshLp = request.contains(QStringLiteral("freshLp"));
            return success({
                { QStringLiteral("status"), QStringLiteral("ready") },
                { QStringLiteral("accountIds"), QJsonArray { QStringLiteral("account") } },
                { QStringLiteral("signingRequirements"), QJsonArray { true } },
                { QStringLiteral("instruction"), QJsonArray { 1 } },
                { QStringLiteral("programId"), QStringLiteral("program") },
                { QStringLiteral("deadlineMs"),
                  QString::number(QDateTime::currentMSecsSinceEpoch() + 60'000) },
            });
        }

        static AmmClientResult success(const QJsonObject& value)
        {
            return { true, value };
        }

        QString quoteHash = QStringLiteral("sha256:expected");
        bool requiresFreshLp = true;
        mutable bool sawFreshLp = false;
    };

    ActiveNetworkSnapshot readyNetwork()
    {
        return {
            QStringLiteral("testnet"),
            QStringLiteral("ready"),
            QStringLiteral("block10:identity"),
            QStringLiteral("program"),
            {},
        };
    }
}

int main()
{
    const QVariantMap request {
        { QStringLiteral("schema"), QStringLiteral("new-position.v1") },
        { QStringLiteral("tokenAId"), QStringLiteral("token-a") },
        { QStringLiteral("tokenBId"), QStringLiteral("token-b") },
        { QStringLiteral("feeBps"), 30 },
    };

    FakeWallet wallet;
    FakeAmmClient client;
    NewPositionRuntime runtime(&wallet, &client);
    const QVariantMap result = runtime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(result.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted"),
                "valid plan should submit"))
        return 1;
    if (!expect(result.value(QStringLiteral("transactionId")).toString()
                    == QStringLiteral("1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"),
                "native hash should become the expected base58 transaction ID"))
        return 1;
    if (!expect(wallet.createdAccounts == 1 && client.sawFreshLp,
                "fresh LP account should enter the plan"))
        return 1;
    if (!expect(wallet.submissions == 1
                    && wallet.submitted.accountIds
                        == QStringList { QStringLiteral("account") }
                    && wallet.submitted.signingRequirements.size() == 1
                    && wallet.submitted.signingRequirements.constFirst()
                    && wallet.submitted.instruction.size() == 1
                    && wallet.submitted.instruction.constFirst() == 1
                    && wallet.submitted.programId == QStringLiteral("program"),
                "runtime should dispatch the unchanged plan once"))
        return 1;

    FakeWallet orderedBytesWallet;
    orderedBytesWallet.transactionHash = QStringLiteral(
        "0102030405060708090a0b0c0d0e0f10"
        "1112131415161718191a1b1c1d1e1f20");
    FakeAmmClient orderedBytesClient;
    orderedBytesClient.requiresFreshLp = false;
    NewPositionRuntime orderedBytesRuntime(&orderedBytesWallet, &orderedBytesClient);
    const QVariantMap orderedBytes = orderedBytesRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(orderedBytes.value(QStringLiteral("transactionId")).toString()
                    == QStringLiteral("4wBqpZM9xaSheZzJSMawUKKwhdpChKbZ5eu5ky4Vigw"),
                "ordered native hash bytes should preserve byte order"))
        return 1;

    FakeWallet invalidHashWallet;
    invalidHashWallet.transactionHash = QStringLiteral("not-a-hash");
    FakeAmmClient invalidHashClient;
    invalidHashClient.requiresFreshLp = false;
    NewPositionRuntime invalidHashRuntime(&invalidHashWallet, &invalidHashClient);
    const QVariantMap invalidHash = invalidHashRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(invalidHash.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_submission_failed")
                    && !invalidHash.contains(QStringLiteral("transactionId")),
                "invalid native hash should fail without a hex fallback"))
        return 1;
    if (!expect(invalidHashWallet.submissions == 1,
                "hash conversion should happen after wallet submission"))
        return 1;

    FakeWallet staleWallet;
    FakeAmmClient staleClient;
    staleClient.quoteHash = QStringLiteral("sha256:changed");
    NewPositionRuntime staleRuntime(&staleWallet, &staleClient);
    const QVariantMap stale = staleRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(stale.value(QStringLiteral("code")).toString()
                    == QStringLiteral("quote_changed"),
                "changed quote should stop submission"))
        return 1;
    if (!expect(staleWallet.createdAccounts == 0 && staleWallet.submissions == 0,
                "stale quote should have no wallet side effects"))
        return 1;

    return 0;
}
