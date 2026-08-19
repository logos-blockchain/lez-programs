#include "SequencerIdentityProbe.h"

#include <QHostAddress>
#include <QHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QList>
#include <QPointer>
#include <QQueue>
#include <QSignalSpy>
#include <QTcpServer>
#include <QTcpSocket>
#include <QUrl>
#include <QtTest>

#include <utility>

namespace {
const QString IDENTITY(64, QLatin1Char('a'));
const QString OTHER_IDENTITY(64, QLatin1Char('b'));

SequencerNetworkContext::Configuration networkConfiguration()
{
    return {
        QStringLiteral("testnet"),
        IDENTITY,
        QStringLiteral("checkpoint:"),
    };
}

QByteArray jsonRpcResult(const QString& identity)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
        { QStringLiteral("id"), 1 },
        { QStringLiteral("result"), identity },
    }).toJson(QJsonDocument::Compact);
}

class RpcServer final : public QObject {
public:
    RpcServer()
    {
        m_server.listen(QHostAddress::LocalHost);
        connect(&m_server, &QTcpServer::newConnection, this, [this]() {
            while (QTcpSocket* socket = m_server.nextPendingConnection())
                attach(socket);
        });
    }

    QUrl endpoint() const
    {
        return QUrl(QStringLiteral("http://127.0.0.1:%1").arg(m_server.serverPort()));
    }

    bool isListening() const { return m_server.isListening(); }

    void enqueueResponse(int status, QByteArray body)
    {
        m_responses.enqueue({ status, std::move(body) });
    }

    void holdNextResponse()
    {
        m_responses.enqueue({ 0, {} });
    }

    void respondHeld(int status, QByteArray body)
    {
        while (!m_held.isEmpty()) {
            const QPointer<QTcpSocket> socket = m_held.dequeue();
            if (socket)
                sendResponse(socket, status, std::move(body));
            return;
        }
    }

    int requestCount() const { return m_requests.size(); }
    QByteArray lastRequest() const { return m_requests.isEmpty() ? QByteArray() : m_requests.last(); }

private:
    struct Response {
        int status;
        QByteArray body;
    };

    void attach(QTcpSocket* socket)
    {
        socket->setParent(this);
        connect(socket, &QTcpSocket::readyRead, this, [this, socket]() {
            QByteArray& request = m_partialRequests[socket];
            request.append(socket->readAll());
            const qsizetype headerEnd = request.indexOf("\r\n\r\n");
            if (headerEnd < 0)
                return;

            const QByteArray headers = request.left(headerEnd);
            qsizetype contentLength = 0;
            for (const QByteArray& line : headers.split('\n')) {
                const qsizetype separator = line.indexOf(':');
                if (separator < 0)
                    continue;
                if (line.left(separator).trimmed().compare("content-length",
                                                           Qt::CaseInsensitive) == 0) {
                    contentLength = line.mid(separator + 1).trimmed().toLongLong();
                    break;
                }
            }
            if (request.size() < headerEnd + 4 + contentLength)
                return;

            m_requests.append(request);
            m_partialRequests.remove(socket);
            if (m_responses.isEmpty()) {
                m_held.enqueue(socket);
                return;
            }

            const Response response = m_responses.dequeue();
            if (response.status == 0) {
                m_held.enqueue(socket);
                return;
            }
            sendResponse(socket, response.status, response.body);
        });
        connect(socket, &QTcpSocket::disconnected, socket, &QObject::deleteLater);
    }

    static void sendResponse(QTcpSocket* socket, int status, QByteArray body)
    {
        const QByteArray statusText = status >= 200 && status < 300
            ? QByteArrayLiteral("OK") : QByteArrayLiteral("Service Unavailable");
        QByteArray response = QByteArrayLiteral("HTTP/1.1 ") + QByteArray::number(status)
            + QByteArrayLiteral(" ") + statusText
            + QByteArrayLiteral("\r\nContent-Type: application/json\r\nContent-Length: ")
            + QByteArray::number(body.size())
            + QByteArrayLiteral("\r\nConnection: close\r\n\r\n") + body;
        socket->write(response);
        socket->flush();
        socket->disconnectFromHost();
    }

    QTcpServer m_server;
    QHash<QTcpSocket*, QByteArray> m_partialRequests;
    QQueue<Response> m_responses;
    QQueue<QPointer<QTcpSocket>> m_held;
    QList<QByteArray> m_requests;
};

SequencerIdentityProbe::Request requestFor(const QUrl& endpoint)
{
    return {
        endpoint,
        QStringLiteral("getChannelId"),
        QJsonArray { 10 },
        SequencerIdentityProbe::stringIdentity,
    };
}
}

class SequencerIdentityProbeTest final : public QObject {
    Q_OBJECT

private slots:
    void sendsConfiguredRequestAndAcceptsIdentity();
    void waitsForEndpointBeforeProbing();
    void retriesRejectedResponses_data();
    void retriesRejectedResponses();
    void supersedesEndpointReply();
    void abortsReplyWhenReachabilityIsLost();
    void retriesTransportFailureAfterEndpointChanges();
    void extractsCheckpointBlockHash();
};

void SequencerIdentityProbeTest::sendsConfiguredRequestAndAcceptsIdentity()
{
    RpcServer server;
    QVERIFY(server.isListening());
    server.enqueueResponse(200, jsonRpcResult(IDENTITY));
    SequencerIdentityProbe probe;

    QVERIFY(probe.configure(networkConfiguration(), requestFor(server.endpoint())));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);

    QTRY_COMPARE(probe.snapshot().status, QStringLiteral("ready"));
    QCOMPARE(probe.snapshot().fingerprint, QStringLiteral("checkpoint:") + IDENTITY);
    QCOMPARE(server.requestCount(), 1);
    QVERIFY(server.lastRequest().contains("\"method\":\"getChannelId\""));
    QVERIFY(server.lastRequest().contains("\"params\":[10]"));
}

void SequencerIdentityProbeTest::waitsForEndpointBeforeProbing()
{
    RpcServer server;
    QVERIFY(server.isListening());
    server.enqueueResponse(200, jsonRpcResult(IDENTITY));
    SequencerIdentityProbe probe;

    QVERIFY(probe.configure(networkConfiguration(), requestFor(QUrl())));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);
    QCOMPARE(probe.snapshot().status, QStringLiteral("network_unknown"));
    QCOMPARE(server.requestCount(), 0);

    QVERIFY(probe.setEndpoint(server.endpoint()));
    QTRY_COMPARE(probe.snapshot().status, QStringLiteral("ready"));
    QCOMPARE(server.requestCount(), 1);
}

void SequencerIdentityProbeTest::retriesRejectedResponses_data()
{
    QTest::addColumn<int>("status");
    QTest::addColumn<QByteArray>("body");
    QTest::addColumn<QString>("failure");

    QTest::newRow("http") << 503 << QByteArrayLiteral("{}")
                           << QStringLiteral("http_status");
    QTest::newRow("malformed") << 200 << QByteArrayLiteral("not-json")
                                << QStringLiteral("malformed_response");
    QTest::newRow("json-rpc-error") << 200
        << QByteArrayLiteral("{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-1}}")
        << QStringLiteral("json_rpc_error");
}

void SequencerIdentityProbeTest::retriesRejectedResponses()
{
    QFETCH(int, status);
    QFETCH(QByteArray, body);
    QFETCH(QString, failure);
    RpcServer server;
    QVERIFY(server.isListening());
    server.enqueueResponse(status, body);
    server.enqueueResponse(200, jsonRpcResult(IDENTITY));
    SequencerIdentityProbe probe;
    QSignalSpy failures(&probe, &SequencerIdentityProbe::probeFailed);

    QVERIFY(probe.configure(networkConfiguration(), requestFor(server.endpoint())));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);

    QTRY_COMPARE(server.requestCount(), 2);
    QTRY_COMPARE(probe.snapshot().status, QStringLiteral("ready"));
    QVERIFY(!failures.isEmpty());
    QCOMPARE(failures.first().at(0).toString(), failure);
}

void SequencerIdentityProbeTest::supersedesEndpointReply()
{
    RpcServer first;
    QVERIFY(first.isListening());
    first.holdNextResponse();
    RpcServer second;
    QVERIFY(second.isListening());
    second.enqueueResponse(200, jsonRpcResult(IDENTITY));
    SequencerIdentityProbe probe;

    QVERIFY(probe.configure(networkConfiguration(), requestFor(first.endpoint())));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);
    QTRY_COMPARE(first.requestCount(), 1);

    QVERIFY(probe.setEndpoint(second.endpoint()));
    QTRY_COMPARE(second.requestCount(), 1);
    QTRY_COMPARE(probe.snapshot().status, QStringLiteral("ready"));

    first.respondHeld(200, jsonRpcResult(OTHER_IDENTITY));
    QTest::qWait(50);
    QCOMPARE(probe.snapshot().status, QStringLiteral("ready"));
}

void SequencerIdentityProbeTest::abortsReplyWhenReachabilityIsLost()
{
    RpcServer server;
    QVERIFY(server.isListening());
    server.holdNextResponse();
    SequencerIdentityProbe probe;

    QVERIFY(probe.configure(networkConfiguration(), requestFor(server.endpoint())));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);
    QTRY_COMPARE(server.requestCount(), 1);

    probe.setReachable(false);
    server.respondHeld(200, jsonRpcResult(IDENTITY));
    QTest::qWait(50);
    QCOMPARE(probe.snapshot().status, QStringLiteral("network_unknown"));
}

void SequencerIdentityProbeTest::retriesTransportFailureAfterEndpointChanges()
{
    QTcpServer unavailable;
    QVERIFY(unavailable.listen(QHostAddress::LocalHost));
    const QUrl unavailableEndpoint(
        QStringLiteral("http://127.0.0.1:%1").arg(unavailable.serverPort()));
    unavailable.close();

    RpcServer server;
    QVERIFY(server.isListening());
    server.enqueueResponse(200, jsonRpcResult(IDENTITY));
    SequencerIdentityProbe probe;
    QSignalSpy failures(&probe, &SequencerIdentityProbe::probeFailed);

    QVERIFY(probe.configure(networkConfiguration(), requestFor(unavailableEndpoint)));
    probe.setSequencerAvailable(true);
    probe.setReachable(true);
    QTRY_VERIFY(!failures.isEmpty());
    QCOMPARE(failures.first().at(0).toString(), QStringLiteral("transport_error"));

    QVERIFY(probe.setEndpoint(server.endpoint()));
    QTRY_COMPARE(probe.snapshot().status, QStringLiteral("ready"));
}

void SequencerIdentityProbeTest::extractsCheckpointBlockHash()
{
    QByteArray block(72, '\0');
    const QByteArray expected(32, static_cast<char>(0xab));
    block.replace(40, expected.size(), expected);

    QCOMPARE(SequencerIdentityProbe::checkpointBlockHash(
                 QJsonValue(QString::fromLatin1(block.toBase64()))),
             QString::fromLatin1(expected.toHex()));
    QVERIFY(SequencerIdentityProbe::checkpointBlockHash(QJsonValue(QStringLiteral("bad"))).isEmpty());
}

QTEST_MAIN(SequencerIdentityProbeTest)

#include "SequencerIdentityProbeTest.moc"
