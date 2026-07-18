#include "AmmUiBackend.h"
#include "FakeWalletProvider.h"

#include <QDir>
#include <QFile>
#include <QHash>
#include <QHostAddress>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSettings>
#include <QTcpServer>
#include <QTcpSocket>
#include <QTemporaryDir>
#include <QtTest>

#include <memory>
#include <utility>

namespace {
class ScopedEnvironment final {
public:
    ScopedEnvironment(QByteArray name, QByteArray value)
        : m_name(std::move(name)),
          m_hadValue(qEnvironmentVariableIsSet(m_name.constData())),
          m_previous(qgetenv(m_name.constData()))
    {
        qputenv(m_name.constData(), value);
    }

    ~ScopedEnvironment()
    {
        if (m_hadValue)
            qputenv(m_name.constData(), m_previous);
        else
            qunsetenv(m_name.constData());
    }

private:
    QByteArray m_name;
    bool m_hadValue;
    QByteArray m_previous;
};

class LocalRpcServer final {
public:
    explicit LocalRpcServer(QString channelId)
        : m_channelId(std::move(channelId))
    {
        QObject::connect(&m_server, &QTcpServer::newConnection, [&]() {
            while (m_server.hasPendingConnections()) {
                QTcpSocket* socket = m_server.nextPendingConnection();
                QObject::connect(socket, &QTcpSocket::readyRead, socket,
                                 [this, socket]() { process(socket); });
                if (socket->bytesAvailable() > 0)
                    process(socket);
            }
        });
    }

    bool listen()
    {
        return m_server.listen(QHostAddress::LocalHost);
    }

    QString endpoint() const
    {
        return QStringLiteral("http://127.0.0.1:%1").arg(m_server.serverPort());
    }

private:
    void process(QTcpSocket* socket)
    {
        QByteArray& request = m_requests[socket];
        request.append(socket->readAll());
        const qsizetype headerEnd = request.indexOf("\r\n\r\n");
        if (headerEnd < 0)
            return;

        qsizetype contentLength = 0;
        for (QByteArray line : request.first(headerEnd).split('\n')) {
            line = line.trimmed();
            if (line.toLower().startsWith("content-length:")) {
                contentLength = line.mid(sizeof("content-length:") - 1)
                                    .trimmed().toLongLong();
            }
        }
        if (request.size() - headerEnd - 4 < contentLength)
            return;

        m_requests.remove(socket);
        const QByteArray payload = QJsonDocument(QJsonObject {
            { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
            { QStringLiteral("id"), 1 },
            { QStringLiteral("result"), m_channelId },
        }).toJson(QJsonDocument::Compact);
        QByteArray response = QByteArrayLiteral(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ");
        response += QByteArray::number(payload.size());
        response += QByteArrayLiteral("\r\nConnection: close\r\n\r\n");
        response += payload;
        socket->write(response);
        socket->disconnectFromHost();
    }

    QTcpServer m_server;
    QHash<QTcpSocket*, QByteArray> m_requests;
    QString m_channelId;
};

QByteArray devnetConfig(const QString& channelId,
                        const QString& ammProgramId,
                        const QString& definitionId)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("channelId"), channelId },
        { QStringLiteral("ammProgramId"), ammProgramId },
        { QStringLiteral("tokenDefinitionIds"), QJsonArray { definitionId } },
    }).toJson(QJsonDocument::Compact);
}

class BackendFixture final {
public:
    BackendFixture()
        : channelId(64, QLatin1Char('a')),
          ammProgramId(64, QLatin1Char('b')),
          definitionId(64, QLatin1Char('c')),
          tokenProgramId(64, QLatin1Char('d')),
          server(channelId)
    {
    }

    bool initialize(bool deferDefinitionReads = false)
    {
        if (!server.listen() || !directory.isValid())
            return false;
        const QString walletHome = directory.filePath(QStringLiteral("wallet"));
        if (!QDir().mkpath(walletHome))
            return false;
        const QString configPath = directory.filePath(QStringLiteral("devnet.json"));
        QFile config(configPath);
        if (!config.open(QIODevice::WriteOnly))
            return false;
        const QByteArray configData = devnetConfig(channelId, ammProgramId, definitionId);
        if (config.write(configData) != qint64(configData.size()))
            return false;
        config.close();

        network = std::make_unique<ScopedEnvironment>(
            QByteArrayLiteral("AMM_UI_NETWORK"), QByteArrayLiteral("devnet"));
        devnetFile = std::make_unique<ScopedEnvironment>(
            QByteArrayLiteral("AMM_UI_DEVNET_FILE"), configPath.toLocal8Bit());
        walletHomeEnvironment = std::make_unique<ScopedEnvironment>(
            QByteArrayLiteral("LEE_WALLET_HOME_DIR"), walletHome.toLocal8Bit());
        settingsHome = std::make_unique<ScopedEnvironment>(
            QByteArrayLiteral("XDG_CONFIG_HOME"),
            directory.filePath(QStringLiteral("settings")).toLocal8Bit());
        QSettings settings(QStringLiteral("Logos"), QStringLiteral("AmmUI"));
        settings.setValue(QStringLiteral("disconnected"), false);
        settings.sync();

        provider.connectResult.adopted = true;
        provider.connectResult.snapshot.sequencerAddress = server.endpoint();
        provider.snapshotResult = provider.connectResult.snapshot;
        provider.readResult.status = QStringLiteral("ok");
        provider.readResult.programOwner = tokenProgramId;
        provider.readResult.dataHex = QStringLiteral(
            "0004000000544553540a0000000000000000000000000000000000");
        provider.deferPublicAccountReads = deferDefinitionReads;
        backend = std::make_unique<AmmUiBackend>(provider);
        return true;
    }

    QString channelId;
    QString ammProgramId;
    QString definitionId;
    QString tokenProgramId;
    LocalRpcServer server;
    QTemporaryDir directory;
    std::unique_ptr<ScopedEnvironment> network;
    std::unique_ptr<ScopedEnvironment> devnetFile;
    std::unique_ptr<ScopedEnvironment> walletHomeEnvironment;
    std::unique_ptr<ScopedEnvironment> settingsHome;
    FakeWalletProvider provider;
    std::unique_ptr<AmmUiBackend> backend;
};
}

class AmmUiBackendDefinitionCacheTest : public QObject {
    Q_OBJECT

private slots:
    void reusesDefinitionsAfterRefreshAndReopen();
    void restartsDefinitionReadAfterRefreshAndReopen();
};

void AmmUiBackendDefinitionCacheTest::reusesDefinitionsAfterRefreshAndReopen()
{
    BackendFixture fixture;
    QVERIFY(fixture.initialize());

    QTRY_COMPARE(fixture.backend->networkStatus(), QStringLiteral("ready"));
    QTRY_COMPARE(fixture.backend->assetStatus(), QStringLiteral("ready"));
    QCOMPARE(fixture.provider.publicAccountReadCalls, 1);
    QCOMPARE(fixture.provider.lastPublicAccountIds,
             QStringList { fixture.definitionId });

    fixture.backend->refreshBalances();
    QTRY_COMPARE(fixture.backend->assetStatus(), QStringLiteral("ready"));
    QCOMPARE(fixture.provider.publicAccountReadCalls, 1);

    fixture.backend->disconnectWallet();
    QTRY_COMPARE(fixture.backend->walletSyncStatus(), QStringLiteral("closed"));
    QVERIFY(fixture.backend->openExisting());
    QTRY_COMPARE(fixture.backend->assetStatus(), QStringLiteral("ready"));
    QCOMPARE(fixture.provider.publicAccountReadCalls, 1);
}

void AmmUiBackendDefinitionCacheTest::restartsDefinitionReadAfterRefreshAndReopen()
{
    BackendFixture fixture;
    QVERIFY(fixture.initialize(true));

    QTRY_COMPARE(fixture.backend->networkStatus(), QStringLiteral("ready"));
    QTRY_COMPARE(fixture.provider.publicAccountReadCalls, 1);
    QCOMPARE(fixture.backend->assetStatus(), QStringLiteral("loading"));

    fixture.backend->refreshBalances();
    QTRY_COMPARE(fixture.provider.publicAccountReadCalls, 2);

    fixture.backend->disconnectWallet();
    QTRY_COMPARE(fixture.backend->walletSyncStatus(), QStringLiteral("closed"));
    QVERIFY(fixture.backend->openExisting());
    QTRY_COMPARE(fixture.provider.publicAccountReadCalls, 3);

    fixture.provider.completePendingPublicAccountReads();
    QTRY_COMPARE(fixture.backend->assetStatus(), QStringLiteral("ready"));
    QCOMPARE(fixture.provider.publicAccountReadCalls, 3);
}

QTEST_GUILESS_MAIN(AmmUiBackendDefinitionCacheTest)
#include "AmmUiBackendDefinitionCacheTest.moc"
