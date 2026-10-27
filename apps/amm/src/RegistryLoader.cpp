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
#include <QStringList>
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
    // symbols) are required; the id fields pass through when present so the entry
    // can be resolved on-chain. The swap fee comes from the AMM config.
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
            // The swap fee is instance-wide (AMM config), no longer a pool field, so it is
            // not required here. Only the pair + on-chain ids identify the pool.
            if (tokenA.isEmpty() || tokenB.isEmpty())
                continue;

            QVariantMap pool;
            pool.insert(QStringLiteral("tokenA"), tokenA);
            pool.insert(QStringLiteral("tokenB"), tokenB);
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

    QByteArray readConfigFileBytes(const QString& path)
    {
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

// The environment is read once, here: these variables pick the source and its ids, so
// letting a later setenv() in the host process swap them on the next refresh() would
// let it replace the token list or the AMM instance under the user's configured URL.
RegistryLoader::RegistryLoader(QObject* parent)
    : QObject(parent)
    , m_tokensConfigPath(qEnvironmentVariable(TOKENS_CONFIG_ENV))
    , m_poolsConfigPath(qEnvironmentVariable(POOLS_CONFIG_ENV))
    , m_envRegistryUrl(qEnvironmentVariable(REGISTRY_URL_ENV))
    , m_envNetwork(qEnvironmentVariable(NETWORK_ENV))
{
}

bool RegistryLoader::hasLocalSource() const
{
    return !m_tokensConfigPath.isEmpty() || !m_poolsConfigPath.isEmpty();
}

QVariantMap RegistryLoader::status() const
{
    return {
        {QStringLiteral("source"), m_source},
        {QStringLiteral("effectiveUrl"), m_effectiveUrl},
        {QStringLiteral("snapshotUrl"), m_snapshotUrl},
        {QStringLiteral("override"), m_override},
        {QStringLiteral("loading"), m_loading},
        {QStringLiteral("error"), m_fetchError},
    };
}

void RegistryLoader::refresh()
{
    // Supersede any in-flight remote fetch.
    ++m_generation;
    m_loading = false;
    m_fetchError.clear();
    m_effectiveUrl.clear();
    m_override.clear();

    // No adopted instance until applyRegistry selects one; the local / none paths
    // below carry neither id, so ops fall back to AMM_PROGRAM_ID / AMM_CONFIG_ID.
    m_activeAmmProgramId.clear();
    m_activeAmmConfigId.clear();

    // AMM_REGISTRY_URL (e2e / dev) overrides the UI-configured URL.
    const bool envUrl = !m_envRegistryUrl.isEmpty();
    QString url = m_envRegistryUrl;
    if (url.isEmpty())
        url = m_configuredUrl;

    // local-replaces-remote: a configured local file wins outright.
    if (hasLocalSource()) {
        // The local files carry token/pool lists ONLY -- no programIds, no ammConfigId.
        // So taking this path also drops the ids, leaving ops to fall back to
        // AMM_PROGRAM_ID. That is easy to hit by accident, because a token list looks
        // orthogonal to a program address, and the resulting failure surfaces far away
        // as a config that cannot be read. Say so rather than silently winning.
        if (!url.isEmpty()) {
            qWarning().noquote()
                << "AMM registry:" << REGISTRY_URL_ENV << "/ configured URL is set (" << url
                << ") but IGNORED because" << TOKENS_CONFIG_ENV << "or" << POOLS_CONFIG_ENV
                << "is set; local files take precedence and carry no program/config ids."
                << "Unset them to use the registry.";
        }
        QStringList overrides;
        if (!m_tokensConfigPath.isEmpty())
            overrides.append(QString::fromLatin1(TOKENS_CONFIG_ENV));
        if (!m_poolsConfigPath.isEmpty())
            overrides.append(QString::fromLatin1(POOLS_CONFIG_ENV));
        m_override = overrides.join(QStringLiteral(", "));
        m_snapshotUrl.clear();
        loadLocal();
        return;
    }

    if (envUrl)
        m_override = QString::fromLatin1(REGISTRY_URL_ENV);
    m_effectiveUrl = url;
    if (url.isEmpty()) {
        m_snapshotUrl.clear();
        m_registryObj = {};  // no registry ⇒ no networks to pick
        publish({}, {}, QStringLiteral("none"), {});
        return;
    }

    m_loading = true;

    // stale-while-revalidate: serve the on-disk cache immediately when we have
    // nothing yet, then revalidate against the network below.
    if (m_tokens.isEmpty() && m_pools.isEmpty())
        loadDiskCache(url);

    emit statusChanged();
    startRemote(QUrl(url));
}

void RegistryLoader::loadLocal()
{
    m_registryObj = {};  // local files carry no networks to pick
    // Local files are bare arrays with no network tag — no filtering.
    publish(parseTokens(jsonArrayFromBytes(readConfigFileBytes(m_tokensConfigPath)), {}),
            parsePools(jsonArrayFromBytes(readConfigFileBytes(m_poolsConfigPath)), {}),
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
        m_loading = false;
        if (reply->error() != QNetworkReply::NoError) {
            qWarning() << "AMM registry: fetch failed:" << reply->errorString();
            m_fetchError = QStringLiteral("fetch_failed");
            emit statusChanged();
            return;  // keep serving whatever we have (cache / previous)
        }

        m_fetchError.clear();
        const QByteArray body = reply->readAll();
        if (applyRegistry(body, QStringLiteral("remote"), reply->url().toString())) {
            // Key the cache by the effective URL (env or UI-configured), matching
            // what loadDiskCache() looks up.
            saveDiskCache(reply->url().toString(), body);
        }
        emit statusChanged();
    });
}

bool RegistryLoader::applyRegistry(const QByteArray& body, const QString& source, const QString& url)
{
    const QJsonDocument doc = QJsonDocument::fromJson(body);
    if (!doc.isObject()) {
        qWarning() << "AMM registry: document is not a JSON object";
        m_fetchError = QStringLiteral("invalid_registry");
        return false;
    }
    m_registryObj = doc.object();
    m_lastSource = source;
    m_snapshotUrl = url;
    return applySelection();
}

bool RegistryLoader::applySelection()
{
    const QJsonArray networks = m_registryObj.value(QStringLiteral("networks")).toArray();
    const QString activeId = selectActiveNetwork(networks);
    if (activeId.isEmpty()) {
        qWarning() << "AMM registry: no networks declared; nothing applied";
        m_fetchError = QStringLiteral("no_networks");
        m_activeAmmProgramId.clear();
        m_activeAmmConfigId.clear();
        publish({}, {}, m_lastSource, {});
        return false;
    }

    // Adopt the active network's declared AMM program id and config-PDA id so the
    // backend can point ops at that instance (setAmmProgramId / setConfigId) without
    // an AMM_PROGRAM_ID / AMM_CONFIG_ID.
    m_activeAmmProgramId.clear();
    m_activeAmmConfigId.clear();
    for (const QJsonValue& entry : networks) {
        const QJsonObject net = entry.toObject();
        if (net.value(QStringLiteral("id")).toString() == activeId) {
            m_activeAmmProgramId = net.value(QStringLiteral("programIds"))
                                       .toObject()
                                       .value(QStringLiteral("amm"))
                                       .toString();
            m_activeAmmConfigId = net.value(QStringLiteral("ammConfigId")).toString();
            break;
        }
    }

    publish(parseTokens(m_registryObj.value(QStringLiteral("tokens")).toArray(), activeId),
            parsePools(m_registryObj.value(QStringLiteral("pools")).toArray(), activeId),
            m_lastSource, activeId);
    return true;
}

void RegistryLoader::selectNetwork(const QString& id)
{
    if (id == m_selectedNetwork)
        return;
    m_selectedNetwork = id;
    // Re-filter the already-loaded registry to the new pick (no re-fetch). If none
    // is loaded yet, the pick is remembered and applied when one loads.
    if (!m_registryObj.isEmpty())
        applySelection();
}

QVariantList RegistryLoader::networks() const
{
    QVariantList out;
    const QJsonArray nets = m_registryObj.value(QStringLiteral("networks")).toArray();
    for (const QJsonValue& entry : nets) {
        const QJsonObject net = entry.toObject();
        const QString id = net.value(QStringLiteral("id")).toString();
        if (id.isEmpty())
            continue;
        out.append(QVariantMap{
            {QStringLiteral("id"), id},
            {QStringLiteral("name"), net.value(QStringLiteral("name")).toString()},
        });
    }
    return out;
}

QString RegistryLoader::selectActiveNetwork(const QJsonArray& networks) const
{
    if (networks.isEmpty())
        return {};

    const auto declares = [&networks](const QString& id) {
        for (const QJsonValue& entry : networks) {
            if (entry.toObject().value(QStringLiteral("id")).toString() == id)
                return true;
        }
        return false;
    };

    // The user's explicit pick, if it's still a declared network.
    if (!m_selectedNetwork.isEmpty() && declares(m_selectedNetwork))
        return m_selectedNetwork;

    // AMM_NETWORK sets the initial default (e2e / dev), if it names one.
    if (!m_envNetwork.isEmpty() && declares(m_envNetwork))
        return m_envNetwork;

    // Otherwise default to the first declared network.
    return networks.at(0).toObject().value(QStringLiteral("id")).toString();
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
    emit statusChanged();
}

void RegistryLoader::loadDiskCache(const QString& url)
{
    QFile file(cachePath());
    if (!file.open(QIODevice::ReadOnly))
        return;
    const QJsonObject obj = QJsonDocument::fromJson(file.readAll()).object();
    // Only apply a cache whose stored URL matches this source URL. This is a
    // source-selection check; it does not authenticate the cached document.
    if (obj.value(QStringLiteral("url")).toString() != url)
        return;

    // Re-apply the cached registry against the current selection (the active
    // network may resolve differently than when it was written).
    const QByteArray body =
        QJsonDocument(obj.value(QStringLiteral("registry")).toObject()).toJson(QJsonDocument::Compact);
    applyRegistry(body, QStringLiteral("cache"), url);
}

void RegistryLoader::saveDiskCache(const QString& url, const QByteArray& body) const
{
    const QString path = cachePath();
    QDir().mkpath(QFileInfo(path).absolutePath());

    QJsonObject obj;
    obj.insert(QStringLiteral("url"), url);
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
