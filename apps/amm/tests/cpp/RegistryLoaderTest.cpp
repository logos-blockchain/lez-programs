#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QtTest>

#include "RegistryLoader.h"

class RegistryLoaderTest : public QObject {
    Q_OBJECT

private slots:
    void initTestCase();
    void cleanupTestCase();
    void clearsRegistryIdentity_data();
    void clearsRegistryIdentity();

private:
    QTemporaryDir m_tempDir;
};

void RegistryLoaderTest::initTestCase()
{
    QVERIFY(m_tempDir.isValid());
    QStandardPaths::setTestModeEnabled(true);
    QCoreApplication::setApplicationName(QDir(m_tempDir.path()).dirName());
    qunsetenv("AMM_REGISTRY_URL");
    qunsetenv("AMM_NETWORK");
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
}

void RegistryLoaderTest::cleanupTestCase()
{
    QDir(QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)).removeRecursively();
}

void RegistryLoaderTest::clearsRegistryIdentity_data()
{
    QTest::addColumn<QByteArray>("localEnv");
    QTest::addColumn<QString>("expectedSource");
    QTest::newRow("clear-url") << QByteArray{} << QStringLiteral("none");
    QTest::newRow("local-tokens") << QByteArray("TOKENS_CONFIG") << QStringLiteral("local");
    QTest::newRow("local-pools") << QByteArray("AMM_POOLS_CONFIG") << QStringLiteral("local");
}

void RegistryLoaderTest::clearsRegistryIdentity()
{
    QFETCH(QByteArray, localEnv);
    QFETCH(QString, expectedSource);
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");

    const QString programId(64, QLatin1Char('a'));
    const QString configId(64, QLatin1Char('b'));
    const QJsonObject registry{
        {QStringLiteral("networks"), QJsonArray{QJsonObject{
            {QStringLiteral("id"), QStringLiteral("testnet")},
            {QStringLiteral("programIds"), QJsonObject{{QStringLiteral("amm"), programId}}},
            {QStringLiteral("ammConfigId"), configId},
        }}},
    };
    QFile registryFile(m_tempDir.filePath(QString::fromLatin1(QTest::currentDataTag()) + ".json"));
    QVERIFY(registryFile.open(QIODevice::WriteOnly));
    const QByteArray registryBytes = QJsonDocument(registry).toJson();
    QCOMPARE(registryFile.write(registryBytes), registryBytes.size());
    registryFile.close();

    RegistryLoader loader;
    QSignalSpy changed(&loader, &RegistryLoader::changed);
    loader.setConfiguredUrl(QUrl::fromLocalFile(registryFile.fileName()).toString());
    loader.refresh();
    QTRY_COMPARE(loader.source(), QStringLiteral("remote"));
    QCOMPARE(loader.activeAmmProgramId(), programId);
    QCOMPARE(loader.activeAmmConfigId(), configId);
    QCOMPARE(loader.activeNetwork(), QStringLiteral("testnet"));
    QCOMPARE(loader.networks().size(), 1);
    changed.clear();

    QString publishedProgramId;
    QString publishedConfigId;
    connect(&loader, &RegistryLoader::changed, this, [&]() {
        publishedProgramId = loader.activeAmmProgramId();
        publishedConfigId = loader.activeAmmConfigId();
    });

    if (localEnv.isEmpty()) {
        loader.setConfiguredUrl({});
    } else {
        QFile localFile(m_tempDir.filePath("local.json"));
        QVERIFY(localFile.open(QIODevice::WriteOnly));
        QCOMPARE(localFile.write("[]"), 2);
        localFile.close();
        qputenv(localEnv.constData(), localFile.fileName().toUtf8());
    }
    loader.refresh();

    QCOMPARE(changed.count(), 1);
    QCOMPARE(loader.source(), expectedSource);
    QVERIFY(loader.networks().isEmpty());
    QVERIFY(loader.activeNetwork().isEmpty());
    QVERIFY(publishedProgramId.isEmpty());
    QVERIFY(publishedConfigId.isEmpty());
}

QTEST_GUILESS_MAIN(RegistryLoaderTest)
#include "RegistryLoaderTest.moc"
