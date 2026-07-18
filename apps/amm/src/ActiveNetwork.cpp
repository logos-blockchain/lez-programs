#include "ActiveNetwork.h"

#include <algorithm>
#include <iterator>

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

namespace {
    const char NETWORK_ENV[] = "AMM_UI_NETWORK";
    const char DEVNET_FILE_ENV[] = "AMM_UI_DEVNET_FILE";

    bool isLowerHex(const QString& value, int size)
    {
        if (value.size() != size)
            return false;
        for (const QChar character : value) {
            const bool isAsciiDigit = character >= QLatin1Char('0')
                && character <= QLatin1Char('9');
            if (!isAsciiDigit
                && (character < QLatin1Char('a') || character > QLatin1Char('f'))) {
                return false;
            }
        }
        return true;
    }
}

bool ActiveNetwork::load()
{
    m_network = {};
    m_network.status = QStringLiteral("config_missing");
    m_expectedIdentity.clear();
    m_failedIdentityProbes = 0;

    const QByteArray selected = qgetenv(NETWORK_ENV);
    m_network.id = selected.isEmpty()
        ? QStringLiteral("testnet")
        : QString::fromLocal8Bit(selected).trimmed();

    QJsonObject entry;
    if (isDevnet()) {
        const QString path = QString::fromLocal8Bit(qgetenv(DEVNET_FILE_ENV));
        QFile file(path);
        if (path.isEmpty() || !file.open(QIODevice::ReadOnly))
            return false;
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
        if (!document.isObject())
            return false;
        entry = document.object();
        m_expectedIdentity = entry.value(QStringLiteral("channelId")).toString();
    } else {
        QFile file(QStringLiteral(":/amm/config/networks.json"));
        if (!file.open(QIODevice::ReadOnly))
            return false;
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
        if (!document.isObject())
            return false;
        const QJsonObject networks = document.object();
        entry = networks.value(m_network.id).toObject();
        m_expectedIdentity = entry.value(QStringLiteral("checkpointHash")).toString();
    }

    m_network.sequencerAddress = entry.value(QStringLiteral("sequencerAddress")).toString();
    m_network.ammProgramId = entry.value(QStringLiteral("ammProgramId")).toString();
    if (!isValidIdentity(m_expectedIdentity)
        || !isLowerHex(m_network.ammProgramId, 64)) {
        return false;
    }

    for (const QJsonValue& value : entry.value(QStringLiteral("tokenDefinitionIds")).toArray()) {
        const QString id = value.toString();
        if (!isLowerHex(id, 64)) {
            m_network.tokenIds.clear();
            return false;
        }
        m_network.tokenIds.append(id);
    }
    m_network.status = QStringLiteral("network_unknown");
    return true;
}

bool ActiveNetwork::isConfigured() const
{
    return m_network.status != QStringLiteral("config_missing");
}

bool ActiveNetwork::isDevnet() const
{
    return m_network.id == QStringLiteral("devnet");
}

bool ActiveNetwork::needsIdentityProbe() const
{
    return m_network.status == QStringLiteral("loading")
        || m_network.status == QStringLiteral("network_unknown");
}

int ActiveNetwork::identityRetryDelayMs() const
{
    static constexpr int retryDelays[] = {
        1000,
        2000,
        4000,
        8000,
        16000,
        30000,
    };
    if (m_failedIdentityProbes <= 0
        || m_network.status != QStringLiteral("network_unknown")) {
        return 0;
    }
    const int index = (std::min)(m_failedIdentityProbes,
                                 static_cast<int>(std::size(retryDelays))) - 1;
    return retryDelays[index];
}

void ActiveNetwork::sequencerChanged(bool available)
{
    m_failedIdentityProbes = 0;
    if (isConfigured())
        clearIdentity(available ? QStringLiteral("loading") : QStringLiteral("network_unknown"));
}

void ActiveNetwork::reachabilityChanged(bool reachable, bool wasReachable)
{
    if (!isConfigured())
        return;
    if (!reachable || !wasReachable)
        m_failedIdentityProbes = 0;
    if (!reachable)
        clearIdentity(QStringLiteral("network_unknown"));
    else if (!wasReachable)
        clearIdentity(QStringLiteral("loading"));
}

void ActiveNetwork::beginIdentityProbe()
{
    if (isConfigured())
        clearIdentity(QStringLiteral("loading"));
}

void ActiveNetwork::finishIdentityProbe(const QString& identity)
{
    if (identity.isEmpty()) {
        m_failedIdentityProbes = (std::min)(m_failedIdentityProbes + 1, 6);
        clearIdentity(QStringLiteral("network_unknown"));
    } else if (identity != m_expectedIdentity) {
        m_failedIdentityProbes = 0;
        clearIdentity(QStringLiteral("network_mismatch"));
    } else {
        m_failedIdentityProbes = 0;
        m_network.status = QStringLiteral("ready");
        m_network.fingerprint = (isDevnet() ? QStringLiteral("channel:")
                                            : QStringLiteral("block10:"))
            + identity;
    }
}

bool ActiveNetwork::isValidIdentity(const QString& value)
{
    return isLowerHex(value, 64);
}

void ActiveNetwork::clearIdentity(const QString& status)
{
    m_network.status = status;
    m_network.fingerprint.clear();
}
