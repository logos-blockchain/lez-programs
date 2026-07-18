#include "AmmClient.h"
#include "NewPositionRuntime.h"
#include "SequencerClient.h"
#include "WalletProvider.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QHash>
#include <QHostAddress>
#include <QJsonDocument>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTemporaryFile>
#include <QTimer>

#include <functional>
#include <memory>
#include <utility>

namespace {
    bool expect(bool condition, const char* message)
    {
        if (!condition)
            qCritical("%s", message);
        return condition;
    }

    class LocalRpcServer final {
    public:
        LocalRpcServer()
        {
            QObject::connect(&m_server, &QTcpServer::newConnection, [&]() {
                while (m_server.hasPendingConnections()) {
                    QTcpSocket* socket = m_server.nextPendingConnection();
                    QObject::connect(socket, &QTcpSocket::readyRead, socket,
                                     [this, socket]() { process(socket); });
                    if (socket->bytesAvailable() > 0)
                        process(socket);
                }
            });
        }

        bool listen()
        {
            return m_server.listen(QHostAddress::LocalHost);
        }

        QString endpoint() const
        {
            return QStringLiteral("http://127.0.0.1:%1").arg(m_server.serverPort());
        }

        int requestCount() const { return m_requestCount; }
        QByteArray lastRequest() const
        {
            return m_lastRequest;
        }
        void failNextRequest() { ++m_failuresRemaining; }
        void holdResponses() { m_holdResponses = true; }
        void releaseNextResponse()
        {
            if (m_heldResponses.isEmpty())
                return;
            const HeldResponse held = m_heldResponses.takeFirst();
            respond(held.socket, held.fail);
        }

    private:
        struct HeldResponse {
            QTcpSocket* socket = nullptr;
            bool fail = false;
        };

        static void respond(QTcpSocket* socket, bool fail)
        {
            if (!socket)
                return;
            const QByteArray payload = QByteArrayLiteral(
                R"({"jsonrpc":"2.0","id":1,"result":null})");
            QByteArray response = fail
                ? QByteArrayLiteral(
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: ")
                : QByteArrayLiteral(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ");
            response += QByteArray::number(payload.size());
            response += QByteArrayLiteral("\r\nConnection: close\r\n\r\n");
            response += payload;
            socket->write(response);
            socket->disconnectFromHost();
        }

        void process(QTcpSocket* socket)
        {
            QByteArray& request = m_requests[socket];
            request.append(socket->readAll());
            const qsizetype headerEnd = request.indexOf("\r\n\r\n");
            if (headerEnd < 0)
                return;

            qsizetype contentLength = 0;
            for (QByteArray line : request.first(headerEnd).split('\n')) {
                line = line.trimmed();
                if (line.toLower().startsWith("content-length:")) {
                    contentLength = line.mid(sizeof("content-length:") - 1)
                                        .trimmed().toLongLong();
                }
            }
            const qsizetype bodyStart = headerEnd + 4;
            if (request.size() - bodyStart < contentLength)
                return;

            ++m_requestCount;
            m_lastRequest = request;
            const bool fail = m_failuresRemaining > 0;
            if (fail)
                --m_failuresRemaining;
            m_requests.remove(socket);
            if (m_holdResponses) {
                m_heldResponses.append({ socket, fail });
                return;
            }
            respond(socket, fail);
        }

        QTcpServer m_server;
        QHash<QTcpSocket*, QByteArray> m_requests;
        QByteArray m_lastRequest;
        int m_requestCount = 0;
        int m_failuresRemaining = 0;
        bool m_holdResponses = false;
        QVector<HeldResponse> m_heldResponses;
    };

    bool waitForRequestCount(const LocalRpcServer& server, int expected)
    {
        QElapsedTimer timer;
        timer.start();
        while (server.requestCount() < expected && timer.elapsed() < 3000)
            QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
        return server.requestCount() >= expected;
    }

    bool waitForCallbacks(const bool& first, const bool& second)
    {
        QElapsedTimer timer;
        timer.start();
        while ((!first || !second) && timer.elapsed() < 3000)
            QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
        return first && second;
    }

    bool waitForCondition(const std::function<bool()>& condition)
    {
        QElapsedTimer timer;
        timer.start();
        while (!condition() && timer.elapsed() < 3000)
            QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
        return condition();
    }

    class FakeWallet final : public WalletProvider {
    public:
        WalletSession connect(const WalletPaths&) override
        {
            return {};
        }

        void connectAsync(const WalletPaths&, SessionCallback callback) override
        {
            callback({});
        }

        WalletCreation createWallet(const WalletPaths&, const QString&) override
        {
            return {};
        }

        WalletSnapshot snapshot(bool) override
        {
            return {};
        }

        void snapshotAsync(bool, SnapshotCallback callback) override
        {
            callback({});
        }

        void clearSnapshot() override {}

        WalletAccountCreation createAccount(bool isPublic) override
        {
            ++createdAccounts;
            WalletAccountCreation creation;
            const QLatin1Char accountDigit(
                "abcdef"[(createdAccounts - 1) % 6]);
            creation.accountId = QString(64, accountDigit);
            if (isPublic && !failCreatedPublicRead)
                creation.publicAccount = readPublicAccount(creation.accountId);
            else
                creation.publicAccount.accountId = creation.accountId;
            return creation;
        }

        void createAccountAsync(bool isPublic, AccountCreationCallback callback) override
        {
            WalletAccountCreation creation = createAccount(isPublic);
            if (deferAccountCreation) {
                pendingAccountCreation = std::move(callback);
                pendingCreation = std::move(creation);
            } else {
                callback(std::move(creation));
            }
        }

        WalletAccountRead readPublicAccount(const QString& accountId) const override
        {
            WalletAccountRead read;
            read.accountId = accountId;
            read.status = QStringLiteral("ok");
            read.programOwner = QString(64, QLatin1Char('0'));
            read.balanceHex = readBalanceHex;
            read.nonceHex = QString(32, QLatin1Char('0'));
            return read;
        }

        void readPublicAccountsAsync(const QStringList& accountIds,
                                     AccountReadsCallback callback) override
        {
            QVector<WalletAccountRead> reads;
            reads.reserve(accountIds.size());
            for (const QString& accountId : accountIds)
                reads.append(readPublicAccount(accountId));
            callback(std::move(reads));
        }

        WalletSubmission submitPublicTransaction(const WalletTransaction& transaction) override
        {
            ++submissions;
            submitted = transaction;
            if (submissionFailuresRemaining > 0) {
                --submissionFailuresRemaining;
                return { WalletFailure::SubmissionFailed, {} };
            }
            return { WalletFailure::None, transactionHash };
        }

        void submitPublicTransactionAsync(
            const WalletTransaction& transaction, SubmissionCallback callback) override
        {
            WalletSubmission submission = submitPublicTransaction(transaction);
            if (deferSubmission) {
                pendingSubmissionCallback = std::move(callback);
                pendingSubmission = std::move(submission);
            } else {
                callback(std::move(submission));
            }
        }

        void finishAccountCreation()
        {
            AccountCreationCallback callback = std::move(pendingAccountCreation);
            if (callback)
                callback(std::move(pendingCreation));
        }

        void finishSubmission()
        {
            SubmissionCallback callback = std::move(pendingSubmissionCallback);
            if (callback)
                callback(std::move(pendingSubmission));
        }

        void disconnect() override {}

        QString transactionHash = QStringLiteral(
            "000102030405060708090a0b0c0d0e0f"
            "101112131415161718191a1b1c1d1e1f");
        int createdAccounts = 0;
        int submissions = 0;
        WalletTransaction submitted;
        bool deferAccountCreation = false;
        bool deferSubmission = false;
        bool failCreatedPublicRead = false;
        int submissionFailuresRemaining = 0;
        QString readBalanceHex = QString(32, QLatin1Char('0'));
        AccountCreationCallback pendingAccountCreation;
        SubmissionCallback pendingSubmissionCallback;
        WalletAccountCreation pendingCreation;
        WalletSubmission pendingSubmission;
    };

    class FakeAmmClient final : public AmmClient {
    public:
        AmmClientResult configId(const QJsonObject&) const override
        {
            return success({
                { QStringLiteral("configId"), QString(64, QLatin1Char('1')) },
            });
        }

        AmmClientResult tokenIds(const QJsonObject&) const override
        {
            ++tokenIdsCalls;
            return success({ { QStringLiteral("status"), QStringLiteral("ok") } });
        }

        AmmClientResult pairIds(const QJsonObject&) const override
        {
            ++pairIdsCalls;
            const QString tokenAId = useValidPairIds
                ? QString(64, QLatin1Char('2')) : QStringLiteral("token-a");
            const QString tokenBId = useValidPairIds
                ? QString(64, QLatin1Char('3')) : QStringLiteral("token-b");
            const QString poolId = useValidPairIds
                ? pairPoolId : QStringLiteral("pool");
            const QString vaultAId = useValidPairIds
                ? QString(64, QLatin1Char('4')) : QStringLiteral("vault-a");
            const QString vaultBId = useValidPairIds
                ? QString(64, QLatin1Char('5')) : QStringLiteral("vault-b");
            const QString lpDefinitionId = useValidPairIds
                ? QString(64, QLatin1Char('6')) : QStringLiteral("lp");
            const QString lpLockHoldingId = useValidPairIds
                ? QString(64, QLatin1Char('7')) : QStringLiteral("lp-lock");
            const QString currentTickId = useValidPairIds
                ? QString(64, QLatin1Char('8')) : QStringLiteral("tick");
            const QString clockId = useValidPairIds
                ? QString(64, QLatin1Char('9')) : QStringLiteral("clock");
            return success({
                { QStringLiteral("status"), QStringLiteral("ok") },
                { QStringLiteral("tokenAId"), tokenAId },
                { QStringLiteral("tokenBId"), tokenBId },
                { QStringLiteral("poolId"), poolId },
                { QStringLiteral("vaultAId"), vaultAId },
                { QStringLiteral("vaultBId"), vaultBId },
                { QStringLiteral("lpDefinitionId"), lpDefinitionId },
                { QStringLiteral("lpLockHoldingId"), lpLockHoldingId },
                { QStringLiteral("currentTickId"), currentTickId },
                { QStringLiteral("clockId"), clockId },
            });
        }

        AmmClientResult context(const QJsonObject&) const override
        {
            ++contextCalls;
            return success({});
        }

        AmmClientResult quote(const QJsonObject&) const override
        {
            ++quoteCalls;
            return success({
                { QStringLiteral("schema"), QStringLiteral("new-position.v2") },
                { QStringLiteral("status"), QStringLiteral("ok") },
                { QStringLiteral("canSubmit"), true },
                { QStringLiteral("poolStatus"), quotePoolStatus },
                { QStringLiteral("quoteHash"), quoteHash },
                { QStringLiteral("requiresFreshLp"), requiresFreshLp },
            });
        }

        AmmClientResult plan(const QJsonObject& request) const override
        {
            sawFreshLp = request.contains(QStringLiteral("freshLp"));
            if (sawFreshLp) {
                freshLpAccountIds.append(
                    request.value(QStringLiteral("freshLp")).toObject()
                        .value(QStringLiteral("id")).toString());
            }
            ++planCalls;
            if (planFailuresRemaining > 0) {
                --planFailuresRemaining;
                return { false, {} };
            }
            return success({
                { QStringLiteral("status"), QStringLiteral("ready") },
                { QStringLiteral("accountIds"), QJsonArray { QStringLiteral("account") } },
                { QStringLiteral("affectedAccountIds"),
                  QJsonArray { QStringLiteral("account") } },
                { QStringLiteral("contextAffectedAccountIds"),
                  QJsonArray { QStringLiteral("account") } },
                { QStringLiteral("signingRequirements"), QJsonArray { true } },
                { QStringLiteral("instruction"), QJsonArray { 1 } },
                { QStringLiteral("programId"), QStringLiteral("program") },
                { QStringLiteral("deadlineMs"),
                  QString::number(QDateTime::currentMSecsSinceEpoch() + 60'000) },
            });
        }

        AmmClientResult normalizeAccountRpc(const QJsonObject& request) const override
        {
            normalizedAccountIds.append(
                request.value(QStringLiteral("accountId")).toString());
            return success({
                { QStringLiteral("id"),
                  request.value(QStringLiteral("accountId")).toString() },
                { QStringLiteral("status"), QStringLiteral("ok") },
                { QStringLiteral("account"), QJsonObject {
                    { QStringLiteral("program_owner"),
                      QString(64, QLatin1Char('0')) },
                    { QStringLiteral("balance"),
                      normalizedBalanceHex },
                    { QStringLiteral("nonce"),
                      QString(32, QLatin1Char('0')) },
                    { QStringLiteral("data"), QString() },
                } },
            });
        }

        static AmmClientResult success(const QJsonObject& value)
        {
            return { true, value };
        }

        QString quoteHash = QStringLiteral("sha256:expected");
        QString quotePoolStatus = QStringLiteral("active_pool");
        bool requiresFreshLp = true;
        mutable bool sawFreshLp = false;
        mutable int tokenIdsCalls = 0;
        mutable int contextCalls = 0;
        mutable int pairIdsCalls = 0;
        mutable int quoteCalls = 0;
        mutable int planCalls = 0;
        mutable int planFailuresRemaining = 0;
        mutable QStringList freshLpAccountIds;
        mutable QStringList normalizedAccountIds;
        QString normalizedBalanceHex = QString(32, QLatin1Char('0'));
        bool useValidPairIds = false;
        QString pairPoolId;
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

    bool waitForContext(NewPositionRuntime& runtime, bool forceRefresh)
    {
        bool completed = false;
        QEventLoop loop;
        QTimer timeout;
        timeout.setSingleShot(true);
        QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);
        runtime.contextAsync({}, readyNetwork(), true, forceRefresh,
            [&](QVariantMap) {
                completed = true;
                loop.quit();
            });
        timeout.start(3000);
        if (!completed)
            loop.exec();
        return completed;
    }

    bool waitForAccounts(SequencerClient& sequencer,
                         const QStringList& accountIds,
                         bool forceRefresh,
                         QVector<WalletAccountRead>* reads)
    {
        bool completed = false;
        QEventLoop loop;
        QTimer timeout;
        timeout.setSingleShot(true);
        QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);
        sequencer.readAccounts(accountIds, forceRefresh,
            [&](QVector<WalletAccountRead> result) {
                *reads = std::move(result);
                completed = true;
                loop.quit();
            });
        timeout.start(3000);
        if (!completed)
            loop.exec();
        return completed;
    }
}

int main(int argc, char** argv)
{
    QCoreApplication application(argc, argv);
    const QVariantMap request {
        { QStringLiteral("schema"), QStringLiteral("new-position.v2") },
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
    if (!expect(result.value(QStringLiteral("nativeTransactionHash")).toString()
                    == wallet.transactionHash,
                "submitted result should preserve the native hash for polling"))
        return 1;
    if (!expect(result.value(QStringLiteral("contextAffectedAccountIds")).toList()
                    == QVariantList { QStringLiteral("account") },
                "submitted result should preserve context refresh account IDs"))
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

    FakeWallet planRetryWallet;
    FakeAmmClient planRetryClient;
    planRetryClient.planFailuresRemaining = 1;
    NewPositionRuntime planRetryRuntime(&planRetryWallet, &planRetryClient);
    const QVariantMap failedPlan = planRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    const QVariantMap retriedPlan = planRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(failedPlan.value(QStringLiteral("code")).toString()
                    == QStringLiteral("backend_error")
                    && retriedPlan.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted"),
                "plan failure should remain retryable"))
        return 1;
    if (!expect(planRetryWallet.createdAccounts == 1
                    && planRetryClient.freshLpAccountIds.size() == 2
                    && planRetryClient.freshLpAccountIds.constFirst()
                        == planRetryClient.freshLpAccountIds.constLast(),
                "plan retry should reuse the pending LP account"))
        return 1;

    FakeWallet submissionRetryWallet;
    submissionRetryWallet.submissionFailuresRemaining = 1;
    FakeAmmClient submissionRetryClient;
    NewPositionRuntime submissionRetryRuntime(
        &submissionRetryWallet, &submissionRetryClient);
    const QVariantMap failedSubmission = submissionRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    const QVariantMap retriedSubmission = submissionRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(failedSubmission.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_submission_failed")
                    && retriedSubmission.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted"),
                "wallet rejection should remain retryable"))
        return 1;
    if (!expect(submissionRetryWallet.createdAccounts == 1
                    && submissionRetryWallet.submissions == 2
                    && submissionRetryClient.freshLpAccountIds.size() == 2
                    && submissionRetryClient.freshLpAccountIds.constFirst()
                        == submissionRetryClient.freshLpAccountIds.constLast(),
                "wallet retry should reuse the pending LP account"))
        return 1;

    FakeWallet consumedWallet;
    FakeAmmClient consumedClient;
    NewPositionRuntime consumedRuntime(&consumedWallet, &consumedClient);
    const QVariantMap firstConsumed = consumedRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    const QVariantMap secondConsumed = consumedRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(firstConsumed.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && secondConsumed.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted")
                    && consumedWallet.createdAccounts == 2
                    && consumedClient.freshLpAccountIds.size() == 2
                    && consumedClient.freshLpAccountIds.constFirst()
                        != consumedClient.freshLpAccountIds.constLast(),
                "accepted submission should consume the LP account reservation"))
        return 1;

    FakeWallet changedWallet;
    FakeAmmClient changedClient;
    changedClient.planFailuresRemaining = 1;
    NewPositionRuntime changedRuntime(&changedWallet, &changedClient);
    const QVariantMap beforeWalletChange = changedRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    changedRuntime.clearWalletAccounts();
    const QVariantMap afterWalletChange = changedRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(beforeWalletChange.value(QStringLiteral("code")).toString()
                    == QStringLiteral("backend_error")
                    && afterWalletChange.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted")
                    && changedWallet.createdAccounts == 2,
                "wallet change should discard the previous LP reservation"))
        return 1;

    FakeWallet readRetryWallet;
    readRetryWallet.failCreatedPublicRead = true;
    FakeAmmClient readRetryClient;
    NewPositionRuntime readRetryRuntime(&readRetryWallet, &readRetryClient);
    const QVariantMap failedCreatedRead = readRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    readRetryWallet.failCreatedPublicRead = false;
    const QVariantMap retriedCreatedRead = readRetryRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(failedCreatedRead.value(QStringLiteral("code")).toString()
                    == QStringLiteral("account_read_failed")
                    && retriedCreatedRead.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted")
                    && readRetryWallet.createdAccounts == 1,
                "created-account read failure should preserve the LP reservation"))
        return 1;

    FakeWallet nonDefaultWallet;
    FakeAmmClient nonDefaultClient;
    nonDefaultClient.planFailuresRemaining = 1;
    NewPositionRuntime nonDefaultRuntime(&nonDefaultWallet, &nonDefaultClient);
    const QVariantMap reservedAccount = nonDefaultRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    nonDefaultWallet.readBalanceHex = QString(31, QLatin1Char('0'))
        + QLatin1Char('1');
    const QVariantMap nonDefaultAccount = nonDefaultRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    nonDefaultWallet.readBalanceHex = QString(32, QLatin1Char('0'));
    const QVariantMap replacementAccount = nonDefaultRuntime.submit(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true);
    if (!expect(reservedAccount.value(QStringLiteral("code")).toString()
                    == QStringLiteral("backend_error")
                    && nonDefaultAccount.value(QStringLiteral("code")).toString()
                        == QStringLiteral("submission_status_unknown")
                    && replacementAccount.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted")
                    && nonDefaultWallet.createdAccounts == 2,
                "non-default LP reservation should be replaced"))
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

    LocalRpcServer configuredEndpointServer;
    LocalRpcServer adoptedEndpointServer;
    if (!expect(configuredEndpointServer.listen()
                    && adoptedEndpointServer.listen(),
                "endpoint-alignment sequencers should listen"))
        return 1;
    QTemporaryFile endpointConfig;
    if (!expect(endpointConfig.open(), "endpoint-alignment config should open"))
        return 1;
    endpointConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), configuredEndpointServer.endpoint() },
        { QStringLiteral("basic_auth"), QJsonObject {
            { QStringLiteral("username"), QStringLiteral("user") },
            { QStringLiteral("password"), QStringLiteral("pass") },
        } },
    }).toJson(QJsonDocument::Compact));
    endpointConfig.flush();
    FakeAmmClient endpointClient;
    SequencerClient alignedSequencer(&endpointClient);
    if (!expect(alignedSequencer.configure(
                    endpointConfig.fileName(), adoptedEndpointServer.endpoint()),
                "adopted wallet endpoint should configure"))
        return 1;
    if (!expect(alignedSequencer.endpoint() == adoptedEndpointServer.endpoint(),
                "adopted wallet endpoint should override stale config"))
        return 1;
    QVector<WalletAccountRead> adoptedEndpointReads;
    const QString endpointAccount(64, QLatin1Char('3'));
    if (!expect(waitForAccounts(
                    alignedSequencer, { endpointAccount }, false,
                    &adoptedEndpointReads),
                "adopted endpoint read should complete"))
        return 1;
    if (!expect(configuredEndpointServer.requestCount() == 0
                    && adoptedEndpointServer.requestCount() == 1,
                "direct reads should follow adopted wallet endpoint"))
        return 1;
    if (!expect(!adoptedEndpointServer.lastRequest().toLower().contains(
                    "authorization:"),
                "stale config credentials must not reach adopted endpoint"))
        return 1;

    if (!expect(alignedSequencer.configure(
                    endpointConfig.fileName(), configuredEndpointServer.endpoint()),
                "matching wallet endpoint should configure"))
        return 1;
    QVector<WalletAccountRead> configuredEndpointReads;
    if (!expect(waitForAccounts(
                    alignedSequencer, { endpointAccount }, false,
                    &configuredEndpointReads),
                "matching endpoint read should complete"))
        return 1;
    if (!expect(configuredEndpointServer.requestCount() == 1
                    && configuredEndpointServer.lastRequest().toLower().contains(
                        "authorization: basic dxnlcjpwyxnz"),
                "matching endpoint should retain configured credentials"))
        return 1;

    LocalRpcServer staleContextServer;
    if (!expect(staleContextServer.listen(),
                "stale-context sequencer should listen"))
        return 1;
    staleContextServer.holdResponses();
    QTemporaryFile staleContextConfig;
    if (!expect(staleContextConfig.open(),
                "stale-context config should open"))
        return 1;
    staleContextConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), staleContextServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    staleContextConfig.flush();
    FakeWallet staleContextWallet;
    FakeAmmClient staleContextClient;
    SequencerClient staleContextSequencer(&staleContextClient);
    if (!expect(staleContextSequencer.configure(staleContextConfig.fileName()),
                "stale-context sequencer should configure"))
        return 1;
    NewPositionRuntime staleContextRuntime(
        &staleContextWallet, &staleContextClient, &staleContextSequencer);
    WalletAccount staleContextHolding;
    staleContextHolding.address = QString(64, QLatin1Char('4'));
    staleContextHolding.isPublic = true;
    staleContextRuntime.setWalletAccounts({ staleContextHolding });
    int staleContextCallbacks = 0;
    int latestContextCallbacks = 0;
    staleContextRuntime.contextAsync(
        {}, readyNetwork(), true, false,
        [&](QVariantMap) { ++staleContextCallbacks; });
    if (!expect(waitForRequestCount(staleContextServer, 1),
                "first context should begin the config read"))
        return 1;
    staleContextRuntime.contextAsync(
        {}, readyNetwork(), true, false,
        [&](QVariantMap) { ++latestContextCallbacks; });
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(staleContextServer.requestCount() == 1,
                "superseding context should share the active config read"))
        return 1;
    staleContextServer.releaseNextResponse();
    if (!expect(waitForRequestCount(staleContextServer, 2),
                "latest context should continue with the wallet read"))
        return 1;
    if (!expect(staleContextCallbacks == 0
                    && latestContextCallbacks == 0
                    && staleContextClient.tokenIdsCalls == 0
                    && staleContextClient.contextCalls == 0,
                "context should wait for the latest wallet read"))
        return 1;
    staleContextServer.releaseNextResponse();
    if (!expect(waitForCondition([&]() { return latestContextCallbacks == 1; }),
                "latest context should complete"))
        return 1;
    if (!expect(staleContextCallbacks == 0
                    && staleContextClient.tokenIdsCalls == 1
                    && staleContextClient.contextCalls == 1
                    && staleContextServer.requestCount() == 2,
                "superseded context should stop before downstream work"))
        return 1;

    LocalRpcServer staleQuoteServer;
    if (!expect(staleQuoteServer.listen(),
                "stale-quote sequencer should listen"))
        return 1;
    staleQuoteServer.holdResponses();
    QTemporaryFile staleQuoteConfig;
    if (!expect(staleQuoteConfig.open(),
                "stale-quote config should open"))
        return 1;
    staleQuoteConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), staleQuoteServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    staleQuoteConfig.flush();
    FakeWallet staleQuoteWallet;
    FakeAmmClient staleQuoteClient;
    SequencerClient staleQuoteSequencer(&staleQuoteClient);
    if (!expect(staleQuoteSequencer.configure(staleQuoteConfig.fileName()),
                "stale-quote sequencer should configure"))
        return 1;
    NewPositionRuntime staleQuoteRuntime(
        &staleQuoteWallet, &staleQuoteClient, &staleQuoteSequencer);
    int staleQuoteCallbacks = 0;
    int latestQuoteCallbacks = 0;
    staleQuoteRuntime.quoteAsync(
        request, readyNetwork(), true, false, false,
        [&](QVariantMap) { ++staleQuoteCallbacks; });
    if (!expect(waitForRequestCount(staleQuoteServer, 1),
                "first quote should begin the config read"))
        return 1;
    staleQuoteRuntime.quoteAsync(
        request, readyNetwork(), true, false, false,
        [&](QVariantMap) { ++latestQuoteCallbacks; });
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(staleQuoteServer.requestCount() == 1,
                "superseding quote should share the active config read"))
        return 1;
    staleQuoteServer.releaseNextResponse();
    if (!expect(waitForCondition([&]() { return latestQuoteCallbacks == 1; }),
                "latest quote should complete"))
        return 1;
    if (!expect(staleQuoteCallbacks == 0
                    && staleQuoteClient.pairIdsCalls == 1
                    && staleQuoteClient.quoteCalls == 1,
                "superseded user quote should stop before pair work"))
        return 1;

    int poolProbeCallbacks = 0;
    int concurrentUserCallbacks = 0;
    staleQuoteRuntime.quoteAsync(
        request, readyNetwork(), true, false, true,
        [&](QVariantMap) { ++poolProbeCallbacks; });
    staleQuoteRuntime.quoteAsync(
        request, readyNetwork(), true, false, false,
        [&](QVariantMap) { ++concurrentUserCallbacks; });
    if (!expect(waitForCondition([&]() {
                    return poolProbeCallbacks == 1
                        && concurrentUserCallbacks == 1;
                }),
                "pool probe and user quote should complete independently"))
        return 1;
    if (!expect(staleQuoteClient.pairIdsCalls == 3
                    && staleQuoteClient.quoteCalls == 3,
                "user quote cancellation must not cancel the pool probe"))
        return 1;

    LocalRpcServer poolProbeServer;
    if (!expect(poolProbeServer.listen(),
                "pool-probe sequencer should listen"))
        return 1;
    QTemporaryFile poolProbeConfig;
    if (!expect(poolProbeConfig.open(),
                "pool-probe config should open"))
        return 1;
    poolProbeConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), poolProbeServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    poolProbeConfig.flush();
    FakeWallet poolProbeWallet;
    FakeAmmClient poolProbeClient;
    SequencerClient poolProbeSequencer(&poolProbeClient);
    if (!expect(poolProbeSequencer.configure(poolProbeConfig.fileName()),
                "pool-probe sequencer should configure"))
        return 1;
    NewPositionRuntime poolProbeRuntime(
        &poolProbeWallet, &poolProbeClient, &poolProbeSequencer);
    const QString poolIdBase58 = QStringLiteral(
        "1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE");
    const QString poolIdHex = QStringLiteral(
        "000102030405060708090a0b0c0d0e0f"
        "101112131415161718191a1b1c1d1e1f");
    QVariantMap poolProbeRequest = request;
    poolProbeRequest.insert(QStringLiteral("poolId"), poolIdBase58);
    QVariantMap poolProbeResult;
    poolProbeRuntime.quoteAsync(
        poolProbeRequest, readyNetwork(), true, true, true,
        [&](QVariantMap result) { poolProbeResult = std::move(result); });
    if (!expect(waitForCondition([&]() { return !poolProbeResult.isEmpty(); }),
                "default pool probe should complete"))
        return 1;
    if (!expect(poolProbeResult.value(QStringLiteral("poolStatus")).toString()
                        == QStringLiteral("missing_pool")
                    && poolProbeServer.requestCount() == 1
                    && poolProbeClient.pairIdsCalls == 0
                    && poolProbeClient.quoteCalls == 0
                    && poolProbeClient.normalizedAccountIds.contains(poolIdHex),
                "default pool probe should read only the pool account"))
        return 1;

    poolProbeClient.normalizedBalanceHex = QString(32, QLatin1Char('1'));
    poolProbeClient.useValidPairIds = true;
    poolProbeClient.pairPoolId = poolIdHex;
    const int requestsBeforeActiveProbe = poolProbeServer.requestCount();
    const int poolReadsBeforeActiveProbe =
        poolProbeClient.normalizedAccountIds.count(poolIdHex);
    poolProbeResult.clear();
    poolProbeRuntime.quoteAsync(
        poolProbeRequest, readyNetwork(), true, true, true,
        [&](QVariantMap result) { poolProbeResult = std::move(result); });
    if (!expect(waitForCondition([&]() { return !poolProbeResult.isEmpty(); }),
                "nondefault pool probe should complete"))
        return 1;
    if (!expect(poolProbeResult.value(QStringLiteral("poolStatus")).toString()
                        == QStringLiteral("active_pool")
                    && poolProbeServer.requestCount() == requestsBeforeActiveProbe + 10
                    && poolProbeClient.normalizedAccountIds.count(poolIdHex)
                        == poolReadsBeforeActiveProbe + 1
                    && poolProbeClient.pairIdsCalls == 1
                    && poolProbeClient.quoteCalls == 1,
                "active pool probe should reuse its fresh pool read"))
        return 1;

    const QString mismatchedPoolId = QString(64, QLatin1Char('d'));
    poolProbeClient.pairPoolId = mismatchedPoolId;
    const int requestsBeforeMismatch = poolProbeServer.requestCount();
    const int sourcePoolReadsBeforeMismatch =
        poolProbeClient.normalizedAccountIds.count(poolIdHex);
    const int pairPoolReadsBeforeMismatch =
        poolProbeClient.normalizedAccountIds.count(mismatchedPoolId);
    poolProbeResult.clear();
    poolProbeRuntime.quoteAsync(
        poolProbeRequest, readyNetwork(), true, true, true,
        [&](QVariantMap result) { poolProbeResult = std::move(result); });
    if (!expect(waitForCondition([&]() { return !poolProbeResult.isEmpty(); }),
                "mismatched pool probe should complete"))
        return 1;
    if (!expect(poolProbeResult.value(QStringLiteral("poolStatus")).toString()
                        == QStringLiteral("active_pool")
                    && poolProbeServer.requestCount() == requestsBeforeMismatch + 11
                    && poolProbeClient.normalizedAccountIds.count(poolIdHex)
                        == sourcePoolReadsBeforeMismatch + 1
                    && poolProbeClient.normalizedAccountIds.count(mismatchedPoolId)
                        == pairPoolReadsBeforeMismatch + 1
                    && poolProbeClient.pairIdsCalls == 2
                    && poolProbeClient.quoteCalls == 2,
                "pool mismatch should retain the full forced quote"))
        return 1;

    const int requestsBeforeFailedProbe = poolProbeServer.requestCount();
    poolProbeServer.failNextRequest();
    poolProbeResult.clear();
    poolProbeRuntime.quoteAsync(
        poolProbeRequest, readyNetwork(), true, true, true,
        [&](QVariantMap result) { poolProbeResult = std::move(result); });
    if (!expect(waitForCondition([&]() { return !poolProbeResult.isEmpty(); }),
                "failed pool probe should complete"))
        return 1;
    if (!expect(poolProbeResult.value(QStringLiteral("code")).toString()
                        == QStringLiteral("account_read_failed")
                    && poolProbeServer.requestCount() == requestsBeforeFailedProbe + 1
                    && poolProbeClient.pairIdsCalls == 2
                    && poolProbeClient.quoteCalls == 2,
                "failed pool probe should wait for the next one-account retry"))
        return 1;

    LocalRpcServer server;
    if (!expect(server.listen(), "local sequencer should listen"))
        return 1;
    QTemporaryFile sequencerConfig;
    if (!expect(sequencerConfig.open(), "sequencer config should open"))
        return 1;
    sequencerConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), server.endpoint() },
    }).toJson(QJsonDocument::Compact));
    sequencerConfig.flush();

    FakeWallet refreshWallet;
    FakeAmmClient refreshClient;
    SequencerClient sequencer(&refreshClient);
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "sequencer client should configure"))
        return 1;
    NewPositionRuntime refreshRuntime(&refreshWallet, &refreshClient, &sequencer);
    WalletAccount holding;
    holding.address = QString(64, QLatin1Char('2'));
    holding.isPublic = true;
    refreshRuntime.setWalletAccounts({ holding });

    if (!expect(waitForContext(refreshRuntime, false),
                "initial context should complete"))
        return 1;
    if (!expect(server.requestCount() == 2,
                "initial context should read config and wallet holding"))
        return 1;
    if (!expect(waitForContext(refreshRuntime, false),
                "cached context should complete"))
        return 1;
    if (!expect(server.requestCount() == 2,
                "cached context should not reread accounts"))
        return 1;
    if (!expect(waitForContext(refreshRuntime, true),
                "forced context should complete"))
        return 1;
    if (!expect(server.requestCount() == 4,
                "forced context should reread config and wallet holding"))
        return 1;

    FakeWallet selectedWallet;
    NewPositionRuntime selectedRuntime(
        &selectedWallet, &refreshClient, &sequencer);
    const QString selectedAccountHex = QStringLiteral(
        "000102030405060708090a0b0c0d0e0f"
        "101112131415161718191a1b1c1d1e1f");
    WalletAccount selectedAccount;
    selectedAccount.address = selectedAccountHex;
    selectedAccount.isPublic = true;
    selectedRuntime.setWalletAccounts({ selectedAccount });
    QVariantMap selectedRequest = request;
    selectedRequest.insert(
        QStringLiteral("holdingAId"),
        QStringLiteral("1thX6LZfHDZZKUs92febYZhYRcXddmzfzF2NvTkPNE"));
    const int requestsBeforeSelectedQuote = server.requestCount();
    const int normalizationsBeforeSelectedQuote =
        refreshClient.normalizedAccountIds.count(selectedAccountHex);
    QVariantMap selectedQuote;
    selectedRuntime.quoteAsync(
        selectedRequest, readyNetwork(), true, false, false,
        [&](QVariantMap result) { selectedQuote = std::move(result); });
    if (!expect(waitForCondition([&]() { return !selectedQuote.isEmpty(); }),
                "selected-holding quote should complete"))
        return 1;
    if (!expect(server.requestCount() == requestsBeforeSelectedQuote + 1
                    && refreshClient.normalizedAccountIds.count(selectedAccountHex)
                        == normalizationsBeforeSelectedQuote + 1,
                "selected holding should be read once per cold quote"))
        return 1;

    server.failNextRequest();
    QVector<WalletAccountRead> failedRefresh;
    if (!expect(waitForAccounts(sequencer, { holding.address }, true,
                                &failedRefresh),
                "failed forced holding refresh should complete"))
        return 1;
    if (!expect(failedRefresh.size() == 1 && !failedRefresh.constFirst().ok(),
                "forced holding refresh should surface the sequencer failure"))
        return 1;
    const int requestsAfterFailure = server.requestCount();
    QVector<WalletAccountRead> recoveredRead;
    if (!expect(waitForAccounts(sequencer, { holding.address }, false,
                                &recoveredRead),
                "holding read after failure should complete"))
        return 1;
    if (!expect(server.requestCount() == requestsAfterFailure + 1,
                "failed forced refresh should evict the stale holding cache"))
        return 1;
    if (!expect(recoveredRead.size() == 1 && recoveredRead.constFirst().ok(),
                "holding read should recover from the sequencer"))
        return 1;

    const int requestsBeforeUnchangedConfig = server.requestCount();
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "unchanged sequencer config should remain valid"))
        return 1;
    QVector<WalletAccountRead> unchangedConfigRead;
    if (!expect(waitForAccounts(sequencer, { holding.address }, false,
                                &unchangedConfigRead),
                "cached read after unchanged config should complete"))
        return 1;
    if (!expect(server.requestCount() == requestsBeforeUnchangedConfig,
                "unchanged config should preserve the account cache"))
        return 1;

    LocalRpcServer replacementServer;
    if (!expect(replacementServer.listen(),
                "replacement sequencer should listen"))
        return 1;
    if (!expect(sequencerConfig.resize(0) && sequencerConfig.seek(0),
                "sequencer config should rewind"))
        return 1;
    sequencerConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), replacementServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    sequencerConfig.flush();
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "same-path endpoint update should configure"))
        return 1;
    QVector<WalletAccountRead> replacementRead;
    if (!expect(waitForAccounts(sequencer, { holding.address }, false,
                                &replacementRead),
                "same-path endpoint read should complete"))
        return 1;
    if (!expect(replacementServer.requestCount() == 1,
                "same-path endpoint update should clear cache and use new sequencer"))
        return 1;

    LocalRpcServer forcedRefreshServer;
    if (!expect(forcedRefreshServer.listen(),
                "forced-refresh sequencer should listen"))
        return 1;
    forcedRefreshServer.holdResponses();
    bool staleCachedCompleted = false;
    QVector<WalletAccountRead> staleCachedRead;
    sequencer.readAccounts({ holding.address }, false,
        [&](QVector<WalletAccountRead> reads) {
            staleCachedRead = std::move(reads);
            staleCachedCompleted = true;
        });
    if (!expect(sequencerConfig.resize(0) && sequencerConfig.seek(0),
                "forced-refresh config should rewind"))
        return 1;
    sequencerConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), forcedRefreshServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    sequencerConfig.flush();
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "forced-refresh sequencer should configure"))
        return 1;
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(staleCachedCompleted
                    && staleCachedRead.size() == 1
                    && !staleCachedRead.constFirst().ok(),
                "cached read should fail after sequencer reconfiguration"))
        return 1;

    bool ordinaryCompleted = false;
    bool forcedCompleted = false;
    sequencer.readAccounts({ holding.address }, false,
        [&](QVector<WalletAccountRead>) { ordinaryCompleted = true; });
    if (!expect(waitForRequestCount(forcedRefreshServer, 1),
                "ordinary read should reach sequencer"))
        return 1;
    sequencer.readAccounts({ holding.address }, true,
        [&](QVector<WalletAccountRead>) { forcedCompleted = true; });
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(forcedRefreshServer.requestCount() == 1,
                "forced refresh should wait for active account read"))
        return 1;

    forcedRefreshServer.releaseNextResponse();
    if (!expect(waitForRequestCount(forcedRefreshServer, 2),
                "forced refresh should issue a follow-up account read"))
        return 1;
    if (!expect(ordinaryCompleted && !forcedCompleted,
                "pre-refresh result must not complete forced callback"))
        return 1;
    forcedRefreshServer.releaseNextResponse();
    if (!expect(waitForCallbacks(ordinaryCompleted, forcedCompleted),
                "forced follow-up read should complete"))
        return 1;

    LocalRpcServer cancelledSubmitServer;
    if (!expect(cancelledSubmitServer.listen(),
                "cancelled-submit sequencer should listen"))
        return 1;
    cancelledSubmitServer.holdResponses();
    if (!expect(sequencerConfig.resize(0) && sequencerConfig.seek(0),
                "cancelled-submit config should rewind"))
        return 1;
    sequencerConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), cancelledSubmitServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    sequencerConfig.flush();
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "cancelled-submit sequencer should configure"))
        return 1;

    FakeWallet cancelledWallet;
    FakeAmmClient cancelledClient;
    NewPositionRuntime cancelledRuntime(
        &cancelledWallet, &cancelledClient, &sequencer);
    bool cancelledCompleted = false;
    QVariantMap cancelledResult;
    cancelledRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            cancelledResult = std::move(result);
            cancelledCompleted = true;
        });
    if (!expect(waitForRequestCount(cancelledSubmitServer, 1),
                "submit should begin account reads"))
        return 1;
    cancelledRuntime.clearWalletAccounts();
    cancelledRuntime.setWalletAccounts({});
    cancelledSubmitServer.releaseNextResponse();
    if (!expect(waitForCallbacks(cancelledCompleted, cancelledCompleted),
                "cancelled submit should complete"))
        return 1;
    if (!expect(cancelledResult.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_unavailable")
                    && cancelledWallet.createdAccounts == 0
                    && cancelledWallet.submissions == 0,
                "wallet change should cancel submit before side effects"))
        return 1;

    LocalRpcServer asyncSubmitServer;
    if (!expect(asyncSubmitServer.listen(), "async-submit sequencer should listen"))
        return 1;
    if (!expect(sequencerConfig.resize(0) && sequencerConfig.seek(0),
                "async-submit config should rewind"))
        return 1;
    sequencerConfig.write(QJsonDocument(QJsonObject {
        { QStringLiteral("sequencer_addr"), asyncSubmitServer.endpoint() },
    }).toJson(QJsonDocument::Compact));
    sequencerConfig.flush();
    if (!expect(sequencer.configure(sequencerConfig.fileName()),
                "async-submit sequencer should configure"))
        return 1;

    FakeWallet asyncWallet;
    asyncWallet.deferAccountCreation = true;
    asyncWallet.deferSubmission = true;
    FakeAmmClient asyncClient;
    NewPositionRuntime asyncRuntime(&asyncWallet, &asyncClient, &sequencer);
    int asyncCallbackCount = 0;
    QVariantMap asyncResult;
    asyncRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            ++asyncCallbackCount;
            asyncResult = std::move(result);
        });
    if (!expect(waitForCondition([&]() { return asyncWallet.createdAccounts == 1; }),
                "submit should asynchronously request a fresh account"))
        return 1;
    if (!expect(asyncCallbackCount == 0 && asyncWallet.submissions == 0,
                "account creation should not block or finish submission"))
        return 1;

    QVariantMap concurrentResult;
    asyncRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) { concurrentResult = std::move(result); });
    if (!expect(concurrentResult.value(QStringLiteral("code")).toString()
                    == QStringLiteral("submit_in_progress"),
                "concurrent submit should fail while async mutation is pending"))
        return 1;

    asyncWallet.finishAccountCreation();
    if (!expect(waitForCondition([&]() { return asyncWallet.submissions == 1; }),
                "fresh account completion should dispatch transaction"))
        return 1;
    if (!expect(asyncCallbackCount == 0,
                "transaction submission should remain asynchronous"))
        return 1;
    asyncWallet.finishSubmission();
    if (!expect(asyncCallbackCount == 1
                    && asyncResult.value(QStringLiteral("status")).toString()
                        == QStringLiteral("submitted")
                    && asyncResult.value(QStringLiteral("contextAffectedAccountIds"))
                        .toList() == QVariantList { QStringLiteral("account") },
                "async wallet completion should finish exactly once"))
        return 1;

    FakeWallet cancelledCreationWallet;
    cancelledCreationWallet.deferAccountCreation = true;
    FakeAmmClient cancelledCreationClient;
    NewPositionRuntime cancelledCreationRuntime(
        &cancelledCreationWallet, &cancelledCreationClient, &sequencer);
    QVariantMap cancelledCreationResult;
    cancelledCreationRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            cancelledCreationResult = std::move(result);
        });
    if (!expect(waitForCondition(
                    [&]() { return cancelledCreationWallet.createdAccounts == 1; }),
                "cancelled creation should reach the wallet"))
        return 1;
    WalletAccount creationDiscoveredAccount;
    creationDiscoveredAccount.address = QString(64, QLatin1Char('8'));
    creationDiscoveredAccount.isPublic = true;
    cancelledCreationRuntime.setWalletAccounts({ creationDiscoveredAccount });
    QVariantMap blockedCreationRetry;
    cancelledCreationRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            blockedCreationRetry = std::move(result);
        });
    if (!expect(cancelledCreationResult.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_unavailable")
                    && blockedCreationRetry.value(QStringLiteral("code")).toString()
                        == QStringLiteral("submit_in_progress"),
                "account refresh should wait for active creation to settle"))
        return 1;
    cancelledCreationWallet.finishAccountCreation();
    const int creationReadsBeforeRetry = refreshClient.normalizedAccountIds.count(
        QString(64, QLatin1Char('a')));
    QVariantMap creationRetryResult;
    cancelledCreationRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            creationRetryResult = std::move(result);
        });
    if (!expect(waitForCondition([&]() {
                    return !creationRetryResult.isEmpty();
                }),
                "creation retry should complete"))
        return 1;
    if (!expect(creationRetryResult.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && cancelledCreationWallet.createdAccounts == 1
                    && cancelledCreationClient.freshLpAccountIds.size() == 1
                    && refreshClient.normalizedAccountIds.count(
                        QString(64, QLatin1Char('a')))
                        > creationReadsBeforeRetry,
                "late creation should be reserved for the retry"))
        return 1;

    FakeWallet failedReadWallet;
    failedReadWallet.failCreatedPublicRead = true;
    FakeAmmClient failedReadClient;
    NewPositionRuntime failedReadRuntime(
        &failedReadWallet, &failedReadClient, &sequencer);
    const int failedReadNormalizations = refreshClient.normalizedAccountIds.count(
        QString(64, QLatin1Char('a')));
    QVariantMap failedReadResult;
    failedReadRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            failedReadResult = std::move(result);
        });
    if (!expect(waitForCondition([&]() { return !failedReadResult.isEmpty(); }),
                "sequencer fallback should complete the failed wallet read"))
        return 1;
    if (!expect(failedReadResult.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && failedReadWallet.createdAccounts == 1
                    && failedReadWallet.submissions == 1
                    && refreshClient.normalizedAccountIds.count(
                        QString(64, QLatin1Char('a')))
                        > failedReadNormalizations,
                "sequencer should validate a newly created LP account"))
        return 1;

    FakeWallet cancelledSubmissionWallet;
    cancelledSubmissionWallet.deferSubmission = true;
    cancelledSubmissionWallet.submissionFailuresRemaining = 1;
    FakeAmmClient cancelledSubmissionClient;
    NewPositionRuntime cancelledSubmissionRuntime(
        &cancelledSubmissionWallet, &cancelledSubmissionClient, &sequencer);
    QVariantMap cancelledSubmissionResult;
    cancelledSubmissionRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            cancelledSubmissionResult = std::move(result);
        });
    if (!expect(waitForCondition(
                    [&]() { return cancelledSubmissionWallet.submissions == 1; }),
                "cancelled submission should reach the wallet"))
        return 1;
    WalletAccount pendingFreshLp;
    pendingFreshLp.address = QString(64, QLatin1Char('a'));
    pendingFreshLp.isPublic = true;
    WalletAccount discoveredAccount;
    discoveredAccount.address = QString(64, QLatin1Char('9'));
    discoveredAccount.isPublic = true;
    cancelledSubmissionRuntime.setWalletAccounts(
        { pendingFreshLp, discoveredAccount });
    QVariantMap blockedSubmissionRetry;
    cancelledSubmissionRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            blockedSubmissionRetry = std::move(result);
        });
    if (!expect(cancelledSubmissionResult.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_unavailable")
                    && blockedSubmissionRetry.value(QStringLiteral("code")).toString()
                        == QStringLiteral("submit_in_progress"),
                "retry should wait for cancelled wallet submission to settle"))
        return 1;
    cancelledSubmissionWallet.finishSubmission();
    cancelledSubmissionWallet.deferSubmission = false;
    QVariantMap settledSubmissionRetry;
    cancelledSubmissionRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            settledSubmissionRetry = std::move(result);
        });
    if (!expect(waitForCondition([&]() {
                    return !settledSubmissionRetry.isEmpty();
                }),
                "settled submission retry should complete"))
        return 1;
    if (!expect(settledSubmissionRetry.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && cancelledSubmissionWallet.createdAccounts == 1
                    && cancelledSubmissionWallet.submissions == 2
                    && cancelledSubmissionClient.freshLpAccountIds.size() == 2
                    && cancelledSubmissionClient.freshLpAccountIds.constFirst()
                        == cancelledSubmissionClient.freshLpAccountIds.constLast(),
                "rejected late submission should restore the LP reservation"))
        return 1;

    FakeWallet acceptedLateWallet;
    acceptedLateWallet.deferSubmission = true;
    FakeAmmClient acceptedLateClient;
    NewPositionRuntime acceptedLateRuntime(
        &acceptedLateWallet, &acceptedLateClient, &sequencer);
    QVariantMap acceptedLateResult;
    acceptedLateRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            acceptedLateResult = std::move(result);
        });
    if (!expect(waitForCondition(
                    [&]() { return acceptedLateWallet.submissions == 1; }),
                "late accepted submission should reach the wallet"))
        return 1;
    acceptedLateRuntime.cancelSubmit();
    QVariantMap acceptedLateBlockedRetry;
    acceptedLateRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            acceptedLateBlockedRetry = std::move(result);
        });
    if (!expect(acceptedLateResult.value(QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_unavailable")
                    && acceptedLateBlockedRetry.value(QStringLiteral("code")).toString()
                        == QStringLiteral("submit_in_progress"),
                "accepted late submission should block retry until settled"))
        return 1;
    acceptedLateWallet.finishSubmission();
    acceptedLateWallet.deferSubmission = false;
    QVariantMap acceptedLateRetry;
    acceptedLateRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            acceptedLateRetry = std::move(result);
        });
    if (!expect(waitForCondition([&]() { return !acceptedLateRetry.isEmpty(); }),
                "accepted late retry should complete"))
        return 1;
    if (!expect(acceptedLateRetry.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && acceptedLateWallet.createdAccounts == 2
                    && acceptedLateWallet.submissions == 2
                    && acceptedLateClient.freshLpAccountIds.size() == 2
                    && acceptedLateClient.freshLpAccountIds.constFirst()
                        != acceptedLateClient.freshLpAccountIds.constLast(),
                "accepted late submission should consume its LP reservation"))
        return 1;

    FakeWallet changedDuringSubmissionWallet;
    changedDuringSubmissionWallet.deferSubmission = true;
    FakeAmmClient changedDuringSubmissionClient;
    NewPositionRuntime changedDuringSubmissionRuntime(
        &changedDuringSubmissionWallet, &changedDuringSubmissionClient, &sequencer);
    QVariantMap changedDuringSubmissionResult;
    changedDuringSubmissionRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            changedDuringSubmissionResult = std::move(result);
        });
    if (!expect(waitForCondition([&]() {
                    return changedDuringSubmissionWallet.submissions == 1;
                }),
                "wallet-change submission should reach the wallet"))
        return 1;
    changedDuringSubmissionRuntime.clearWalletAccounts();
    if (!expect(changedDuringSubmissionResult.value(
                    QStringLiteral("code")).toString()
                    == QStringLiteral("wallet_unavailable"),
                "wallet change should cancel deferred submission"))
        return 1;
    changedDuringSubmissionWallet.finishSubmission();
    changedDuringSubmissionWallet.deferSubmission = false;
    QVariantMap changedWalletRetry;
    changedDuringSubmissionRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            changedWalletRetry = std::move(result);
        });
    if (!expect(waitForCondition([&]() { return !changedWalletRetry.isEmpty(); }),
                "new wallet submission should complete"))
        return 1;
    if (!expect(changedWalletRetry.value(QStringLiteral("status")).toString()
                    == QStringLiteral("submitted")
                    && changedDuringSubmissionWallet.createdAccounts == 2,
                "old wallet callback should not mutate new reservation state"))
        return 1;

    FakeWallet cancelledMutationWallet;
    cancelledMutationWallet.deferAccountCreation = true;
    FakeAmmClient cancelledMutationClient;
    NewPositionRuntime cancelledMutationRuntime(
        &cancelledMutationWallet, &cancelledMutationClient, &sequencer);
    int cancelledMutationCallbacks = 0;
    QVariantMap cancelledMutationResult;
    cancelledMutationRuntime.submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap result) {
            ++cancelledMutationCallbacks;
            cancelledMutationResult = std::move(result);
        });
    if (!expect(waitForCondition(
                    [&]() { return cancelledMutationWallet.createdAccounts == 1; }),
                "cancellable submit should reach account creation"))
        return 1;
    cancelledMutationRuntime.clearWalletAccounts();
    if (!expect(cancelledMutationCallbacks == 1
                    && cancelledMutationResult.value(QStringLiteral("code")).toString()
                        == QStringLiteral("wallet_unavailable"),
                "wallet change should immediately cancel pending mutation"))
        return 1;
    cancelledMutationWallet.finishAccountCreation();
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(cancelledMutationCallbacks == 1
                    && cancelledMutationWallet.submissions == 0,
                "stale account completion should have no transaction side effect"))
        return 1;

    FakeWallet destroyedRuntimeWallet;
    destroyedRuntimeWallet.deferSubmission = true;
    FakeAmmClient destroyedRuntimeClient;
    destroyedRuntimeClient.requiresFreshLp = false;
    int destroyedRuntimeCallbacks = 0;
    auto destroyedRuntime = std::make_unique<NewPositionRuntime>(
        &destroyedRuntimeWallet, &destroyedRuntimeClient, &sequencer);
    destroyedRuntime->submitAsync(
        request, QStringLiteral("sha256:expected"), readyNetwork(), true,
        [&](QVariantMap) { ++destroyedRuntimeCallbacks; });
    if (!expect(waitForCondition(
                    [&]() { return destroyedRuntimeWallet.submissions == 1; }),
                "lifetime test should reach async submission"))
        return 1;
    destroyedRuntime.reset();
    destroyedRuntimeWallet.finishSubmission();
    QCoreApplication::processEvents(QEventLoop::AllEvents, 10);
    if (!expect(destroyedRuntimeCallbacks == 0,
                "destroyed runtime should ignore late wallet completion"))
        return 1;

    return 0;
}
