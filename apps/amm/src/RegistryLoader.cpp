#include "RegistryLoader.h"

#include <QByteArray>
#include <QFile>
#include <QIODevice>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QString>
#include <QVariantMap>

namespace {
    // Absolute path to the JSON known-pools config consumed by the pools list.
    // Mirrors TOKENS_CONFIG for the token list; produced by the AMM testnet
    // setup script (apps/amm/tests/testnet/setup-amm-testnet.sh).
    constexpr char POOLS_CONFIG_ENV[] = "AMM_POOLS_CONFIG";

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

    // Absolute path to the JSON token-list config consumed by the token list.
    constexpr char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";

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
            if (definitionId.isEmpty() || holding.isEmpty() || !decimals.isDouble())
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

    // v1 registry source: a local JSON file at an env-var path. Returns empty on
    // unset/unreadable — callers fail soft. A later phase adds a remote source
    // (AMM_REGISTRY_URL) whose fetched payload feeds the same parsers above.
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

void RegistryLoader::refresh()
{
    // v1 source: the local JSON files. The parsers are source-agnostic, so a
    // later remote source (AMM_REGISTRY_URL) can feed the same validation and
    // shaping without touching the consumers.
    m_tokens = parseTokensJson(readConfigFileBytes(TOKENS_CONFIG_ENV));
    m_pools = parsePoolsJson(readConfigFileBytes(POOLS_CONFIG_ENV));
    ++m_revision;
    emit changed();
}
