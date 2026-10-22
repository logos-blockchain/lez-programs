#include "RegistryLoader.h"

#include <memory>

#include <QFile>
#include <QTest>
#include <QTemporaryDir>

class RegistryLoaderTest : public QObject {
    Q_OBJECT

private slots:
    void init();
    void cleanup();
    void clearingTheRegistryDropsTheSelectedConfig();
    void localFilesDropTheSelectedConfig();

private:
    static QByteArray registryBody();
    std::unique_ptr<RegistryLoader> m_loader;
};

void RegistryLoaderTest::init()
{
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
    qunsetenv("AMM_REGISTRY_URL");
    qunsetenv("AMM_NETWORK");
    m_loader = std::make_unique<RegistryLoader>();
    QVERIFY(m_loader->applyRegistry(registryBody(), QStringLiteral("remote")));
    QCOMPARE(m_loader->activeAmmProgramId(), QStringLiteral("P"));
    QCOMPARE(m_loader->activeAmmConfigId(), QStringLiteral("C"));
}

void RegistryLoaderTest::cleanup()
{
    qunsetenv("TOKENS_CONFIG");
    qunsetenv("AMM_POOLS_CONFIG");
    qunsetenv("AMM_REGISTRY_URL");
    qunsetenv("AMM_NETWORK");
}

void RegistryLoaderTest::clearingTheRegistryDropsTheSelectedConfig()
{
    m_loader->setConfiguredUrl({});
    m_loader->refresh();

    QCOMPARE(m_loader->source(), QStringLiteral("none"));
    QCOMPARE(m_loader->activeAmmProgramId(), QString());
    QCOMPARE(m_loader->activeAmmConfigId(), QString());
}

void RegistryLoaderTest::localFilesDropTheSelectedConfig()
{
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString tokensPath = dir.filePath(QStringLiteral("tokens.json"));
    QFile tokens(tokensPath);
    QVERIFY(tokens.open(QIODevice::WriteOnly));
    QVERIFY(tokens.write(R"([{"symbol":"AAA","name":"Token A","definitionId":"token-a"}])") > 0);
    tokens.close();

    qputenv("TOKENS_CONFIG", tokensPath.toUtf8());
    m_loader->refresh();

    QCOMPARE(m_loader->source(), QStringLiteral("local"));
    QCOMPARE(m_loader->activeAmmProgramId(), QString());
    QCOMPARE(m_loader->activeAmmConfigId(), QString());
}

QByteArray RegistryLoaderTest::registryBody()
{
    return QByteArrayLiteral(
        R"({"networks":[{"id":"test","name":"Test","programIds":{"amm":"P"},"ammConfigId":"C"}],)"
        R"("tokens":[{"network":"test","definitionId":"token-a","symbol":"AAA","name":"Token A"}],)"
        R"("pools":[{"network":"test","tokenA":"AAA","tokenB":"BBB"}]})");
}

QTEST_MAIN(RegistryLoaderTest)
#include "RegistryLoaderTest.moc"
