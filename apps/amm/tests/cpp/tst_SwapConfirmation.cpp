#include "SwapConfirmation.h"
#include "FakeWalletProvider.h"
#include "WalletController.h"

#include <QSettings>
#include <QTemporaryDir>
#include <QTest>

#include <limits>

namespace {

QVariantMap confirmedRequest()
{
    return {
        {QStringLiteral("contextRevision"), 7},
        {QStringLiteral("swapMode"), QStringLiteral("swap-exact-input")},
        {QStringLiteral("sellDefinitionId"), QString(64, QLatin1Char('a'))},
        {QStringLiteral("buyDefinitionId"), QString(64, QLatin1Char('b'))},
        {QStringLiteral("sellHoldingId"), QString(64, QLatin1Char('c'))},
        {QStringLiteral("buyHoldingId"), QString(64, QLatin1Char('d'))},
        {QStringLiteral("amount"), QStringLiteral("9007199254740993")},
        {QStringLiteral("bound"), QStringLiteral("18446744073709551615")},
        {QStringLiteral("deadline"), QStringLiteral("9007199254740995")},
    };
}

} // namespace

class SwapConfirmationTest : public QObject {
    Q_OBJECT

private slots:
    void initTestCase();
    void changedWalletSessionRejectsSamePathConfirmation_data();
    void changedWalletSessionRejectsSamePathConfirmation();
    void balanceAndAccountOrderChangesPreserveConfirmation();
    void walletCreationStartsSession();
    void failedConnectionPreservesSession();
    void staleBackendContextRejectsBeforeReplicaUpdate();
    void invalidRevision_data();
    void invalidRevision();
    void forwardsConfirmedStrings_data();
    void forwardsConfirmedStrings();
    void closedWalletRejects();
    void invalidEnvelope_data();
    void invalidEnvelope();
    void emptySubmissionResultIsError();

private:
    QTemporaryDir m_settingsDirectory;
};

void SwapConfirmationTest::initTestCase()
{
    QVERIFY(m_settingsDirectory.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope,
                       m_settingsDirectory.path());
}

void SwapConfirmationTest::changedWalletSessionRejectsSamePathConfirmation_data()
{
    QTest::addColumn<QString>("change");
    QTest::newRow("observed-shared-wallet-restore") << QStringLiteral("restore");
    QTest::newRow("shared-wallet-adoption") << QStringLiteral("adopt");
    QTest::newRow("disconnect-reconnect") << QStringLiteral("reconnect");
}

void SwapConfirmationTest::changedWalletSessionRejectsSamePathConfirmation()
{
    QFETCH(QString, change);
    FakeWalletProvider provider;
    provider.connectResult.adopted = true;
    provider.connectResult.snapshot.accounts = {
        {QString(64, QLatin1Char('c')), QStringLiteral("10"), true},
    };
    WalletController controller(provider, QStringLiteral("SwapConfirmationTest"));
    QVERIFY(controller.open());
    const WalletUiState initialState = controller.state();
    WalletUiState sourceState = initialState;
    int sourceRevision = 7;
    // Use the same controller notification and source comparison as AmmUiBackend.
    connect(&controller, &WalletController::stateChanged, this, [&]() {
        if (SwapConfirmation::walletContextChanged(sourceState, controller.state()))
            ++sourceRevision;
        sourceState = controller.state();
    });
    const QVariantMap pending = confirmedRequest();

    if (change == QStringLiteral("restore")) {
        // The pinned core's restore_storage replaces its key chain in place.
        // A refresh observes different accounts without changing paths/open state.
        provider.snapshotResult = provider.connectResult.snapshot;
        provider.snapshotResult.accounts.front().address = QString(64, QLatin1Char('e'));
        controller.refresh();
        QVERIFY(provider.lastForceRefresh);
    } else {
        if (change == QStringLiteral("reconnect"))
            controller.disconnect();
        QVERIFY(controller.open());
    }

    QCOMPARE(controller.state().isWalletOpen, initialState.isWalletOpen);
    QCOMPARE(controller.state().configPath, initialState.configPath);
    QCOMPARE(controller.state().storagePath, initialState.storagePath);
    QCOMPARE(controller.state().walletHome, initialState.walletHome);
    QCOMPARE(controller.state().sequencerAddress, initialState.sequencerAddress);
    QVERIFY(controller.state().sessionRevision != initialState.sessionRevision);
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(pending, sourceRevision, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("unexpected-submission");
        });
    QCOMPARE(dispatches, 0);
    QCOMPARE(result.value(QStringLiteral("error")).toString(), QStringLiteral("swap_context_changed"));
}

void SwapConfirmationTest::balanceAndAccountOrderChangesPreserveConfirmation()
{
    FakeWalletProvider provider;
    provider.connectResult.snapshot.accounts = {
        {QString(64, QLatin1Char('c')), QStringLiteral("10"), true},
        {QString(64, QLatin1Char('d')), QStringLiteral("20"), false},
    };
    WalletController controller(provider, QStringLiteral("SwapConfirmationTest"));
    QVERIFY(controller.open());
    const WalletUiState initialState = controller.state();
    provider.snapshotResult = provider.connectResult.snapshot;
    provider.snapshotResult.accounts.front().balance = QStringLiteral("30");
    provider.snapshotResult.accounts.swapItemsAt(0, 1);
    provider.snapshotResult.lastSyncedBlock = 8;
    provider.snapshotResult.currentBlockHeight = 9;
    controller.refresh();
    QCOMPARE(controller.state().sessionRevision, initialState.sessionRevision);
    QVERIFY(!SwapConfirmation::walletContextChanged(initialState, controller.state()));
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(confirmedRequest(), 7, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("confirmed-submission");
        });
    QCOMPARE(dispatches, 1);
    QCOMPARE(result.value(QStringLiteral("status")).toString(), QStringLiteral("ok"));
}

void SwapConfirmationTest::walletCreationStartsSession()
{
    FakeWalletProvider provider;
    provider.createWalletResult.mnemonic = QStringLiteral("test mnemonic");
    WalletController controller(provider, QStringLiteral("SwapConfirmationTest"));
    const quint64 initialRevision = controller.state().sessionRevision;
    QCOMPARE(controller.createDefaultWallet({}), QStringLiteral("test mnemonic"));
    QVERIFY(controller.state().sessionRevision != initialRevision);
}

void SwapConfirmationTest::failedConnectionPreservesSession()
{
    FakeWalletProvider provider;
    provider.connectResult.failure = WalletFailure::OpenFailed;
    WalletController controller(provider, QStringLiteral("SwapConfirmationTest"));
    const quint64 initialRevision = controller.state().sessionRevision;
    QVERIFY(!controller.open());
    QCOMPARE(controller.state().sessionRevision, initialRevision);
}

void SwapConfirmationTest::staleBackendContextRejectsBeforeReplicaUpdate()
{
    // The QML replica can still show revision 7 after the backend changes its
    // wallet/network context to revision 8. The gate must use backend state.
    const QVariantMap pending = confirmedRequest();
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(pending, 8, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("unexpected-submission");
        });
    QCOMPARE(dispatches, 0);
    QCOMPARE(result.value(QStringLiteral("status")).toString(), QStringLiteral("error"));
    QCOMPARE(result.value(QStringLiteral("error")).toString(), QStringLiteral("swap_context_changed"));
    QVERIFY(!result.contains(QStringLiteral("txHash")));
}

void SwapConfirmationTest::invalidRevision_data()
{
    QTest::addColumn<QVariant>("revision");
    QTest::addColumn<int>("currentRevision");
    QTest::newRow("missing") << QVariant{} << 0;
    QTest::newRow("string") << QVariant(QStringLiteral("7")) << 7;
    QTest::newRow("boolean-true") << QVariant(true) << 1;
    QTest::newRow("boolean-false") << QVariant(false) << 0;
    QTest::newRow("fraction") << QVariant(7.1) << 7;
    QTest::newRow("fraction-rounds-to-current") << QVariant(6.9) << 7;
    QTest::newRow("nan") << QVariant(std::numeric_limits<double>::quiet_NaN()) << 0;
    QTest::newRow("infinity") << QVariant(std::numeric_limits<double>::infinity()) << 0;
    QTest::newRow("out-of-range") << QVariant(4294967303.0) << 7;
    QTest::newRow("bytes") << QVariant(QByteArray("7")) << 7;
    QTest::newRow("stale") << QVariant(6) << 7;
}

void SwapConfirmationTest::invalidRevision()
{
    QFETCH(QVariant, revision);
    QFETCH(int, currentRevision);
    QVariantMap request = confirmedRequest();
    request.remove(QStringLiteral("contextRevision"));
    if (revision.isValid())
        request.insert(QStringLiteral("contextRevision"), revision);
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(request, currentRevision, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("unexpected-submission");
        });
    QCOMPARE(dispatches, 0);
    QCOMPARE(result.value(QStringLiteral("error")).toString(), QStringLiteral("swap_context_changed"));
}

void SwapConfirmationTest::forwardsConfirmedStrings_data()
{
    QTest::addColumn<QString>("mode");
    QTest::addColumn<QVariant>("revision");
    QTest::newRow("exact-input-integer") << QStringLiteral("swap-exact-input") << QVariant(7);
    QTest::newRow("exact-input-qml-number") << QStringLiteral("swap-exact-input") << QVariant(7.0);
    QTest::newRow("exact-output-integer") << QStringLiteral("swap-exact-output") << QVariant(7);
    QTest::newRow("exact-output-qml-number") << QStringLiteral("swap-exact-output") << QVariant(7.0);
}

void SwapConfirmationTest::forwardsConfirmedStrings()
{
    QFETCH(QString, mode);
    QFETCH(QVariant, revision);
    QVariantMap request = confirmedRequest();
    request.insert(QStringLiteral("swapMode"), mode);
    request.insert(QStringLiteral("contextRevision"), revision);
    int dispatches = 0;
    SwapConfirmation::Request actual;
    const QVariantMap result = SwapConfirmation::submit(request, 7, true,
        [&](const SwapConfirmation::Request& confirmed) {
            ++dispatches;
            actual = confirmed;
            return QStringLiteral("signed-snapshot-tx");
        });
    QCOMPARE(dispatches, 1);
    QCOMPARE(actual.mode, mode);
    QCOMPARE(actual.input, request.value(QStringLiteral("sellDefinitionId")).toString());
    QCOMPARE(actual.output, request.value(QStringLiteral("buyDefinitionId")).toString());
    QCOMPARE(actual.inputHolding, request.value(QStringLiteral("sellHoldingId")).toString());
    QCOMPARE(actual.outputHolding, request.value(QStringLiteral("buyHoldingId")).toString());
    QCOMPARE(actual.amount, QStringLiteral("9007199254740993"));
    QCOMPARE(actual.bound, QStringLiteral("18446744073709551615"));
    QCOMPARE(actual.deadline, QStringLiteral("9007199254740995"));
    QCOMPARE(result.value(QStringLiteral("status")).toString(), QStringLiteral("ok"));
    QCOMPARE(result.value(QStringLiteral("txHash")).toString(), QStringLiteral("signed-snapshot-tx"));
    QVERIFY(!result.contains(QStringLiteral("error")));
}

void SwapConfirmationTest::closedWalletRejects()
{
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(confirmedRequest(), 7, false,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("unexpected-submission");
        });
    QCOMPARE(dispatches, 0);
    QCOMPARE(result.value(QStringLiteral("error")).toString(), QStringLiteral("wallet_unavailable"));
}

void SwapConfirmationTest::invalidEnvelope_data()
{
    QTest::addColumn<QString>("field");
    QTest::addColumn<QVariant>("value");
    QTest::addColumn<QString>("error");
    QTest::newRow("invalid-mode") << QStringLiteral("swapMode") << QVariant(QStringLiteral("unknown"))
                                  << QStringLiteral("invalid_swap_mode");
    QTest::newRow("numeric-amount") << QStringLiteral("amount") << QVariant(9007199254740992.0)
                                    << QStringLiteral("invalid_swap_request");
    QTest::newRow("missing-bound") << QStringLiteral("bound") << QVariant{}
                                   << QStringLiteral("invalid_swap_request");
    QTest::newRow("numeric-deadline") << QStringLiteral("deadline") << QVariant(100)
                                      << QStringLiteral("invalid_swap_request");
    QTest::newRow("missing-holding") << QStringLiteral("sellHoldingId") << QVariant{}
                                     << QStringLiteral("invalid_swap_request");
}

void SwapConfirmationTest::invalidEnvelope()
{
    QFETCH(QString, field);
    QFETCH(QVariant, value);
    QFETCH(QString, error);
    QVariantMap request = confirmedRequest();
    request.remove(field);
    if (value.isValid())
        request.insert(field, value);
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(request, 7, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QStringLiteral("unexpected-submission");
        });
    QCOMPARE(dispatches, 0);
    QCOMPARE(result.value(QStringLiteral("error")).toString(), error);
}

void SwapConfirmationTest::emptySubmissionResultIsError()
{
    int dispatches = 0;
    const QVariantMap result = SwapConfirmation::submit(confirmedRequest(), 7, true,
        [&](const SwapConfirmation::Request&) {
            ++dispatches;
            return QString{};
        });
    QCOMPARE(dispatches, 1);
    QCOMPARE(result.value(QStringLiteral("status")).toString(), QStringLiteral("error"));
    QCOMPARE(result.value(QStringLiteral("error")).toString(), QStringLiteral("wallet_submission_failed"));
    QVERIFY(!result.contains(QStringLiteral("txHash")));
}

QTEST_GUILESS_MAIN(SwapConfirmationTest)

#include "tst_SwapConfirmation.moc"
