#pragma once

#include <optional>

#include <QString>
#include <QtGlobal>

// State shared by consumers that need to verify they are talking to a known
// sequencer. Deployment-specific configuration belongs to the consumer; this
// type only compares a supplied identity and tracks the probe lifecycle.
struct SequencerNetworkSnapshot {
    QString id;
    QString status = QStringLiteral("config_missing");
    QString fingerprint;
};

class SequencerNetworkContext final {
public:
    struct Configuration {
        QString id;
        QString expectedIdentity;
        QString fingerprintPrefix;
    };

    // Replaces the active network. Returns false and publishes config_missing
    // when the expected identity is not a 64-character lowercase hex value.
    bool configure(Configuration configuration);
    void clearConfiguration();

    bool isConfigured() const { return m_configured; }
    bool isReady() const { return m_snapshot.status == QStringLiteral("ready"); }
    bool needsIdentityProbe() const;
    const SequencerNetworkSnapshot& snapshot() const { return m_snapshot; }

    // These inputs are intentionally separate: an endpoint can be configured
    // while it is unreachable. Either loss invalidates an outstanding probe.
    void setSequencerAvailable(bool available);
    void setReachable(bool reachable);

    // A caller must retain this generation and pass it back when its async RPC
    // completes. Empty means a probe cannot currently start.
    std::optional<quint64> beginIdentityProbe();
    bool finishIdentityProbe(quint64 generation, const QString& identity);

    static bool isValidIdentity(const QString& value);

private:
    void clearIdentity(const QString& status);
    void invalidateProbe();

    SequencerNetworkSnapshot m_snapshot;
    QString m_expectedIdentity;
    QString m_fingerprintPrefix;
    quint64 m_probeGeneration = 0;
    bool m_configured = false;
    bool m_sequencerAvailable = false;
    bool m_reachable = false;
    bool m_probeInFlight = false;
};
