#include <QtTest>

#include "SequencerNetworkContext.h"

namespace {
const QString NETWORK_ID = QStringLiteral("testnet");
const QString IDENTITY(64, QLatin1Char('a'));

SequencerNetworkContext::Configuration configuration()
{
    return {
        NETWORK_ID,
        IDENTITY,
        QStringLiteral("checkpoint:"),
    };
}
}

class SequencerNetworkContextTest final : public QObject {
    Q_OBJECT

private slots:
    void acceptsMatchingIdentity();
    void rejectsLateReplyAfterReachabilityLoss();
    void rejectsSupersededProbe();
    void rejectsInvalidConfiguration();
};

void SequencerNetworkContextTest::acceptsMatchingIdentity()
{
    SequencerNetworkContext context;

    QVERIFY(context.configure(configuration()));
    context.setSequencerAvailable(true);
    context.setReachable(true);
    const std::optional<quint64> probe = context.beginIdentityProbe();

    QVERIFY(probe.has_value());
    QVERIFY(context.finishIdentityProbe(*probe, IDENTITY));
    QCOMPARE(context.snapshot().id, NETWORK_ID);
    QCOMPARE(context.snapshot().status, QStringLiteral("ready"));
    QCOMPARE(context.snapshot().fingerprint, QStringLiteral("checkpoint:") + IDENTITY);
}

void SequencerNetworkContextTest::rejectsLateReplyAfterReachabilityLoss()
{
    SequencerNetworkContext context;

    QVERIFY(context.configure(configuration()));
    context.setSequencerAvailable(true);
    context.setReachable(true);
    const std::optional<quint64> probe = context.beginIdentityProbe();
    QVERIFY(probe.has_value());

    context.setReachable(false);

    QVERIFY(!context.finishIdentityProbe(*probe, IDENTITY));
    QCOMPARE(context.snapshot().status, QStringLiteral("network_unknown"));
    QVERIFY(context.snapshot().fingerprint.isEmpty());
}

void SequencerNetworkContextTest::rejectsSupersededProbe()
{
    SequencerNetworkContext context;

    QVERIFY(context.configure(configuration()));
    context.setSequencerAvailable(true);
    context.setReachable(true);
    const std::optional<quint64> firstProbe = context.beginIdentityProbe();
    QVERIFY(firstProbe.has_value());

    context.setReachable(false);
    context.setReachable(true);
    const std::optional<quint64> secondProbe = context.beginIdentityProbe();
    QVERIFY(secondProbe.has_value());

    QVERIFY(!context.finishIdentityProbe(*firstProbe, IDENTITY));
    QVERIFY(context.finishIdentityProbe(*secondProbe, IDENTITY));
    QCOMPARE(context.snapshot().status, QStringLiteral("ready"));
}

void SequencerNetworkContextTest::rejectsInvalidConfiguration()
{
    SequencerNetworkContext context;
    SequencerNetworkContext::Configuration invalid = configuration();
    invalid.expectedIdentity = QString(64, QLatin1Char('A'));

    QVERIFY(!context.configure(invalid));
    QVERIFY(!context.isConfigured());
    QCOMPARE(context.snapshot().id, NETWORK_ID);
    QCOMPARE(context.snapshot().status, QStringLiteral("config_missing"));
}

QTEST_MAIN(SequencerNetworkContextTest)

#include "SequencerNetworkContextTest.moc"
