#include "SequencerNetworkSettings.h"

#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryFile>
#include <QtTest>

class SequencerNetworkSettingsTest : public QObject {
    Q_OBJECT

private slots:
    void loadsBundledTestnetIdentity();
    void loadsDevnetChannelIdentity();
    void rejectsInvalidDevnetIdentity();
};

void SequencerNetworkSettingsTest::loadsBundledTestnetIdentity()
{
    const auto settings = SequencerNetworkSettingsLoader::load(
        QStringLiteral("testnet"), {});

    QVERIFY(settings);
    QCOMPARE(settings->context.id, QStringLiteral("testnet"));
    QCOMPARE(settings->context.expectedIdentity,
             QStringLiteral("0d25d71fca70d7008a892f6b3f768a4c66badbcd64e67d79ca595b92f1db544a"));
    QCOMPARE(settings->context.fingerprintPrefix, QStringLiteral("block10:"));
    QCOMPARE(settings->identityMethod, SequencerIdentityMethod::CheckpointBlock);
}

void SequencerNetworkSettingsTest::loadsDevnetChannelIdentity()
{
    const QString identity(64, QLatin1Char('a'));
    QTemporaryFile config;
    QVERIFY(config.open());
    const QByteArray contents = QJsonDocument(QJsonObject {
        { QStringLiteral("channelId"), identity },
    }).toJson(QJsonDocument::Compact);
    QCOMPARE(config.write(contents), qint64(contents.size()));
    config.flush();

    const auto settings = SequencerNetworkSettingsLoader::load(
        QStringLiteral("devnet"), config.fileName());

    QVERIFY(settings);
    QCOMPARE(settings->context.id, QStringLiteral("devnet"));
    QCOMPARE(settings->context.expectedIdentity, identity);
    QCOMPARE(settings->context.fingerprintPrefix, QStringLiteral("channel:"));
    QCOMPARE(settings->identityMethod, SequencerIdentityMethod::ChannelId);
}

void SequencerNetworkSettingsTest::rejectsInvalidDevnetIdentity()
{
    QTemporaryFile config;
    QVERIFY(config.open());
    const QByteArray contents = QJsonDocument(QJsonObject {
        { QStringLiteral("channelId"), QStringLiteral("not-an-identity") },
    }).toJson(QJsonDocument::Compact);
    QCOMPARE(config.write(contents), qint64(contents.size()));
    config.flush();

    QVERIFY(!SequencerNetworkSettingsLoader::load(
        QStringLiteral("devnet"), config.fileName()));
}

QTEST_GUILESS_MAIN(SequencerNetworkSettingsTest)
#include "SequencerNetworkSettingsTest.moc"
