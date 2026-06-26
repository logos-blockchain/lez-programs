#pragma once

#include <QString>
#include <QStringList>

struct ActiveNetworkSnapshot {
    QString id;
    QString status;
    QString fingerprint;
    QString ammProgramId;
    QStringList tokenIds;
};

class ActiveNetwork final {
public:
    bool load();

    const QString& status() const { return m_network.status; }
    bool isConfigured() const;
    bool isDevnet() const;
    bool needsIdentityProbe() const;
    ActiveNetworkSnapshot snapshot() const { return m_network; }

    void sequencerChanged(bool available);
    void reachabilityChanged(bool reachable, bool wasReachable);
    void beginIdentityProbe();
    void finishIdentityProbe(const QString& identity);

    static bool isValidIdentity(const QString& value);

private:
    void clearIdentity(const QString& status);

    ActiveNetworkSnapshot m_network;
    QString m_expectedIdentity;
};
