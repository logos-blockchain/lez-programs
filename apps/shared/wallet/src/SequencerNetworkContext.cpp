#include "SequencerNetworkContext.h"

#include <utility>

namespace {
bool isLowerHex(const QString& value, int size)
{
    if (value.size() != size)
        return false;
    for (const QChar character : value) {
        const bool digit = character >= QLatin1Char('0')
            && character <= QLatin1Char('9');
        if (!digit && (character < QLatin1Char('a') || character > QLatin1Char('f')))
            return false;
    }
    return true;
}
}

bool SequencerNetworkContext::configure(Configuration configuration)
{
    clearConfiguration();
    m_snapshot.id = std::move(configuration.id);
    if (!isValidIdentity(configuration.expectedIdentity))
        return false;

    m_expectedIdentity = std::move(configuration.expectedIdentity);
    m_fingerprintPrefix = std::move(configuration.fingerprintPrefix);
    m_configured = true;
    clearIdentity(QStringLiteral("network_unknown"));
    return true;
}

void SequencerNetworkContext::clearConfiguration()
{
    invalidateProbe();
    m_snapshot = {};
    m_snapshot.status = QStringLiteral("config_missing");
    m_expectedIdentity.clear();
    m_fingerprintPrefix.clear();
    m_configured = false;
    m_sequencerAvailable = false;
    m_reachable = false;
}

bool SequencerNetworkContext::needsIdentityProbe() const
{
    return m_configured
        && m_sequencerAvailable
        && m_reachable
        && !m_probeInFlight
        && (m_snapshot.status == QStringLiteral("loading")
            || m_snapshot.status == QStringLiteral("network_unknown"));
}

void SequencerNetworkContext::setSequencerAvailable(bool available)
{
    if (m_sequencerAvailable == available)
        return;

    m_sequencerAvailable = available;
    if (!m_configured)
        return;

    invalidateProbe();
    clearIdentity(available && m_reachable ? QStringLiteral("loading")
                                           : QStringLiteral("network_unknown"));
}

void SequencerNetworkContext::setReachable(bool reachable)
{
    if (m_reachable == reachable)
        return;

    m_reachable = reachable;
    if (!m_configured)
        return;

    invalidateProbe();
    clearIdentity(reachable && m_sequencerAvailable ? QStringLiteral("loading")
                                                     : QStringLiteral("network_unknown"));
}

std::optional<quint64> SequencerNetworkContext::beginIdentityProbe()
{
    if (!needsIdentityProbe())
        return std::nullopt;

    m_probeInFlight = true;
    const quint64 generation = ++m_probeGeneration;
    clearIdentity(QStringLiteral("loading"));
    return generation;
}

bool SequencerNetworkContext::finishIdentityProbe(quint64 generation,
                                                   const QString& identity)
{
    if (!m_configured
        || !m_sequencerAvailable
        || !m_reachable
        || !m_probeInFlight
        || generation != m_probeGeneration) {
        return false;
    }

    m_probeInFlight = false;
    if (!isValidIdentity(identity)) {
        clearIdentity(QStringLiteral("network_unknown"));
    } else if (identity != m_expectedIdentity) {
        clearIdentity(QStringLiteral("network_mismatch"));
    } else {
        m_snapshot.status = QStringLiteral("ready");
        m_snapshot.fingerprint = m_fingerprintPrefix + identity;
    }
    return true;
}

bool SequencerNetworkContext::isValidIdentity(const QString& value)
{
    return isLowerHex(value, 64);
}

void SequencerNetworkContext::clearIdentity(const QString& status)
{
    m_snapshot.status = status;
    m_snapshot.fingerprint.clear();
}

void SequencerNetworkContext::invalidateProbe()
{
    ++m_probeGeneration;
    m_probeInFlight = false;
}
