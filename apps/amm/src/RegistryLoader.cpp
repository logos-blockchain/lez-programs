#include "RegistryLoader.h"

#include <QByteArray>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QIODevice>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QStandardPaths>
#include <QString>
#include <QVariantMap>

namespace {
    // Local-file source (dev / local-sequencer). Bare `[...]` arrays; takes
    // precedence over the remote registry when either is set.
    constexpr char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";
    constexpr char POOLS_CONFIG_ENV[] = "AMM_POOLS_CONFIG";
    // Remote source: the URL of a single multi-network registry document.
    constexpr char REGISTRY_URL_ENV[] = "AMM_REGISTRY_URL";
    // Optional: force the active network by id (else it is inferred, see
    // RegistryLoader::selectActiveNetwork).
    constexpr char NETWORK_ENV[] = "AMM_NETWORK";

    // Parses a tokens array into the QVariantList the Swap token picker renders,
    // keeping only entries for `networkFilter` (empty ⇒ keep all, for local files
    // which carry no network tag). Fail-soft: one malformed entry is skipped.
    // symbol/name are display; definitionId is the token's account id and passes
    // through as configured (base58 or hex). `holding` is per-wallet and absent
    // from a shared registry — the app resolves it — so only definitionId is
    // required. `decimals` is optional (the app doesn't use it yet); absent ⇒ 0.
    QVariantList parseTokens(const QJsonArray& arr, const QString& networkFilter)
    {
        QVariantList out;
        for (const QJsonValue& entry : arr) {
            if (!entry.isObject())
                continue;
            const QJsonObject obj = entry.toObject();
            if (!networkFilter.isEmpty()
                && obj.value(QStringLiteral("network")).toString() != networkFilter)
                continue;

            const QString definitionId = obj.value(QStringLiteral("definitionId")).toString();
            const QJsonValue decimals = obj.value(QStringLiteral("decimals"));
            if (definitionId.isEmpty())
                continue;

            QVariantMap token;
            token.insert(QStringLiteral("symbol"), obj.value(QStringLiteral("symbol")).toString());
            token.insert(QStringLiteral("name"), obj.value(QStringLiteral("name")).toString());
            token.insert(QStringLiteral("definitionId"), definitionId);
            token.insert(QStringLiteral("holding"), obj.value(QStringLiteral("holding")).toString());
            token.insert(QStringLiteral("decimals"), decimals.toInt());
            out.append(token);
        }
        return out;
    }

    // Parses a pools array into the QVariantList the Pools UI renders, keeping
    // only entries for `networkFilter` (empty ⇒ keep all). tokenA/tokenB (display
    // symbols) and a numeric feeBps are required; the id fields pass through when
    // present so the entry can be resolved on-chain.
    QVariantList parsePools(const QJsonArray& arr, const QString& networkFilter)
    {
        QVariantList out;
        for (const QJsonValue& entry : arr) {
            if (!entry.isObject())
                continue;
            const QJsonObject obj = entry.toObject();
            if (!networkFilter.isEmpty()
                && obj.value(QStringLiteral("network")).toString() != networkFilter)
                continue;

            const QString tokenA = obj.value(QStringLiteral("tokenA")).toString();
            const QString tokenB = obj.value(QStringLiteral("tokenB")).toString();
            const QJsonValue feeBps = obj.value(QStringLiteral("feeBps"));
            if (tokenA.isEmpty() || tokenB.isEmpty() || !feeBps.isDouble())
                continue;

            QVariantMap pool;
            pool.insert(QStringLiteral("tokenA"), tokenA);
            pool.insert(QStringLiteral("tokenB"), tokenB);
            pool.insert(QStringLiteral("feeBps"), feeBps.toInt());
            pool.insert(QStringLiteral("poolId"),
                        obj.value(QStringLiteral("poolId")).toString());
            pool.insert(QStringLiteral("tokenADefinitionId"),
                        obj.value(QStringLiteral("tokenADefinitionId")).toString());
            pool.insert(QStringLiteral("tokenBDefinitionId"),
                        obj.value(QStringLiteral("tokenBDefinitionId")).toString());
            out.append(pool);
        }
        return out;
    }

    QByteArray readConfigFileBytes(const char* envVar)
    {
        const QString path = qEnvironmentVariable(envVar);
        if (path.isEmpty())
            return {};
        QFile file(path);
        if (!file.open(QIODevice::ReadOnly | QIODevice::Text))
            return {};
        return file.readAll();
    }

    QJsonArray jsonArrayFromBytes(const QByteArray& bytes)
    {
        const QJsonDocument doc = QJsonDocument::fromJson(bytes);
        return doc.isArray() ? doc.array() : QJsonArray{};
    }
}

RegistryLoader::RegistryLoader(QObject* parent)
    : QObject(parent)
{
}

bool RegistryLoader::hasLocalSource()
{
    return !qEnvironmentVariableIsEmpty(TOKENS_CONFIG_ENV)
        || !qEnvironmentVariableIsEmpty(POOLS_CONFIG_ENV);
}

void RegistryLoader::refresh()
{
    // Supersede any in-flight remote fetch.
    ++m_generation;

    // No adopted network id until applyRegistry selects one; the local / none paths
    // below carry none, so ops fall back to AMM_PROGRAM_BIN.
    m_activeAmmProgramId.clear();

    // local-replaces-remote: a configured local file wins outright.
    if (hasLocalSource()) {
        loadLocal();
        return;
    }

    const QString url = qEnvironmentVariable(REGISTRY_URL_ENV);
    if (url.isEmpty()) {
        publish({}, {}, QStringLiteral("none"), {});
        return;
    }

    // stale-while-revalidate: serve the on-disk cache immediately when we have
    // nothing yet, then revalidate against the network below.
    if (m_tokens.isEmpty() && m_pools.isEmpty())
        loadDiskCache(url);

    startRemote(QUrl(url));
}

void RegistryLoader::loadLocal()
{
    // Local files are bare arrays with no network tag — no filtering.
    publish(parseTokens(jsonArrayFromBytes(readConfigFileBytes(TOKENS_CONFIG_ENV)), {}),
            parsePools(jsonArrayFromBytes(readConfigFileBytes(POOLS_CONFIG_ENV)), {}),
            QStringLiteral("local"), {});
}

void RegistryLoader::startRemote(const QUrl& url)
{
    const quint64 generation = m_generation;
    QNetworkReply* reply = nam()->get(QNetworkRequest(url));
    connect(reply, &QNetworkReply::finished, this, [this, reply, generation]() {
        reply->deleteLater();
        if (generation != m_generation)
            return;  // superseded by a newer refresh
        if (reply->error() != QNetworkReply::NoError) {
            qWarning() << "AMM registry: fetch failed:" << reply->errorString();
            return;  // keep serving whatever we have (cache / previous)
        }

        const QByteArray body = reply->readAll();
        const QString stamp = QJsonDocument::fromJson(body)
                                  .object()
                                  .value(QStringLiteral("timestamp"))
                                  .toVariant()
                                  .toString();
        // Revalidation: an unchanged registry with a non-empty snapshot is
        // already current.
        if (!stamp.isEmpty() && stamp == m_stamp && !m_tokens.isEmpty())
            return;

        if (applyRegistry(body, QStringLiteral("remote"))) {
            m_stamp = stamp;
            saveDiskCache(qEnvironmentVariable(REGISTRY_URL_ENV), stamp, body);
        }
    });
}

bool RegistryLoader::applyRegistry(const QByteArray& body, const QString& source)
{
    const QJsonDocument doc = QJsonDocument::fromJson(body);
    if (!doc.isObject()) {
        qWarning() << "AMM registry: document is not a JSON object";
        return false;
    }
    const QJsonObject registry = doc.object();
    const QJsonArray networks = registry.value(QStringLiteral("networks")).toArray();

    const QString activeId = selectActiveNetwork(networks);
    if (activeId.isEmpty()) {
        qWarning() << "AMM registry: cannot determine the active network"
                      " (set AMM_NETWORK for a multi-network registry); not applied";
        return false;
    }

    // Adopt the active network's declared AMM program id so the backend can point
    // ops at it (setAmmProgramId) without an AMM_PROGRAM_BIN.
    m_activeAmmProgramId.clear();
    for (const QJsonValue& entry : networks) {
        const QJsonObject net = entry.toObject();
        if (net.value(QStringLiteral("id")).toString() == activeId) {
            m_activeAmmProgramId = net.value(QStringLiteral("programIds"))
                                       .toObject()
                                       .value(QStringLiteral("amm"))
                                       .toString();
            break;
        }
    }

    publish(parseTokens(registry.value(QStringLiteral("tokens")).toArray(), activeId),
            parsePools(registry.value(QStringLiteral("pools")).toArray(), activeId),
            source, activeId);
    return true;
}

QString RegistryLoader::selectActiveNetwork(const QJsonArray& networks) const
{
    // Explicit override wins if it names a declared network.
    const QString forced = qEnvironmentVariable(NETWORK_ENV);
    if (!forced.isEmpty()) {
        for (const QJsonValue& entry : networks) {
            if (entry.toObject().value(QStringLiteral("id")).toString() == forced)
                return forced;
        }
        return {};
    }

    // A single declared network is unambiguous. Multiple networks can't be told
    // apart from the connection (program ids and account ids are deterministic and
    // may be identical across networks), so AMM_NETWORK is required to pick one.
    if (networks.size() == 1)
        return networks.at(0).toObject().value(QStringLiteral("id")).toString();

    return {};
}

void RegistryLoader::publish(const QVariantList& tokens, const QVariantList& pools,
                             const QString& source, const QString& network)
{
    m_tokens = tokens;
    m_pools = pools;
    m_source = source;
    m_activeNetwork = network;
    ++m_revision;
    emit changed();
}

void RegistryLoader::loadDiskCache(const QString& url)
{
    QFile file(cachePath());
    if (!file.open(QIODevice::ReadOnly))
        return;
    const QJsonObject obj = QJsonDocument::fromJson(file.readAll()).object();
    // Only trust a cache written for this same source URL.
    if (obj.value(QStringLiteral("url")).toString() != url)
        return;

    m_stamp = obj.value(QStringLiteral("stamp")).toVariant().toString();
    // Re-apply the cached registry against the current connection (the active
    // network may resolve differently than when it was written).
    const QByteArray body =
        QJsonDocument(obj.value(QStringLiteral("registry")).toObject()).toJson(QJsonDocument::Compact);
    applyRegistry(body, QStringLiteral("cache"));
}

void RegistryLoader::saveDiskCache(const QString& url, const QString& stamp,
                                   const QByteArray& body) const
{
    const QString path = cachePath();
    QDir().mkpath(QFileInfo(path).absolutePath());

    QJsonObject obj;
    obj.insert(QStringLiteral("url"), url);
    obj.insert(QStringLiteral("stamp"), stamp);
    obj.insert(QStringLiteral("registry"), QJsonDocument::fromJson(body).object());

    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return;
    file.write(QJsonDocument(obj).toJson(QJsonDocument::Compact));
}

QString RegistryLoader::cachePath()
{
    return QStandardPaths::writableLocation(QStandardPaths::AppDataLocation)
        + QStringLiteral("/amm-registry-cache.json");
}

QNetworkAccessManager* RegistryLoader::nam()
{
    if (!m_nam)
        m_nam = new QNetworkAccessManager(this);
    return m_nam;
}
