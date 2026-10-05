#include "SwapConfirmation.h"

#include <QMetaType>

namespace {

QVariantMap failure(const QString& error)
{
    return {{QStringLiteral("status"), QStringLiteral("error")},
            {QStringLiteral("error"), error}};
}

bool revisionMatches(const QVariant& revision, int currentRevision)
{
    // QML numbers arrive as either an integer or a double. Do not let QVariant's
    // coercion accept a string, boolean, or rounded fractional revision.
    switch (revision.metaType().id()) {
    case QMetaType::Int:
    case QMetaType::UInt:
    case QMetaType::LongLong:
    case QMetaType::ULongLong:
    case QMetaType::Double:
        return revision.toDouble() == static_cast<double>(currentRevision);
    default:
        return false;
    }
}

} // namespace

QVariantMap SwapConfirmation::submit(const QVariantMap& request, int currentRevision,
                                     bool walletOpen, const Submit& dispatch)
{
    if (!revisionMatches(request.value(QStringLiteral("contextRevision")), currentRevision))
        return failure(QStringLiteral("swap_context_changed"));
    if (!walletOpen)
        return failure(QStringLiteral("wallet_unavailable"));

    const QString mode = request.value(QStringLiteral("swapMode")).toString();
    if (mode != QStringLiteral("swap-exact-input")
        && mode != QStringLiteral("swap-exact-output")) {
        return failure(QStringLiteral("invalid_swap_mode"));
    }

    // Validate the confirmation envelope, leaving account and amount domain
    // validation to the module that already owns it.
    for (const auto* key : {"swapMode", "sellDefinitionId", "buyDefinitionId",
                            "sellHoldingId", "buyHoldingId", "amount", "bound", "deadline"}) {
        if (request.value(QLatin1String(key)).metaType().id() != QMetaType::QString)
            return failure(QStringLiteral("invalid_swap_request"));
    }

    const Request confirmed{
        mode,
        request.value(QStringLiteral("sellDefinitionId")).toString(),
        request.value(QStringLiteral("buyDefinitionId")).toString(),
        request.value(QStringLiteral("sellHoldingId")).toString(),
        request.value(QStringLiteral("buyHoldingId")).toString(),
        request.value(QStringLiteral("amount")).toString(),
        request.value(QStringLiteral("bound")).toString(),
        request.value(QStringLiteral("deadline")).toString(),
    };
    const QString txHash = dispatch(confirmed);
    if (txHash.isEmpty())
        return failure(QStringLiteral("wallet_submission_failed"));
    return {{QStringLiteral("status"), QStringLiteral("ok")},
            {QStringLiteral("txHash"), txHash}};
}
