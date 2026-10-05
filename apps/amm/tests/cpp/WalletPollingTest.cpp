#include <QQmlComponent>
#include <QQmlEngine>
#include <QSettings>
#include <QTemporaryDir>
#include <QTimer>
#include <QtTest>

#include <memory>

#include "FakeWalletProvider.h"
#include "WalletController.h"

class WalletPollingTest final : public QObject {
    Q_OBJECT

private slots:
    void initTestCase();
    void init();
    void cleanup();
    void pollingRefreshesQuote_data();
    void pollingRefreshesQuote();
    void inactiveFlowWaitsForActivation();
    void staleReplyCannotRestoreQuoteDuringPoll_data();
    void staleReplyCannotRestoreQuoteDuringPoll();
    void readinessAndActivationRequestOneQuote();

private:
    void openWallet();
    void startPoll();
    int quoteCalls() const;

    QTemporaryDir m_settings;
    std::unique_ptr<QQmlEngine> m_engine;
    std::unique_ptr<QObject> m_root;
    std::unique_ptr<FakeWalletProvider> m_provider;
    std::unique_ptr<WalletController> m_controller;
    QObject* m_backend = nullptr;
    QObject* m_flow = nullptr;
    QTimer* m_pollTimer = nullptr;
};

void WalletPollingTest::initTestCase()
{
    QVERIFY(m_settings.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
}

void WalletPollingTest::init()
{
    m_engine = std::make_unique<QQmlEngine>();
    QQmlComponent component(m_engine.get());
    component.setData(R"QML(
import QtQml
import "." as State

QtObject {
    id: root
    property bool active: false
    property bool deferQuotes: false
    property var pendingQuotes: []
    property var request: ({
        "ok": true,
        "request": {
            "tokenAId": "A", "tokenBId": "B",
            "maxAmountA": "10", "maxAmountB": "10",
            "amountA": "10", "amountB": "10", "price": "1", "slippageBps": 100
        }
    })
    property QtObject backend: QtObject {
        property bool walletStateReady: false
        property bool poolExists: true
        property int quoteCalls: 0
        function resolvePoolAccount(tokenA, tokenB) {
            return poolExists ? { status: "ok", reserveA: "1000", reserveB: "1000" }
                              : { status: "error", error: "no_pool" }
        }
        function addLiquidityQuote(request) {
            ++quoteCalls
            return { quoteResult: true, status: "ok", amountA: "10", amountB: "10",
                     expectedLp: String(quoteCalls), minimumLp: "1" }
        }
        function createPoolQuote(request) {
            ++quoteCalls
            return { quoteResult: true, status: "ok", actualAmountA: "10",
                     actualAmountB: "10", expectedLp: String(quoteCalls),
                     minimumAmountA: "1", minimumAmountB: "1" }
        }
    }
    property QtObject runtime: QtObject {
        function watch(result, success, failure) {
            if (root.deferQuotes && result.quoteResult)
                root.pendingQuotes.push(function() { success(result) })
            else
                success(result)
        }
    }
    property State.NewPositionFlow flow: State.NewPositionFlow {
        backend: root.backend
        runtime: root.runtime
        active: root.active
        // LiquidityPage rebuilds the form's current request on this signal.
        onQuoteRefreshRequested: function(immediate) {
            scheduleQuote(immediate, root.request)
        }
    }
    function finishQuote(index) { pendingQuotes.splice(index, 1)[0]() }
}
)QML", QUrl::fromLocalFile(QStringLiteral(AMM_QML_STATE_DIR "/WalletPollingTest.qml")));
    m_root.reset(component.create());
    QVERIFY2(m_root, qPrintable(component.errorString()));
    m_backend = m_root->property("backend").value<QObject*>();
    m_flow = m_root->property("flow").value<QObject*>();
    QVERIFY(m_backend);
    QVERIFY(m_flow);

    m_provider = std::make_unique<FakeWalletProvider>();
    m_provider->deferAsync = true;
    m_controller = std::make_unique<WalletController>(
        *m_provider, QStringLiteral("AmmWalletPollingTest"));
    // Keep the same readiness contract as AmmUiBackend::syncWalletState.
    connect(m_controller.get(), &WalletController::stateChanged, m_root.get(), [this]() {
        const QString status = m_controller->state().syncStatus;
        m_backend->setProperty("walletStateReady", status != QStringLiteral("opening")
                              && status != QStringLiteral("syncing"));
    });
    m_pollTimer = m_controller->findChild<QTimer*>(QStringLiteral("walletSnapshotPollTimer"));
    QVERIFY(m_pollTimer);
}

void WalletPollingTest::cleanup()
{
    if (m_root)
        m_root->setProperty("active", false);
    if (m_controller)
        m_controller->disconnect();
    m_controller.reset();
    m_provider.reset();
    m_root.reset();
    m_engine.reset();
}

void WalletPollingTest::openWallet()
{
    QVERIFY(m_controller->open());
    m_provider->finishConnect();
    QCOMPARE(m_controller->state().syncStatus, QStringLiteral("ready"));
}

void WalletPollingTest::startPoll()
{
    const int expectedCalls = m_provider->snapshotCalls + 1;
    m_pollTimer->start(1);
    QTRY_COMPARE_WITH_TIMEOUT(m_provider->snapshotCalls, expectedCalls, 1000);
    QCOMPARE(m_controller->state().syncStatus, QStringLiteral("syncing"));
}

int WalletPollingTest::quoteCalls() const
{
    return m_backend->property("quoteCalls").toInt();
}

void WalletPollingTest::pollingRefreshesQuote_data()
{
    QTest::addColumn<bool>("poolExists");
    QTest::newRow("add-liquidity") << true;
    QTest::newRow("create-pool") << false;
}

void WalletPollingTest::pollingRefreshesQuote()
{
    QFETCH(bool, poolExists);
    m_backend->setProperty("poolExists", poolExists);
    m_root->setProperty("active", true);
    openWallet();
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 1, 1000);
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);

    startPoll();
    QCOMPARE(m_flow->property("quoteStale").toBool(), true);
    QCOMPARE(m_flow->property("quoteLoading").toBool(), false);
    QCOMPARE(quoteCalls(), 1);
    m_provider->finishSnapshot();
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 2, 1000);
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);
    QCOMPARE(m_flow->property("newPositionQuote").value<QJSValue>()
                 .property(QStringLiteral("expectedLp")).toString(), QStringLiteral("2"));
}

void WalletPollingTest::inactiveFlowWaitsForActivation()
{
    openWallet();
    startPoll();
    m_provider->finishSnapshot();
    QTest::qWait(20);
    QCOMPARE(quoteCalls(), 0);

    m_root->setProperty("active", true);
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 1, 1000);
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);
}

void WalletPollingTest::staleReplyCannotRestoreQuoteDuringPoll_data()
{
    QTest::addColumn<bool>("finishOldAfterRecovery");
    QTest::newRow("during-poll") << false;
    QTest::newRow("after-recovery") << true;
}

void WalletPollingTest::staleReplyCannotRestoreQuoteDuringPoll()
{
    QFETCH(bool, finishOldAfterRecovery);
    m_root->setProperty("deferQuotes", true);
    m_root->setProperty("active", true);
    openWallet();
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 1, 1000);
    QCOMPARE(m_flow->property("quoteLoading").toBool(), true);

    startPoll();
    if (!finishOldAfterRecovery) {
        QVERIFY(QMetaObject::invokeMethod(m_root.get(), "finishQuote",
                                         Q_ARG(QVariant, 0)));
    }
    QCOMPARE(m_flow->property("quoteStale").toBool(), true);
    QCOMPARE(m_flow->property("quoteLoading").toBool(), false);

    m_provider->finishSnapshot();
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 2, 1000);
    QCOMPARE(m_flow->property("quoteStale").toBool(), true);
    QCOMPARE(m_flow->property("quoteLoading").toBool(), true);
    QVERIFY(QMetaObject::invokeMethod(m_root.get(), "finishQuote",
                                     Q_ARG(QVariant, finishOldAfterRecovery ? 1 : 0)));
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);
    if (finishOldAfterRecovery) {
        QVERIFY(QMetaObject::invokeMethod(m_root.get(), "finishQuote",
                                         Q_ARG(QVariant, 0)));
    }
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);
    QCOMPARE(m_flow->property("newPositionQuote").value<QJSValue>()
                 .property(QStringLiteral("expectedLp")).toString(), QStringLiteral("2"));
}

void WalletPollingTest::readinessAndActivationRequestOneQuote()
{
    openWallet();
    m_root->setProperty("active", true);
    QTRY_COMPARE_WITH_TIMEOUT(quoteCalls(), 1, 1000);
    QTest::qWait(20);
    QCOMPARE(quoteCalls(), 1);
    QCOMPARE(m_flow->property("quoteStale").toBool(), false);
}

QTEST_GUILESS_MAIN(WalletPollingTest)
#include "WalletPollingTest.moc"
