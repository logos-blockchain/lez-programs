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
    void init();
    void cleanupTestCase();
    void clearsRegistryIdentityWhenUrlCleared();
    void ignoresEnvironmentChangesAfterConstruction();
    void startupLocalSourceWinsAndCarriesNoIds();

private:
    // Writes a one-network registry declaring `programId` / `configId` and returns
    // its file URL.
    QString writeRegistry(const QString& name, const QString& programId,
                          const QString& configId);
    // Writes a local tokens file with one token and returns its path.
    QString writeLocalTokens(const QString& name, const QString& symbol);

    QTemporaryDir m_tempDir;
};

void RegistryLoaderTest::initTestCase()
{
    QVERIFY(m_tempDir.isValid());
    QStandardPaths::setTestModeEnabled(true);
    QCoreApplication::setApplicationName(QDir(m_tempDir.path()).dirName());
}

void RegistryLoaderTest::init()
{
    qunsetenv("AMM_REGISTRY_URL");
    qunsetenv("AMM_NETWORK");
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
}

void RegistryLoaderTest::cleanupTestCase()
{
    QDir(QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)).removeRecursively();
}

QString RegistryLoaderTest::writeRegistry(const QString& name, const QString& programId,
                                          const QString& configId)
{
    const QJsonObject registry{
        {QStringLiteral("networks"), QJsonArray{QJsonObject{
            {QStringLiteral("id"), QStringLiteral("testnet")},
            {QStringLiteral("programIds"), QJsonObject{{QStringLiteral("amm"), programId}}},
            {QStringLiteral("ammConfigId"), configId},
        }}},
    };
    QFile file(m_tempDir.filePath(name));
    if (!file.open(QIODevice::WriteOnly))
        return {};
    file.write(QJsonDocument(registry).toJson());
    return QUrl::fromLocalFile(file.fileName()).toString();
}

QString RegistryLoaderTest::writeLocalTokens(const QString& name, const QString& symbol)
{
    const QJsonArray tokens{QJsonObject{
        {QStringLiteral("symbol"), symbol},
        {QStringLiteral("definitionId"), QString(64, QLatin1Char('c'))},
    }};
    QFile file(m_tempDir.filePath(name));
    if (!file.open(QIODevice::WriteOnly))
        return {};
    file.write(QJsonDocument(tokens).toJson());
    return file.fileName();
}

void RegistryLoaderTest::clearsRegistryIdentityWhenUrlCleared()
{
    const QString programId(64, QLatin1Char('a'));
    const QString configId(64, QLatin1Char('b'));
    const QString url = writeRegistry(QStringLiteral("clear.json"), programId, configId);
    QVERIFY(!url.isEmpty());

    RegistryLoader loader;
    QSignalSpy changed(&loader, &RegistryLoader::changed);
    loader.setConfiguredUrl(url);
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

    loader.setConfiguredUrl({});
    loader.refresh();

    QCOMPARE(changed.count(), 1);
    QCOMPARE(loader.source(), QStringLiteral("none"));
    QVERIFY(loader.networks().isEmpty());
    QVERIFY(loader.activeNetwork().isEmpty());
    QVERIFY(publishedProgramId.isEmpty());
    QVERIFY(publishedConfigId.isEmpty());
}

// The source and its ids are fixed when the loader is built: code in the host process
// that sets TOKENS_CONFIG or AMM_REGISTRY_URL later must not swap the token list or the
// AMM instance on the next refresh.
void RegistryLoaderTest::ignoresEnvironmentChangesAfterConstruction()
{
    const QString programId(64, QLatin1Char('a'));
    const QString configId(64, QLatin1Char('b'));
    const QString url = writeRegistry(QStringLiteral("user.json"), programId, configId);
    const QString attackerUrl = writeRegistry(QStringLiteral("attacker.json"),
                                              QString(64, QLatin1Char('d')),
                                              QString(64, QLatin1Char('e')));
    const QString attackerTokens =
        writeLocalTokens(QStringLiteral("attacker-tokens.json"), QStringLiteral("FAKE"));
    QVERIFY(!url.isEmpty() && !attackerUrl.isEmpty() && !attackerTokens.isEmpty());

    RegistryLoader loader;
    loader.setConfiguredUrl(url);
    loader.refresh();
    QTRY_COMPARE(loader.source(), QStringLiteral("remote"));

    qputenv("TOKENS_CONFIG", attackerTokens.toUtf8());
    qputenv("AMM_REGISTRY_URL", attackerUrl.toUtf8());
    QVERIFY(!loader.hasLocalSource());
    QSignalSpy changed(&loader, &RegistryLoader::changed);
    loader.refresh();
    // The cache is served first; wait for the fetched snapshot.
    QTRY_VERIFY(!changed.isEmpty() && loader.source() == QStringLiteral("remote"));

    QCOMPARE(loader.activeAmmProgramId(), programId);
    QCOMPARE(loader.activeAmmConfigId(), configId);
}

void RegistryLoaderTest::startupLocalSourceWinsAndCarriesNoIds()
{
    const QString url = writeRegistry(QStringLiteral("ignored.json"),
                                      QString(64, QLatin1Char('a')),
                                      QString(64, QLatin1Char('b')));
    const QString tokens = writeLocalTokens(QStringLiteral("dev-tokens.json"),
                                            QStringLiteral("DEV"));
    QVERIFY(!url.isEmpty() && !tokens.isEmpty());
    qputenv("TOKENS_CONFIG", tokens.toUtf8());

    RegistryLoader loader;
    QVERIFY(loader.hasLocalSource());
    loader.setConfiguredUrl(url);
    loader.refresh();

    QCOMPARE(loader.source(), QStringLiteral("local"));
    QCOMPARE(loader.tokens().size(), 1);
    QVERIFY(loader.activeAmmProgramId().isEmpty());
    QVERIFY(loader.activeAmmConfigId().isEmpty());
    QVERIFY(loader.networks().isEmpty());

    // Unsetting it later does not switch back either: the refresh re-publishes the
    // local snapshot synchronously instead of starting a registry fetch.
    qunsetenv("TOKENS_CONFIG");
    QSignalSpy changed(&loader, &RegistryLoader::changed);
    loader.refresh();
    QCOMPARE(changed.count(), 1);
    QCOMPARE(loader.source(), QStringLiteral("local"));
}

QTEST_GUILESS_MAIN(RegistryLoaderTest)
#include "RegistryLoaderTest.moc"
