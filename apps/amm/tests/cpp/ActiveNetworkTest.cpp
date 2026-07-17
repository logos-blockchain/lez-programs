#include "ActiveNetwork.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryFile>
#include <QtTest>

class ActiveNetworkTest : public QObject {
    Q_OBJECT

private slots:
    void validatesIdentityBeforeReadiness();
};

void ActiveNetworkTest::validatesIdentityBeforeReadiness()
{
    const QString identity(64, QLatin1Char('a'));
    const QString programId(64, QLatin1Char('b'));
    const QString tokenId(64, QLatin1Char('c'));
    QTemporaryFile config;
    QVERIFY(config.open());
    config.write(QJsonDocument(QJsonObject {
        { QStringLiteral("channelId"), identity },
        { QStringLiteral("ammProgramId"), programId },
        { QStringLiteral("tokenDefinitionIds"), QJsonArray { tokenId } },
    }).toJson(QJsonDocument::Compact));
    config.flush();
    qputenv("AMM_UI_NETWORK", "devnet");
    qputenv("AMM_UI_DEVNET_FILE", config.fileName().toLocal8Bit());

    ActiveNetwork network;
    QVERIFY(network.load());
    QCOMPARE(network.status(), QStringLiteral("network_unknown"));
    network.sequencerChanged(true);
    network.finishIdentityProbe(QString(64, QLatin1Char('d')));
    QCOMPARE(network.status(), QStringLiteral("network_mismatch"));
    network.reachabilityChanged(false, true);
    network.reachabilityChanged(true, false);
    network.finishIdentityProbe(identity);
    QCOMPARE(network.status(), QStringLiteral("ready"));
    QCOMPARE(network.snapshot().fingerprint, QStringLiteral("channel:") + identity);
    QCOMPARE(network.snapshot().tokenIds, QStringList { tokenId });
}

QTEST_GUILESS_MAIN(ActiveNetworkTest)
#include "ActiveNetworkTest.moc"
