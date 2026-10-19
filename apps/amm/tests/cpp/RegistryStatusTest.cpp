#include "RegistryLoader.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>

class RegistryStatusTest : public QObject {
    Q_OBJECT

private:
    QTemporaryDir m_temp;

    QString writeSource(const QString& name, const QByteArray& body)
    {
        const QString path = m_temp.filePath(name);
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
            return {};
        if (file.write(body) != body.size())
            return {};
        return QUrl::fromLocalFile(path).toString();
    }

    static QByteArray registry()
    {
        return R"({"networks":[{"id":"test","name":"Test"}],"tokens":[{"network":"test","definitionId":"token","symbol":"T"}]})";
    }

    static QString cachePath()
    {
        return QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)
            + QStringLiteral("/amm-registry-cache.json");
    }

    static bool writeCache(const QString& url)
    {
        const QString path = cachePath();
        if (!QDir().mkpath(QFileInfo(path).absolutePath()))
            return false;
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
            return false;
        const QByteArray body = QJsonDocument(QJsonObject{
            {QStringLiteral("url"), url},
            {QStringLiteral("registry"), QJsonDocument::fromJson(registry()).object()},
        }).toJson();
        return file.write(body) == body.size();
    }

private slots:
    void initTestCase()
    {
        QVERIFY(m_temp.isValid());
        QCoreApplication::setApplicationName(QStringLiteral("amm-registry-status-test"));
        qputenv("XDG_DATA_HOME", m_temp.path().toUtf8());
    }

    void init()
    {
        qunsetenv("TOKENS_CONFIG");
        qunsetenv("AMM_POOLS_CONFIG");
        qunsetenv("AMM_REGISTRY_URL");
        qunsetenv("AMM_NETWORK");
        QFile::remove(cachePath());
    }

    void noSource()
    {
        RegistryLoader loader;
        QSignalSpy status(&loader, &RegistryLoader::statusChanged);
        loader.refresh();
        QCOMPARE(loader.status().value("source").toString(), QStringLiteral("none"));
        QVERIFY(loader.status().value("effectiveUrl").toString().isEmpty());
        QVERIFY(loader.status().value("snapshotUrl").toString().isEmpty());
        QVERIFY(!loader.status().value("loading").toBool());
        QVERIFY(loader.networks().isEmpty());
        QCOMPARE(status.count(), 1);
    }

    void localOverridesConfiguredAndEnvironmentUrl()
    {
        const QString url = writeSource("tokens.json", R"([{"definitionId":"local-token"}])");
        QVERIFY(!url.isEmpty());
        qputenv("TOKENS_CONFIG", QUrl(url).toLocalFile().toUtf8());
        qputenv("AMM_REGISTRY_URL", "https://env.example/registry.json");
        RegistryLoader loader;
        loader.setConfiguredUrl("https://saved.example/registry.json");
        loader.refresh();
        QCOMPARE(loader.status().value("source").toString(), QStringLiteral("local"));
        QCOMPARE(loader.status().value("override").toString(), QStringLiteral("TOKENS_CONFIG"));
        QVERIFY(loader.status().value("effectiveUrl").toString().isEmpty());
        QVERIFY(loader.status().value("snapshotUrl").toString().isEmpty());
        QVERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.tokens().size(), 1);
        QVERIFY(loader.networks().isEmpty());
    }

    void remoteOverridePublishesRequestThenSnapshot()
    {
        const QString url = writeSource("override.json", registry());
        QVERIFY(!url.isEmpty());
        qputenv("AMM_REGISTRY_URL", url.toUtf8());
        RegistryLoader loader;
        loader.setConfiguredUrl("https://saved.example/registry.json");
        QSignalSpy snapshots(&loader, &RegistryLoader::changed);
        QSignalSpy status(&loader, &RegistryLoader::statusChanged);
        loader.refresh();
        QCOMPARE(loader.status().value("source").toString(), QStringLiteral("none"));
        QCOMPARE(loader.status().value("effectiveUrl").toString(), url);
        QCOMPARE(loader.status().value("override").toString(), QStringLiteral("AMM_REGISTRY_URL"));
        QVERIFY(loader.status().value("loading").toBool());
        QVERIFY(loader.status().value("snapshotUrl").toString().isEmpty());
        QCOMPARE(snapshots.count(), 0);
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("source").toString(), QStringLiteral("remote"));
        QCOMPARE(loader.status().value("snapshotUrl").toString(), url);
        QVERIFY(loader.status().value("error").toString().isEmpty());
        QCOMPARE(loader.activeNetwork(), QStringLiteral("test"));
        QCOMPARE(snapshots.count(), 1);
        QVERIFY(status.count() >= 2);
    }

    void failedRefreshKeepsPreviousOrigin()
    {
        const QString oldUrl = writeSource("old.json", registry());
        const QString newUrl = QUrl::fromLocalFile(m_temp.filePath("missing.json")).toString();
        QVERIFY(!oldUrl.isEmpty());
        RegistryLoader loader;
        loader.setConfiguredUrl(oldUrl);
        loader.refresh();
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("snapshotUrl").toString(), oldUrl);
        const int revision = loader.revision();
        loader.setConfiguredUrl(newUrl);
        loader.refresh();
        QCOMPARE(loader.status().value("effectiveUrl").toString(), newUrl);
        QCOMPARE(loader.status().value("snapshotUrl").toString(), oldUrl);
        QVERIFY(loader.status().value("loading").toBool());
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("error").toString(), QStringLiteral("fetch_failed"));
        QCOMPARE(loader.status().value("source").toString(), QStringLiteral("remote"));
        QCOMPARE(loader.status().value("snapshotUrl").toString(), oldUrl);
        QCOMPARE(loader.revision(), revision);
        QCOMPARE(loader.tokens().size(), 1);
    }

    void cacheOriginMustMatchRequest_data()
    {
        QTest::addColumn<bool>("matching");
        QTest::newRow("matching-cache") << true;
        QTest::newRow("other-origin-cache") << false;
    }

    void cacheOriginMustMatchRequest()
    {
        QFETCH(bool, matching);
        const QString url = QUrl::fromLocalFile(m_temp.filePath("offline.json")).toString();
        QVERIFY(writeCache(matching ? url : QStringLiteral("https://other.example/registry.json")));
        RegistryLoader loader;
        loader.setConfiguredUrl(url);
        loader.refresh();
        QCOMPARE(loader.status().value("source").toString(),
                 matching ? QStringLiteral("cache") : QStringLiteral("none"));
        QCOMPARE(loader.status().value("snapshotUrl").toString(), matching ? url : QString{});
        QVERIFY(loader.status().value("loading").toBool());
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("error").toString(), QStringLiteral("fetch_failed"));
        QCOMPARE(loader.tokens().size(), matching ? 1 : 0);
        QCOMPARE(loader.status().value("snapshotUrl").toString(), matching ? url : QString{});
    }

    void invalidAndEmptyRegistries_data()
    {
        QTest::addColumn<QByteArray>("body");
        QTest::addColumn<QString>("error");
        QTest::addColumn<QString>("source");
        QTest::newRow("invalid-json") << QByteArray("not json") << QString("invalid_registry") << QString("none");
        QTest::newRow("empty-networks") << QByteArray("{}") << QString("no_networks") << QString("remote");
    }

    void invalidAndEmptyRegistries()
    {
        QFETCH(QByteArray, body);
        QFETCH(QString, error);
        QFETCH(QString, source);
        const QString url = writeSource("invalid.json", body);
        QVERIFY(!url.isEmpty());
        RegistryLoader loader;
        loader.setConfiguredUrl(url);
        loader.refresh();
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("error").toString(), error);
        QCOMPARE(loader.status().value("source").toString(), source);
        QVERIFY(loader.networks().isEmpty());
    }

    void supersededReplyCannotChangeStatus()
    {
        const QString first = writeSource("first.json", registry());
        const QString second = writeSource("second.json", registry());
        QVERIFY(!first.isEmpty());
        QVERIFY(!second.isEmpty());
        RegistryLoader loader;
        loader.setConfiguredUrl(first);
        loader.refresh();
        loader.setConfiguredUrl(second);
        loader.refresh();
        QTRY_VERIFY(!loader.status().value("loading").toBool());
        QCOMPARE(loader.status().value("effectiveUrl").toString(), second);
        QCOMPARE(loader.status().value("snapshotUrl").toString(), second);
        QCOMPARE(loader.revision(), 1);
    }
};

QTEST_GUILESS_MAIN(RegistryStatusTest)
#include "RegistryStatusTest.moc"
