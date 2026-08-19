#pragma once

#include <functional>
#include <optional>

#include <QJsonArray>
#include <QJsonValue>
#include <QObject>
#include <QString>
#include <QUrl>

#include "SequencerNetworkContext.h"

class QNetworkAccessManager;
class QNetworkReply;
class QTimer;

// Owns the JSON-RPC lifecycle used to prove that a wallet endpoint belongs to
// a configured network. Consumers supply only their RPC method, parameters,
// and the protocol-specific extraction of an identity from `result`.
class SequencerIdentityProbe final : public QObject {
    Q_OBJECT

public:
    using IdentityParser = std::function<QString(const QJsonValue& result)>;

    struct Request {
        QUrl endpoint;
        QString method;
        QJsonArray params;
        IdentityParser identityFromResult;
    };

    explicit SequencerIdentityProbe(QObject* parent = nullptr);
    ~SequencerIdentityProbe() override;

    // Replaces both the expected network identity and RPC request. Existing
    // replies are aborted before the new context can issue a probe.
    bool configure(SequencerNetworkContext::Configuration network, Request request);

    // An endpoint change invalidates the previous identity result, including a
    // reply that may already be in flight. An invalid/empty endpoint leaves the
    // configured network in network_unknown until a valid endpoint is supplied.
    bool setEndpoint(QUrl endpoint);

    void clearConfiguration();
    void setSequencerAvailable(bool available);
    void setReachable(bool reachable);

    // Safe to call after every wallet/network state update. It starts a probe
    // only when the context currently needs one and no retry is pending.
    void start();

    const SequencerNetworkSnapshot& snapshot() const { return m_context.snapshot(); }

    // Common JSON-RPC result parsers. `checkpointBlockHash` decodes the
    // fixed-layout base64 block response used by checkpoint probes.
    static QString stringIdentity(const QJsonValue& result);
    static QString checkpointBlockHash(const QJsonValue& result);

signals:
    // Emitted whenever the externally visible network state changes.
    void snapshotChanged();
    // A transient RPC failure was rejected and a retry may be scheduled.
    void probeFailed(const QString& reason);

private:
    static bool isValidRequest(const Request& request);
    static bool isValidEndpoint(const QUrl& endpoint);

    void restartContext();
    void updateContextAvailability();
    void cancelPendingWork();
    void handleReply(QNetworkReply* reply, quint64 contextGeneration,
                     quint64 requestGeneration);
    void scheduleRetry();

    SequencerNetworkContext m_context;
    SequencerNetworkContext::Configuration m_networkConfiguration;
    Request m_request;
    QNetworkAccessManager* m_network;
    QNetworkReply* m_reply = nullptr;
    QTimer* m_retryTimer;
    quint64 m_requestGeneration = 0;
    int m_nextRetryDelayMilliseconds = 250;
    bool m_requestConfigured = false;
    bool m_sequencerAvailable = false;
    bool m_reachable = false;
};
