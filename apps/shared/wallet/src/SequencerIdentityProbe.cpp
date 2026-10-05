#include "SequencerIdentityProbe.h"

#include <algorithm>
#include <utility>

#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QTimer>
#include <QVariant>

namespace {
constexpr int REQUEST_TIMEOUT_MILLISECONDS = 4000;
constexpr int INITIAL_RETRY_DELAY_MILLISECONDS = 250;
constexpr int MAX_RETRY_DELAY_MILLISECONDS = 4000;
constexpr qsizetype CHECKPOINT_BLOCK_HASH_OFFSET = 40;
constexpr qsizetype CHECKPOINT_BLOCK_HASH_SIZE = 32;

QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
        { QStringLiteral("id"), 1 },
        { QStringLiteral("method"), method },
        { QStringLiteral("params"), params },
    }).toJson(QJsonDocument::Compact);
}

bool hasSuccessStatus(const QVariant& status)
{
    if (!status.isValid())
        return false;
    const int code = status.toInt();
    return code >= 200 && code < 300;
}
}

SequencerIdentityProbe::SequencerIdentityProbe(QObject* parent)
    : QObject(parent),
      m_network(new QNetworkAccessManager(this)),
      m_retryTimer(new QTimer(this))
{
    m_retryTimer->setSingleShot(true);
    connect(m_retryTimer, &QTimer::timeout, this, &SequencerIdentityProbe::start);
}

SequencerIdentityProbe::~SequencerIdentityProbe()
{
    cancelPendingWork();
}

bool SequencerIdentityProbe::configure(SequencerNetworkContext::Configuration network,
                                        Request request)
{
    cancelPendingWork();
    m_networkConfiguration = std::move(network);
    m_request = std::move(request);
    m_requestConfigured = isValidRequest(m_request)
        && m_context.configure(m_networkConfiguration);
    if (!m_requestConfigured) {
        if (m_context.isConfigured())
            m_context.clearConfiguration();
        emit snapshotChanged();
        return false;
    }

    updateContextAvailability();
    emit snapshotChanged();
    start();
    return true;
}

bool SequencerIdentityProbe::setEndpoint(QUrl endpoint)
{
    Request updated = m_request;
    updated.endpoint = std::move(endpoint);
    if (!m_requestConfigured || !isValidRequest(updated))
        return false;
    if (updated.endpoint == m_request.endpoint)
        return true;

    m_request.endpoint = std::move(updated.endpoint);
    restartContext();
    return true;
}

void SequencerIdentityProbe::clearConfiguration()
{
    cancelPendingWork();
    m_requestConfigured = false;
    m_request = {};
    m_networkConfiguration = {};
    m_context.clearConfiguration();
    emit snapshotChanged();
}

void SequencerIdentityProbe::setSequencerAvailable(bool available)
{
    if (m_sequencerAvailable == available)
        return;

    cancelPendingWork();
    m_sequencerAvailable = available;
    updateContextAvailability();
    emit snapshotChanged();
    start();
}

void SequencerIdentityProbe::setReachable(bool reachable)
{
    if (m_reachable == reachable)
        return;

    cancelPendingWork();
    m_reachable = reachable;
    updateContextAvailability();
    emit snapshotChanged();
    start();
}

void SequencerIdentityProbe::start()
{
    if (!m_requestConfigured
        || !isValidEndpoint(m_request.endpoint)
        || m_reply
        || m_retryTimer->isActive())
        return;

    const std::optional<quint64> contextGeneration = m_context.beginIdentityProbe();
    if (!contextGeneration)
        return;

    emit snapshotChanged();
    QNetworkRequest request(m_request.endpoint);
    request.setHeader(QNetworkRequest::ContentTypeHeader,
                      QStringLiteral("application/json"));
    request.setTransferTimeout(REQUEST_TIMEOUT_MILLISECONDS);
    QNetworkReply* reply = m_network->post(request,
                                            jsonRpcBody(m_request.method, m_request.params));
    m_reply = reply;
    const quint64 requestGeneration = m_requestGeneration;
    connect(reply, &QNetworkReply::finished, this,
            [this, reply, contextGeneration = *contextGeneration, requestGeneration]() {
        handleReply(reply, contextGeneration, requestGeneration);
    });
}

bool SequencerIdentityProbe::isValidRequest(const Request& request)
{
    return !request.method.trimmed().isEmpty()
        && static_cast<bool>(request.identityFromResult);
}

bool SequencerIdentityProbe::isValidEndpoint(const QUrl& endpoint)
{
    return endpoint.isValid()
        && (endpoint.scheme() == QStringLiteral("http")
            || endpoint.scheme() == QStringLiteral("https"))
        && !endpoint.host().isEmpty();
}

QString SequencerIdentityProbe::stringIdentity(const QJsonValue& result)
{
    return result.isString() ? result.toString() : QString();
}

QString SequencerIdentityProbe::checkpointBlockHash(const QJsonValue& result)
{
    if (!result.isString())
        return {};

    const QByteArray block = QByteArray::fromBase64(result.toString().toLatin1());
    if (block.size() < CHECKPOINT_BLOCK_HASH_OFFSET + CHECKPOINT_BLOCK_HASH_SIZE)
        return {};
    return QString::fromLatin1(
        block.mid(CHECKPOINT_BLOCK_HASH_OFFSET, CHECKPOINT_BLOCK_HASH_SIZE).toHex());
}

void SequencerIdentityProbe::restartContext()
{
    cancelPendingWork();
    if (!m_context.configure(m_networkConfiguration)) {
        m_requestConfigured = false;
        emit snapshotChanged();
        return;
    }
    updateContextAvailability();
    emit snapshotChanged();
    start();
}

void SequencerIdentityProbe::updateContextAvailability()
{
    m_context.setSequencerAvailable(m_sequencerAvailable
                                    && isValidEndpoint(m_request.endpoint));
    m_context.setReachable(m_reachable);
}

void SequencerIdentityProbe::cancelPendingWork()
{
    m_retryTimer->stop();
    m_nextRetryDelayMilliseconds = INITIAL_RETRY_DELAY_MILLISECONDS;
    ++m_requestGeneration;
    if (!m_reply)
        return;

    QNetworkReply* reply = m_reply;
    m_reply = nullptr;
    reply->abort();
    reply->deleteLater();
}

void SequencerIdentityProbe::handleReply(QNetworkReply* reply,
                                         quint64 contextGeneration,
                                         quint64 requestGeneration)
{
    if (m_reply == reply)
        m_reply = nullptr;
    if (requestGeneration != m_requestGeneration) {
        reply->deleteLater();
        return;
    }

    QString failure;
    QString identity;
    const QVariant status = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute);
    if (status.isValid() && !hasSuccessStatus(status)) {
        failure = QStringLiteral("http_status");
    } else if (reply->error() != QNetworkReply::NoError) {
        failure = QStringLiteral("transport_error");
    } else if (!hasSuccessStatus(status)) {
        failure = QStringLiteral("http_status");
    } else {
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(reply->readAll(), &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
            failure = QStringLiteral("malformed_response");
        } else {
            const QJsonObject response = document.object();
            const QJsonValue rpcError = response.value(QStringLiteral("error"));
            if ((!rpcError.isUndefined() && !rpcError.isNull())) {
                failure = QStringLiteral("json_rpc_error");
            } else {
                const QJsonValue result = response.value(QStringLiteral("result"));
                if (result.isUndefined()) {
                    failure = QStringLiteral("malformed_response");
                } else {
                    identity = m_request.identityFromResult(result);
                    if (identity.isEmpty())
                        failure = QStringLiteral("malformed_response");
                }
            }
        }
    }

    const bool accepted = m_context.finishIdentityProbe(contextGeneration, identity);
    reply->deleteLater();
    if (!accepted)
        return;

    emit snapshotChanged();
    if (!failure.isEmpty()) {
        emit probeFailed(failure);
        scheduleRetry();
    } else if (!m_context.isReady() && m_context.needsIdentityProbe()) {
        emit probeFailed(QStringLiteral("invalid_identity"));
        scheduleRetry();
    } else if (m_context.isReady()) {
        m_nextRetryDelayMilliseconds = INITIAL_RETRY_DELAY_MILLISECONDS;
    }
}

void SequencerIdentityProbe::scheduleRetry()
{
    if (!m_requestConfigured || !m_context.needsIdentityProbe() || m_retryTimer->isActive())
        return;

    const int delay = m_nextRetryDelayMilliseconds;
    m_nextRetryDelayMilliseconds = std::min(m_nextRetryDelayMilliseconds * 2,
                                            MAX_RETRY_DELAY_MILLISECONDS);
    m_retryTimer->start(delay);
}
