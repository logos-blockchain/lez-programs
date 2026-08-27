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
    // Local-file source (dev / local-sequencer). Takes precedence over the
    // remote registry when either is set.
    constexpr char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";
    constexpr char POOLS_CONFIG_ENV[] = "AMM_POOLS_CONFIG";
    // Remote source: the URL of a registry manifest (registry.json).
    constexpr char REGISTRY_URL_ENV[] = "AMM_REGISTRY_URL";

    // Parses the pools JSON payload into the QVariantList the Pools UI renders.
    // Source-agnostic (local file or remote payload). Fails soft (empty list)
    // when the payload is not a JSON array — one malformed entry is skipped
    // rather than dropping the whole list. tokenA/tokenB (display symbols) and a
    // numeric feeBps are required; the id fields pass through when present so the
    // entry can later be resolved on-chain.
    QVariantList parsePoolsJson(const QByteArray& bytes)
    {
        QVariantList out;

        const QJsonDocument doc = QJsonDocument::fromJson(bytes);
        if (!doc.isArray())
            return out;

        for (const QJsonValue& entry : doc.array()) {
            if (!entry.isObject())
                continue;
            const QJsonObject obj = entry.toObject();

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

    // Parses the tokens JSON payload into the QVariantList the Swap token picker
    // renders. Same fail-soft, skip-malformed-entry behavior as parsePoolsJson().
    // symbol/name are display; definitionId/holding are the token's account ids
    // and pass through as configured (base58 or hex) — the module methods
    // normalize to hex at their boundary. decimals must be a non-negative integer
    // (a wrong value would misrender amounts).
    QVariantList parseTokensJson(const QByteArray& bytes)
    {
        QVariantList out;

        const QJsonDocument doc = QJsonDocument::fromJson(bytes);
        if (!doc.isArray())
            return out;

        for (const QJsonValue& entry : doc.array()) {
            if (!entry.isObject())
                continue;
            const QJsonObject obj = entry.toObject();

            const QString definitionId = obj.value(QStringLiteral("definitionId")).toString();
            const QString holding = obj.value(QStringLiteral("holding")).toString();
            const QJsonValue decimals = obj.value(QStringLiteral("decimals"));
            // holding is per-wallet and absent from a shared remote list; only
            // definitionId + a valid decimals are required for a token to render.
            if (definitionId.isEmpty() || !decimals.isDouble())
                continue;

            QVariantMap token;
            token.insert(QStringLiteral("symbol"), obj.value(QStringLiteral("symbol")).toString());
            token.insert(QStringLiteral("name"), obj.value(QStringLiteral("name")).toString());
            token.insert(QStringLiteral("definitionId"), definitionId);
            token.insert(QStringLiteral("holding"), holding);
            token.insert(QStringLiteral("decimals"), decimals.toInt());
            out.append(token);
        }
        return out;
    }

    // Reads a local JSON file at an env-var path. Returns empty on
    // unset/unreadable — callers fail soft.
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

void RegistryLoader::setExpectedProgramIds(const QString& ammProgramId,
                                           const QString& tokenProgramId)
{
    m_expectedAmmProgramId = ammProgramId;
    m_expectedTokenProgramId = tokenProgramId;
}

void RegistryLoader::refresh()
{
    // Supersede any in-flight remote fetch (a reply from an older generation is
    // dropped in its finished handler).
    ++m_generation;

    // local-replaces-remote: a configured local file wins outright.
    if (hasLocalSource()) {
        loadLocal();
        return;
    }

    const QString url = qEnvironmentVariable(REGISTRY_URL_ENV);
    if (url.isEmpty()) {
        publish({}, {}, QStringLiteral("none"));
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
    publish(parseTokensJson(readConfigFileBytes(TOKENS_CONFIG_ENV)),
            parsePoolsJson(readConfigFileBytes(POOLS_CONFIG_ENV)),
            QStringLiteral("local"));
}

void RegistryLoader::startRemote(const QUrl& manifestUrl)
{
    const quint64 generation = m_generation;
    QNetworkReply* reply = nam()->get(QNetworkRequest(manifestUrl));
    connect(reply, &QNetworkReply::finished, this, [this, reply, manifestUrl, generation]() {
        reply->deleteLater();
        if (generation != m_generation)
            return;  // superseded by a newer refresh
        if (reply->error() != QNetworkReply::NoError) {
            qWarning() << "AMM registry: manifest fetch failed:" << reply->errorString();
            return;  // keep serving whatever we have (cache / previous)
        }

        const QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
        if (!doc.isObject()) {
            qWarning() << "AMM registry: manifest is not a JSON object";
            return;
        }
        const QJsonObject manifest = doc.object();
        if (!deploymentMatches(manifest)) {
            qWarning() << "AMM registry: manifest targets a different deployment; ignoring";
            return;
        }

        const QString stamp = manifest.value(QStringLiteral("timestamp")).toVariant().toString();
        // Revalidation: an unchanged manifest with a non-empty snapshot means the
        // cached lists are already current — skip re-downloading them.
        if (!stamp.isEmpty() && stamp == m_stamp && !m_tokens.isEmpty())
            return;

        const QString tokensRel = manifest.value(QStringLiteral("tokensUrl")).toString();
        const QString poolsRel = manifest.value(QStringLiteral("poolsUrl")).toString();
        if (tokensRel.isEmpty() || poolsRel.isEmpty()) {
            qWarning() << "AMM registry: manifest missing tokensUrl/poolsUrl";
            return;
        }
        fetchLists(manifestUrl.resolved(QUrl(tokensRel)),
                   manifestUrl.resolved(QUrl(poolsRel)), stamp, generation);
    });
}

void RegistryLoader::fetchLists(const QUrl& tokensUrl, const QUrl& poolsUrl,
                                const QString& stamp, quint64 generation)
{
    // Fetch the two lists in sequence, then publish both together so the UI
    // never sees tokens without their pools (or vice versa).
    QNetworkReply* tokensReply = nam()->get(QNetworkRequest(tokensUrl));
    connect(tokensReply, &QNetworkReply::finished, this,
            [this, tokensReply, poolsUrl, stamp, generation]() {
        tokensReply->deleteLater();
        if (generation != m_generation)
            return;
        if (tokensReply->error() != QNetworkReply::NoError) {
            qWarning() << "AMM registry: tokens fetch failed:" << tokensReply->errorString();
            return;
        }
        const QVariantList tokens = parseTokensJson(tokensReply->readAll());

        QNetworkReply* poolsReply = nam()->get(QNetworkRequest(poolsUrl));
        connect(poolsReply, &QNetworkReply::finished, this,
                [this, poolsReply, tokens, stamp, generation]() {
            poolsReply->deleteLater();
            if (generation != m_generation)
                return;
            if (poolsReply->error() != QNetworkReply::NoError) {
                qWarning() << "AMM registry: pools fetch failed:" << poolsReply->errorString();
                return;
            }
            const QVariantList pools = parsePoolsJson(poolsReply->readAll());
            m_stamp = stamp;
            publish(tokens, pools, QStringLiteral("remote"));
            saveDiskCache(qEnvironmentVariable(REGISTRY_URL_ENV), stamp);
        });
    });
}

bool RegistryLoader::deploymentMatches(const QJsonObject& manifest) const
{
    // No expected ids ⇒ nothing to check against (permissive).
    if (m_expectedAmmProgramId.isEmpty() && m_expectedTokenProgramId.isEmpty())
        return true;

    const QJsonObject ids = manifest.value(QStringLiteral("programIds")).toObject();
    const QString amm = ids.value(QStringLiteral("amm")).toString();
    const QString token = ids.value(QStringLiteral("token")).toString();
    // A manifest that doesn't declare a deployment is trusted (the operator
    // chose the URL); the guard only rejects a declared, mismatched deployment.
    if (amm.isEmpty() && token.isEmpty())
        return true;
    return amm == m_expectedAmmProgramId && token == m_expectedTokenProgramId;
}

void RegistryLoader::publish(const QVariantList& tokens, const QVariantList& pools,
                             const QString& source)
{
    m_tokens = tokens;
    m_pools = pools;
    m_source = source;
    ++m_revision;
    emit changed();
}

void RegistryLoader::loadDiskCache(const QString& url)
{
    QFile file(cachePath());
    if (!file.open(QIODevice::ReadOnly))
        return;
    const QJsonDocument doc = QJsonDocument::fromJson(file.readAll());
    if (!doc.isObject())
        return;
    const QJsonObject obj = doc.object();
    // Only trust a cache written for this same source URL.
    if (obj.value(QStringLiteral("url")).toString() != url)
        return;

    m_stamp = obj.value(QStringLiteral("stamp")).toVariant().toString();
    publish(obj.value(QStringLiteral("tokens")).toArray().toVariantList(),
            obj.value(QStringLiteral("pools")).toArray().toVariantList(),
            QStringLiteral("cache"));
}

void RegistryLoader::saveDiskCache(const QString& url, const QString& stamp) const
{
    const QString path = cachePath();
    QDir().mkpath(QFileInfo(path).absolutePath());

    QJsonObject obj;
    obj.insert(QStringLiteral("url"), url);
    obj.insert(QStringLiteral("stamp"), stamp);
    obj.insert(QStringLiteral("tokens"), QJsonArray::fromVariantList(m_tokens));
    obj.insert(QStringLiteral("pools"), QJsonArray::fromVariantList(m_pools));

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
