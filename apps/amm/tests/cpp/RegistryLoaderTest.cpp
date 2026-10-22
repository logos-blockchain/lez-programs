#include "RegistryLoader.h"

#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QTest>

namespace {
const char PREVIOUS_URL[] = "https://saved.example/registry.json";
const char NEXT_URL[] = "https://override.example/registry.json";
const char UNREACHABLE_URL[] = "http://127.0.0.1:9/registry.json";

QByteArray registryBody()
{
    return QByteArrayLiteral(
        R"({"networks":[{"id":"net","name":"Net","programIds":{"amm":"P"},"ammConfigId":"C"}],)"
        R"("tokens":[{"network":"net","definitionId":"def","symbol":"AAA","name":"Token"}],)"
        R"("pools":[]})");
}

QString field(const RegistryLoader& loader, const char* key)
{
    return loader.status().value(QLatin1String(key)).toString();
}
}

class RegistryLoaderTest : public QObject {
    Q_OBJECT

private slots:
    void init();
    void cleanup();

    void localFilesReportTheirSourceAndWhyNetworksAreHidden();
    void missingRegistryReportsThatNothingIsConfigured();
    void pendingRefreshKeepsThePreviousSnapshotOrigin();
    void failedRefreshDoesNotAdoptTheRequestedUrl();
    void registryWithoutNetworksExplainsTheEmptyPicker();

private:
    std::unique_ptr<RegistryLoader> m_loader;
    std::unique_ptr<QTemporaryDir> m_dir;
};

void RegistryLoaderTest::init()
{
    QStandardPaths::setTestModeEnabled(true);
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
    qunsetenv("AMM_REGISTRY_URL");
    qunsetenv("AMM_NETWORK");
    m_dir = std::make_unique<QTemporaryDir>();
    QVERIFY(m_dir->isValid());
    m_loader = std::make_unique<RegistryLoader>();
    QVERIFY(m_loader->applyRegistry(registryBody(), QStringLiteral("remote"),
                                    QString::fromLatin1(PREVIOUS_URL)));
}

void RegistryLoaderTest::cleanup()
{
    m_loader.reset();
    m_dir.reset();
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
    qunsetenv("AMM_REGISTRY_URL");
}

void RegistryLoaderTest::localFilesReportTheirSourceAndWhyNetworksAreHidden()
{
    const QString tokensPath = m_dir->filePath(QStringLiteral("tokens.json"));
    QFile tokens(tokensPath);
    QVERIFY(tokens.open(QIODevice::WriteOnly));
    tokens.write(QByteArrayLiteral(
        R"([{"symbol":"LOC","name":"Local","definitionId":"local-def"}])"));
    tokens.close();
    qputenv("TOKENS_CONFIG", tokensPath.toUtf8());
    qputenv("AMM_REGISTRY_URL", NEXT_URL);

    m_loader->setConfiguredUrl(QString::fromLatin1(PREVIOUS_URL));
    m_loader->refresh();

    QCOMPARE(field(*m_loader, "source"), QStringLiteral("local"));
    QCOMPARE(field(*m_loader, "snapshotOrigin"), QString());
    QCOMPARE(field(*m_loader, "requestUrl"), QString::fromLatin1(NEXT_URL));
    QCOMPARE(field(*m_loader, "refreshState"), QStringLiteral("ready"));
    QCOMPARE(field(*m_loader, "overrideKind"), QStringLiteral("local_files"));
    QCOMPARE(field(*m_loader, "networkNote"),
             QStringLiteral("Local token files do not declare networks."));
    QCOMPARE(m_loader->tokens().size(), 1);
}

void RegistryLoaderTest::missingRegistryReportsThatNothingIsConfigured()
{
    m_loader->setConfiguredUrl({});
    m_loader->refresh();

    QCOMPARE(field(*m_loader, "source"), QStringLiteral("none"));
    QCOMPARE(field(*m_loader, "snapshotOrigin"), QString());
    QCOMPARE(field(*m_loader, "requestUrl"), QString());
    QCOMPARE(field(*m_loader, "refreshState"), QStringLiteral("ready"));
    QCOMPARE(field(*m_loader, "overrideKind"), QString());
    QCOMPARE(field(*m_loader, "networkNote"),
             QStringLiteral("No registry is configured."));
    QCOMPARE(m_loader->tokens().size(), 0);
}

void RegistryLoaderTest::pendingRefreshKeepsThePreviousSnapshotOrigin()
{
    qputenv("AMM_REGISTRY_URL", UNREACHABLE_URL);
    m_loader->setConfiguredUrl(QString::fromLatin1(PREVIOUS_URL));
    m_loader->refresh();

    QCOMPARE(field(*m_loader, "source"), QStringLiteral("remote"));
    QCOMPARE(field(*m_loader, "snapshotOrigin"), QString::fromLatin1(PREVIOUS_URL));
    QCOMPARE(field(*m_loader, "requestUrl"), QString::fromLatin1(UNREACHABLE_URL));
    QCOMPARE(field(*m_loader, "refreshState"), QStringLiteral("loading"));
    QCOMPARE(field(*m_loader, "overrideKind"), QStringLiteral("env_url"));
    QCOMPARE(m_loader->tokens().size(), 1);
    QCOMPARE(m_loader->networks().size(), 1);
}

void RegistryLoaderTest::failedRefreshDoesNotAdoptTheRequestedUrl()
{
    m_loader->setConfiguredUrl(QString::fromLatin1(UNREACHABLE_URL));
    QSignalSpy failed(m_loader.get(), &RegistryLoader::statusChanged);
    m_loader->refresh();

    QTRY_COMPARE(field(*m_loader, "refreshState"), QStringLiteral("failed"));
    QVERIFY(failed.size() >= 1);
    QCOMPARE(field(*m_loader, "source"), QStringLiteral("remote"));
    QCOMPARE(field(*m_loader, "snapshotOrigin"), QString::fromLatin1(PREVIOUS_URL));
    QCOMPARE(field(*m_loader, "requestUrl"), QString::fromLatin1(UNREACHABLE_URL));
    QVERIFY(!field(*m_loader, "refreshError").isEmpty());
    QCOMPARE(m_loader->tokens().size(), 1);
    QCOMPARE(field(*m_loader, "networkNote"), QString());
}

void RegistryLoaderTest::registryWithoutNetworksExplainsTheEmptyPicker()
{
    m_loader->applyRegistry(QByteArrayLiteral(R"({"networks":[]})"),
                            QStringLiteral("remote"),
                            QString::fromLatin1(PREVIOUS_URL));

    QCOMPARE(field(*m_loader, "source"), QStringLiteral("remote"));
    QCOMPARE(field(*m_loader, "snapshotOrigin"), QString::fromLatin1(PREVIOUS_URL));
    QCOMPARE(m_loader->networks().size(), 0);
    QCOMPARE(field(*m_loader, "networkNote"),
             QStringLiteral("This registry does not declare any networks."));
}

QTEST_MAIN(RegistryLoaderTest)
#include "RegistryLoaderTest.moc"
