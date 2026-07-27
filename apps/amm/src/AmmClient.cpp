#include "AmmClient.h"

#include <QDebug>
#include <QJsonDocument>
#include <QJsonParseError>

#include "amm_client.h"

namespace {
    using Operation = char* (*)(const char*);

    AmmClientResult call(Operation operation, const QJsonObject& request)
    {
        const QByteArray payload = QJsonDocument(request).toJson(QJsonDocument::Compact);
        char* raw = operation(payload.constData());
        if (!raw) {
            qWarning() << "AmmClient: bundled client returned a null response";
            return {};
        }
        const QByteArray response(raw);
        amm_free(raw);

        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(response, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
            qWarning() << "AmmClient: bundled client returned invalid JSON";
            return {};
        }
        const QJsonObject envelope = document.object();
        if (!envelope.value(QStringLiteral("ok")).toBool()) {
            qWarning() << "AmmClient: bundled client failure:"
                       << envelope.value(QStringLiteral("error")).toString();
            return {};
        }
        if (!envelope.value(QStringLiteral("value")).isObject()) {
            qWarning() << "AmmClient: bundled client value is not an object";
            return {};
        }
        return { true, envelope.value(QStringLiteral("value")).toObject() };
    }
}

AmmClientResult BundledAmmClient::configId(const QJsonObject& request) const
{
    return call(amm_config_id, request);
}

AmmClientResult BundledAmmClient::tokenIds(const QJsonObject& request) const
{
    return call(amm_token_ids, request);
}

AmmClientResult BundledAmmClient::pairIds(const QJsonObject& request) const
{
    return call(amm_pair_ids, request);
}

AmmClientResult BundledAmmClient::context(const QJsonObject& request) const
{
    return call(amm_context, request);
}

AmmClientResult BundledAmmClient::quote(const QJsonObject& request) const
{
    return call(amm_quote, request);
}

AmmClientResult BundledAmmClient::plan(const QJsonObject& request) const
{
    return call(amm_plan, request);
}

AmmClientResult BundledAmmClient::swapPair(const QJsonObject& request) const
{
    return call(amm_swap_pair, request);
}

AmmClientResult BundledAmmClient::resolvePool(const QJsonObject& request) const
{
    return call(amm_resolve_pool, request);
}

AmmClientResult BundledAmmClient::swapPlan(const QJsonObject& request) const
{
    return call(amm_swap_plan, request);
}

AmmClientResult BundledAmmClient::programId(const QJsonObject& request) const
{
    return call(amm_program_id, request);
}
