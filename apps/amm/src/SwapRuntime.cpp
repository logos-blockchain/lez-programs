#include "SwapRuntime.h"

#include <QJsonArray>
#include <QStringList>
#include <QVector>

#include "AmmClient.h"
#include "WalletProvider.h"

namespace {
    QJsonObject accountReadJson(const WalletAccountRead& read)
    {
        QJsonObject result {
            { QStringLiteral("id"), read.accountId },
            { QStringLiteral("status"), read.status },
        };
        if (read.ok()) {
            result.insert(QStringLiteral("account"), QJsonObject {
                { QStringLiteral("program_owner"), read.programOwner },
                { QStringLiteral("balance"), read.balanceHex },
                { QStringLiteral("nonce"), read.nonceHex },
                { QStringLiteral("data"), read.dataHex },
            });
        }
        return result;
    }

    QStringList jsonStringList(const QJsonArray& values)
    {
        QStringList result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(value.toString());
        return result;
    }

    QVector<bool> jsonBoolList(const QJsonArray& values)
    {
        QVector<bool> result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(value.toBool());
        return result;
    }

    QVector<quint32> jsonUIntList(const QJsonArray& values)
    {
        QVector<quint32> result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(static_cast<quint32>(value.toInteger()));
        return result;
    }
}

SwapRuntime::SwapRuntime(WalletProvider* wallet, AmmClient* client)
    : m_wallet(wallet),
      m_client(client)
{
}

QJsonObject SwapRuntime::readConfig(const ActiveNetworkSnapshot& network) const
{
    const AmmClientResult configResult = m_client->configId(
        QJsonObject { { QStringLiteral("ammProgramId"), network.ammProgramId } });
    if (!configResult.ok)
        return {};
    return accountReadJson(m_wallet->readPublicAccount(
        configResult.value.value(QStringLiteral("configId")).toString()));
}

QVariantMap SwapRuntime::resolvePool(const QString& tokenInId,
                                     const QString& tokenOutId,
                                     const ActiveNetworkSnapshot& network)
{
    const QVariantMap absent { { QStringLiteral("exists"), false } };
    // Attaches a diagnostic code the Swap UI surfaces verbatim (SwapCard.qml:
    // pool.error). The normal "no pool / no liquidity yet" state stays
    // code-less — that's the bare `absent` / resolve_pool `{exists:false}`
    // below, which the UI renders as its neutral "no pool" message, not an error.
    const auto failure = [](const QString& code) {
        return QVariantMap {
            { QStringLiteral("exists"), false },
            { QStringLiteral("error"), code },
        };
    };

    // Network still resolving AMM_PROGRAM_BIN: transient startup state, not a
    // diagnostic — surface nothing so the UI keeps its "loading" affordance.
    if (network.status != QStringLiteral("ready"))
        return absent;

    // readConfig returns {} only when the config_id op itself fails (a client
    // bug, not a chain state) — as opposed to the config account merely being
    // unreadable, which surfaces as swap_pair's `config_unavailable` below.
    const QJsonObject config = readConfig(network);
    if (config.isEmpty())
        return failure(QStringLiteral("backend_error"));

    const AmmClientResult pairResult = m_client->swapPair(QJsonObject {
        { QStringLiteral("ammProgramId"), network.ammProgramId },
        { QStringLiteral("tokenInId"), tokenInId },
        { QStringLiteral("tokenOutId"), tokenOutId },
        { QStringLiteral("config"), config },
    });
    if (!pairResult.ok)
        return failure(QStringLiteral("backend_error"));
    if (pairResult.value.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        // swap_pair reports `config_unavailable` when the AMM config account is
        // missing/uninitialized on this network, `same_token_pair` for an
        // invalid pair, etc. Propagate its code rather than flattening to
        // "no pool", so a misconfigured network is distinguishable from an
        // empty one.
        const QString code = pairResult.value.value(QStringLiteral("code")).toString();
        return failure(code.isEmpty() ? QStringLiteral("backend_error") : code);
    }

    const QJsonObject pool = accountReadJson(m_wallet->readPublicAccount(
        pairResult.value.value(QStringLiteral("poolId")).toString()));
    const AmmClientResult resolveResult =
        m_client->resolvePool(QJsonObject { { QStringLiteral("pool"), pool } });
    if (!resolveResult.ok)
        return failure(QStringLiteral("backend_error"));
    return resolveResult.value.toVariantMap();
}

QString SwapRuntime::swap(const QString& tokenInId,
                          const QString& tokenOutId,
                          const QString& userInputHoldingId,
                          const QString& userOutputHoldingId,
                          const QString& amountInDecimal,
                          const QString& minOutDecimal,
                          const QString& deadlineMs,
                          const ActiveNetworkSnapshot& network,
                          bool walletOpen)
{
    if (network.status != QStringLiteral("ready") || !walletOpen)
        return {};

    const QJsonObject config = readConfig(network);
    if (config.isEmpty())
        return {};

    const AmmClientResult planResult = m_client->swapPlan(QJsonObject {
        { QStringLiteral("ammProgramId"), network.ammProgramId },
        { QStringLiteral("tokenInId"), tokenInId },
        { QStringLiteral("tokenOutId"), tokenOutId },
        { QStringLiteral("config"), config },
        { QStringLiteral("userInputHoldingId"), userInputHoldingId },
        { QStringLiteral("userOutputHoldingId"), userOutputHoldingId },
        { QStringLiteral("amountIn"), amountInDecimal },
        { QStringLiteral("minOut"), minOutDecimal },
        { QStringLiteral("deadlineMs"), deadlineMs },
    });
    if (!planResult.ok
        || planResult.value.value(QStringLiteral("status")).toString() != QStringLiteral("ready"))
        return {};

    const QJsonObject plan = planResult.value;
    const WalletSubmission submission = m_wallet->submitPublicTransaction({
        plan.value(QStringLiteral("programId")).toString(),
        jsonStringList(plan.value(QStringLiteral("accountIds")).toArray()),
        jsonBoolList(plan.value(QStringLiteral("signingRequirements")).toArray()),
        jsonUIntList(plan.value(QStringLiteral("instruction")).toArray()),
    });
    if (!submission.accepted())
        return {};
    return submission.nativeHash;
}
