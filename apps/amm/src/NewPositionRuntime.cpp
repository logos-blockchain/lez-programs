#include "NewPositionRuntime.h"

#include <array>
#include <cstddef>
#include <cstdint>

#include <QByteArray>
#include <QDateTime>
#include <QJsonObject>
#include <QScopedValueRollback>
#include <libbase58.h>

#include "AmmClient.h"
#include "WalletProvider.h"

namespace {
    const char SCHEMA[] = "new-position.v1";
    constexpr qsizetype HASH_BYTES = 32;
    constexpr std::size_t BASE58_BUFFER_SIZE = 45;

    QString base58TransactionId(const QString& transactionHash)
    {
        const QByteArray hex = transactionHash.toLatin1();
        const QByteArray bytes = QByteArray::fromHex(hex);
        if (bytes.size() != HASH_BYTES || bytes.toHex() != hex)
            return {};

        std::array<char, BASE58_BUFFER_SIZE> encoded {};
        std::size_t size = encoded.size();
        if (!b58enc(encoded.data(), &size, bytes.constData(),
                    static_cast<std::size_t>(bytes.size()))) {
            return {};
        }
        return QString::fromLatin1(
            encoded.data(), static_cast<qsizetype>(size - 1));
    }

    QJsonObject issue(const QString& code,
                      const QJsonArray& blockingFields = {})
    {
        return {
            { QStringLiteral("code"), code },
            { QStringLiteral("recoverable"), true },
            { QStringLiteral("blockingFields"), blockingFields },
            { QStringLiteral("details"), QJsonObject() },
        };
    }

    QJsonObject publicError(const QString& code,
                            const QJsonArray& blockingFields = {},
                            const QJsonObject& details = {})
    {
        QJsonObject error = issue(code, blockingFields);
        error.insert(QStringLiteral("details"), details);
        return {
            { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("canSubmit"), false },
            { QStringLiteral("code"), code },
            { QStringLiteral("errors"), QJsonArray { error } },
            { QStringLiteral("warnings"), QJsonArray() },
            { QStringLiteral("accountPreview"), QJsonArray() },
        };
    }

    QJsonObject contextState(const QString& status,
                             const ActiveNetworkSnapshot& network,
                             const QString& code = {})
    {
        QJsonObject state {
            { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
            { QStringLiteral("status"), status },
            { QStringLiteral("networkId"), network.id },
            { QStringLiteral("networkFingerprint"), network.fingerprint },
            { QStringLiteral("tokens"), QJsonArray() },
            { QStringLiteral("feeTiers"), QJsonArray() },
            { QStringLiteral("warnings"), QJsonArray() },
        };
        if (!code.isEmpty())
            state.insert(QStringLiteral("code"), code);
        return state;
    }

    QJsonArray variantStringArray(const QVariant& value)
    {
        QJsonArray result;
        for (const QVariant& item : value.toList())
            result.append(item.toString());
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

    QJsonArray accountReadsJson(const QVector<WalletAccountRead>& reads)
    {
        QJsonArray result;
        for (const WalletAccountRead& read : reads)
            result.append(accountReadJson(read));
        return result;
    }
}

NewPositionRuntime::NewPositionRuntime(WalletProvider* wallet, AmmClient* client)
    : m_wallet(wallet),
      m_client(client)
{
}

void NewPositionRuntime::clearWalletAccounts()
{
    m_wallet->clearSnapshot();
}

QJsonArray NewPositionRuntime::walletAccountReads(bool walletOpen, bool refresh) const
{
    if (!walletOpen)
        return {};
    return accountReadsJson(m_wallet->snapshot(refresh).publicAccountReads);
}

QJsonObject NewPositionRuntime::buildQuoteInput(const QVariantMap& request,
                                                const ActiveNetworkSnapshot& network,
                                                bool walletOpen,
                                                bool freshWalletAccounts,
                                                QJsonObject* error) const
{
    if (network.status != QStringLiteral("ready")) {
        *error = publicError(network.status);
        return {};
    }
    const AmmClientResult configResult = m_client->configId(
        QJsonObject { { QStringLiteral("ammProgramId"), network.ammProgramId } });
    if (!configResult.ok) {
        *error = publicError(QStringLiteral("backend_error"));
        return {};
    }
    const QJsonObject configManifest = configResult.value;
    const QJsonObject config = accountReadJson(m_wallet->readPublicAccount(
        configManifest.value(QStringLiteral("configId")).toString()));
    const QJsonObject requestObject = QJsonObject::fromVariantMap(request);
    const AmmClientResult pairResult = m_client->pairIds(
        QJsonObject {
            { QStringLiteral("ammProgramId"), network.ammProgramId },
            { QStringLiteral("config"), config },
            { QStringLiteral("tokenAId"), requestObject.value(QStringLiteral("tokenAId")) },
            { QStringLiteral("tokenBId"), requestObject.value(QStringLiteral("tokenBId")) },
        });
    if (!pairResult.ok) {
        *error = publicError(QStringLiteral("backend_error"));
        return {};
    }
    const QJsonObject pairManifest = pairResult.value;
    if (pairManifest.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        *error = publicError(pairManifest.value(QStringLiteral("code")).toString());
        return {};
    }

    const QJsonArray walletAccounts = walletAccountReads(walletOpen, freshWalletAccounts);
    const QJsonObject snapshot {
        { QStringLiteral("config"), config },
        { QStringLiteral("tokenA"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("tokenAId")).toString())) },
        { QStringLiteral("tokenB"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("tokenBId")).toString())) },
        { QStringLiteral("pool"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("poolId")).toString())) },
        { QStringLiteral("vaultA"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("vaultAId")).toString())) },
        { QStringLiteral("vaultB"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("vaultBId")).toString())) },
        { QStringLiteral("lpDefinition"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("lpDefinitionId")).toString())) },
        { QStringLiteral("lpLockHolding"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("lpLockHoldingId")).toString())) },
        { QStringLiteral("currentTick"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("currentTickId")).toString())) },
        { QStringLiteral("clock"), accountReadJson(m_wallet->readPublicAccount(pairManifest.value(QStringLiteral("clockId")).toString())) },
        { QStringLiteral("walletAvailable"), walletOpen },
        { QStringLiteral("walletAccounts"), walletAccounts },
    };
    return {
        { QStringLiteral("networkId"), network.id },
        { QStringLiteral("networkFingerprint"), network.fingerprint },
        { QStringLiteral("ammProgramId"), network.ammProgramId },
        { QStringLiteral("request"), requestObject },
        { QStringLiteral("snapshot"), snapshot },
    };
}

QVariantMap NewPositionRuntime::context(const QVariantMap& request,
                                        const ActiveNetworkSnapshot& network,
                                        bool walletOpen,
                                        bool refreshWalletAccounts)
{
    if (network.status != QStringLiteral("ready"))
        return contextState(network.status, network).toVariantMap();

    const QJsonArray walletAccounts = walletAccountReads(walletOpen, refreshWalletAccounts);

    const QJsonObject hints = QJsonObject::fromVariantMap(request);
    const AmmClientResult configResult = m_client->configId(
        QJsonObject { { QStringLiteral("ammProgramId"), network.ammProgramId } });
    if (!configResult.ok)
        return contextState(
            QStringLiteral("error"), network, QStringLiteral("backend_error")).toVariantMap();

    const QJsonObject configManifest = configResult.value;
    const QJsonObject config = accountReadJson(m_wallet->readPublicAccount(
        configManifest.value(QStringLiteral("configId")).toString()));
    QJsonArray configured;
    for (const QString& id : network.tokenIds)
        configured.append(id);
    const QJsonArray recent = variantStringArray(hints.value(QStringLiteral("recentTokenIds")).toVariant());
    const QJsonArray resolved = variantStringArray(hints.value(QStringLiteral("resolvedTokenIds")).toVariant());

    const AmmClientResult tokenResult = m_client->tokenIds(
        QJsonObject {
            { QStringLiteral("ammProgramId"), network.ammProgramId },
            { QStringLiteral("config"), config },
            { QStringLiteral("walletAccounts"), walletAccounts },
            { QStringLiteral("configuredTokenIds"), configured },
            { QStringLiteral("recentTokenIds"), recent },
            { QStringLiteral("resolvedTokenIds"), resolved },
        });
    const QJsonObject tokenManifest = tokenResult.value;
    if (!tokenResult.ok
        || tokenManifest.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        const QString code = tokenResult.ok
            ? tokenManifest.value(QStringLiteral("code")).toString()
            : QStringLiteral("backend_error");
        return contextState(
            QStringLiteral("error"),
            network,
            code.isEmpty() ? QStringLiteral("backend_error") : code).toVariantMap();
    }

    QJsonArray definitions;
    for (const QJsonValue& id : tokenManifest.value(QStringLiteral("tokenIds")).toArray())
        definitions.append(accountReadJson(m_wallet->readPublicAccount(id.toString())));

    const AmmClientResult contextResult = m_client->context(
        QJsonObject {
            { QStringLiteral("networkId"), network.id },
            { QStringLiteral("networkFingerprint"), network.fingerprint },
            { QStringLiteral("ammProgramId"), network.ammProgramId },
            { QStringLiteral("walletAvailable"), walletOpen },
            { QStringLiteral("config"), config },
            { QStringLiteral("walletAccounts"), walletAccounts },
            { QStringLiteral("tokenDefinitions"), definitions },
            { QStringLiteral("configuredTokenIds"), configured },
            { QStringLiteral("recentTokenIds"), recent },
            { QStringLiteral("resolvedTokenIds"), resolved },
        });
    return (contextResult.ok
        ? contextResult.value
        : contextState(
            QStringLiteral("error"), network, QStringLiteral("backend_error"))).toVariantMap();
}

QVariantMap NewPositionRuntime::quote(const QVariantMap& request,
                                      const ActiveNetworkSnapshot& network,
                                      bool walletOpen)
{
    QJsonObject error;
    const QJsonObject input = buildQuoteInput(request, network, walletOpen, false, &error);
    if (!error.isEmpty())
        return error.toVariantMap();

    const AmmClientResult result = m_client->quote(input);
    return (result.ok ? result.value : publicError(QStringLiteral("backend_error"))).toVariantMap();
}

QVariantMap NewPositionRuntime::submit(const QVariantMap& request,
                                       const QString& quoteHash,
                                       const ActiveNetworkSnapshot& network,
                                       bool walletOpen)
{
    if (m_submitInFlight)
        return publicError(QStringLiteral("submit_in_progress")).toVariantMap();
    if (!walletOpen)
        return publicError(QStringLiteral("wallet_unavailable")).toVariantMap();
    QScopedValueRollback<bool> submitGuard(m_submitInFlight, true);

    QJsonObject error;
    const QJsonObject input = buildQuoteInput(request, network, walletOpen, true, &error);
    if (!error.isEmpty())
        return error.toVariantMap();

    const AmmClientResult quoteResult = m_client->quote(input);
    if (!quoteResult.ok)
        return publicError(QStringLiteral("backend_error")).toVariantMap();
    const QJsonObject quote = quoteResult.value;
    if (quote.value(QStringLiteral("quoteHash")).toString() != quoteHash) {
        QJsonObject result = publicError(QStringLiteral("quote_changed"));
        result.insert(QStringLiteral("quote"), quote);
        return result.toVariantMap();
    }
    if (!quote.value(QStringLiteral("canSubmit")).toBool(false)) {
        QJsonObject result = publicError(QStringLiteral("quote_not_submittable"));
        result.insert(QStringLiteral("quote"), quote);
        return result.toVariantMap();
    }

    QJsonValue freshLp;
    if (quote.value(QStringLiteral("requiresFreshLp")).toBool(false)) {
        const WalletAccountCreation creation = m_wallet->createAccount(true);
        if (!creation.ok() || !creation.publicAccount.ok())
            return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
        freshLp = accountReadJson(creation.publicAccount);
    }

    QJsonObject planInput = input;
    planInput.insert(QStringLiteral("quoteHash"), quoteHash);
    planInput.insert(QStringLiteral("nowMs"), QDateTime::currentMSecsSinceEpoch());
    if (!freshLp.isUndefined())
        planInput.insert(QStringLiteral("freshLp"), freshLp);

    const AmmClientResult planResult = m_client->plan(planInput);
    if (!planResult.ok)
        return publicError(QStringLiteral("backend_error")).toVariantMap();
    const QJsonObject plan = planResult.value;
    if (plan.value(QStringLiteral("status")).toString() != QStringLiteral("ready")) {
        const QString code = plan.value(QStringLiteral("code")).toString();
        return publicError(code.isEmpty() ? QStringLiteral("wallet_submission_failed") : code)
            .toVariantMap();
    }

    const QStringList accountIds = jsonStringList(plan.value(QStringLiteral("accountIds")).toArray());
    const QVector<bool> signingRequirements = jsonBoolList(plan.value(QStringLiteral("signingRequirements")).toArray());
    const QVector<quint32> instruction = jsonUIntList(plan.value(QStringLiteral("instruction")).toArray());
    const QString programId = plan.value(QStringLiteral("programId")).toString();
    bool deadlineValid = false;
    const qulonglong deadline = plan.value(QStringLiteral("deadlineMs")).toString().toULongLong(&deadlineValid);
    if (!deadlineValid
        || static_cast<qulonglong>(QDateTime::currentMSecsSinceEpoch()) >= deadline) {
        return publicError(QStringLiteral("transaction_deadline_expired")).toVariantMap();
    }
    const WalletSubmission submission = m_wallet->submitPublicTransaction({
        programId,
        accountIds,
        signingRequirements,
        instruction,
    });
    if (!submission.accepted())
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
    const QString transactionId = base58TransactionId(submission.nativeHash);
    if (transactionId.isEmpty())
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();

    return QJsonObject {
        { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
        { QStringLiteral("status"), QStringLiteral("submitted") },
        { QStringLiteral("transactionId"), transactionId },
        { QStringLiteral("deadlineMs"), plan.value(QStringLiteral("deadlineMs")) },
    }.toVariantMap();
}
