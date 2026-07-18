#include "NewPositionRuntime.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <utility>

#include <QByteArray>
#include <QDateTime>
#include <QJsonObject>
#include <QPointer>
#include <QScopedValueRollback>
#include <libbase58.h>

#include "AmmClient.h"
#include "SequencerClient.h"
#include "WalletProvider.h"

namespace {
    const char SCHEMA[] = "new-position.v2";
    constexpr qsizetype HASH_BYTES = 32;
    constexpr qsizetype ACCOUNT_ID_HEX_SIZE = HASH_BYTES * 2;
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

    QString accountIdHex(const QString& accountId)
    {
        const QByteArray encoded = accountId.toLatin1();
        std::array<unsigned char, HASH_BYTES> bytes {};
        std::size_t size = bytes.size();
        if (!b58tobin(bytes.data(), &size, encoded.constData(),
                     static_cast<std::size_t>(encoded.size()))
            || size != bytes.size()) {
            return {};
        }
        return QString::fromLatin1(
            QByteArray(reinterpret_cast<const char*>(bytes.data()),
                       static_cast<qsizetype>(bytes.size())).toHex());
    }

    bool isLowerHex(const QString& value, qsizetype size)
    {
        if (value.size() != size)
            return false;
        for (qsizetype index = 0; index < value.size(); ++index) {
            const char16_t character = value.at(index).unicode();
            if (!((character >= u'0' && character <= u'9')
                  || (character >= u'a' && character <= u'f'))) {
                return false;
            }
        }
        return true;
    }

    bool isDefaultAccountRead(const WalletAccountRead& read,
                              const QString& expectedAccountId)
    {
        return read.ok()
            && read.accountId == expectedAccountId
            && isLowerHex(read.accountId, ACCOUNT_ID_HEX_SIZE)
            && read.programOwner == QString(ACCOUNT_ID_HEX_SIZE, QLatin1Char('0'))
            && read.balanceHex == QString(32, QLatin1Char('0'))
            && read.nonceHex == QString(32, QLatin1Char('0'))
            && read.dataHex.isEmpty();
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

NewPositionRuntime::NewPositionRuntime(WalletProvider* wallet,
                                       AmmClient* client,
                                       SequencerClient* sequencer)
    : m_wallet(wallet),
      m_client(client),
      m_sequencer(sequencer)
{
}

void NewPositionRuntime::clearWalletAccounts()
{
    ++m_walletGeneration;
    m_walletPublicAccountIds.clear();
    clearPendingFreshLp();
    m_wallet->clearSnapshot();
    cancelSubmit();
}

void NewPositionRuntime::setWalletAccounts(const QVector<WalletAccount>& accounts)
{
    QStringList publicAccountIds;
    for (const WalletAccount& account : accounts) {
        if (account.isPublic)
            publicAccountIds.append(account.address);
    }
    publicAccountIds.sort(Qt::CaseSensitive);
    if (publicAccountIds != m_walletPublicAccountIds) {
        if (!m_pendingFreshLpAccountId.isEmpty()
            && !publicAccountIds.contains(m_pendingFreshLpAccountId)
            && m_freshLpState == FreshLpState::Ready) {
            clearPendingFreshLp();
        }
        m_walletPublicAccountIds = std::move(publicAccountIds);
        cancelSubmit();
        return;
    }
}

void NewPositionRuntime::rememberPendingFreshLp(const QString& accountId)
{
    m_pendingFreshLpAccountId = accountId;
    m_freshLpState = FreshLpState::Ready;
    if (!m_walletPublicAccountIds.contains(accountId)) {
        m_walletPublicAccountIds.append(accountId);
        m_walletPublicAccountIds.sort(Qt::CaseSensitive);
    }
}

void NewPositionRuntime::clearPendingFreshLp()
{
    m_pendingFreshLpAccountId.clear();
    m_freshLpState = FreshLpState::None;
}

void NewPositionRuntime::cancelSubmit()
{
    if (!m_submitInFlight)
        return;
    ++m_submitGeneration;
    m_submitInFlight = false;
    ResultCallback callback = std::move(m_submitCallback);
    m_submitCallback = {};
    if (callback)
        callback(publicError(QStringLiteral("wallet_unavailable")).toVariantMap());
}

void NewPositionRuntime::finishSubmit(quint64 submitGeneration, QVariantMap result)
{
    if (!m_submitInFlight || submitGeneration != m_submitGeneration)
        return;
    m_submitInFlight = false;
    ResultCallback callback = std::move(m_submitCallback);
    m_submitCallback = {};
    if (callback)
        callback(std::move(result));
}

bool NewPositionRuntime::submitIsCurrent(quint64 submitGeneration,
                                         quint64 walletGeneration) const
{
    return m_submitInFlight
        && submitGeneration == m_submitGeneration
        && walletGeneration == m_walletGeneration;
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

void NewPositionRuntime::contextAsync(const QVariantMap& request,
                                      const ActiveNetworkSnapshot& network,
                                      bool walletOpen,
                                      bool refreshPublicData,
                                      ResultCallback callback)
{
    if (network.status != QStringLiteral("ready")) {
        callback(contextState(network.status, network).toVariantMap());
        return;
    }
    if (!m_sequencer || !m_sequencer->isConfigured()) {
        callback(contextState(QStringLiteral("error"), network,
                              QStringLiteral("sequencer_config_required")).toVariantMap());
        return;
    }

    const AmmClientResult configResult = m_client->configId(
        QJsonObject { { QStringLiteral("ammProgramId"), network.ammProgramId } });
    if (!configResult.ok) {
        callback(contextState(QStringLiteral("error"), network,
                              QStringLiteral("backend_error")).toVariantMap());
        return;
    }
    const QString configId = configResult.value.value(
        QStringLiteral("configId")).toString();
    QPointer<NewPositionRuntime> guard(this);
    m_sequencer->readAccounts({ configId }, refreshPublicData,
        [guard, request, network, walletOpen, refreshPublicData,
         callback = std::move(callback)](QVector<WalletAccountRead> configReads) mutable {
            if (!guard)
                return;
            const QJsonObject config = accountReadJson(configReads.value(0));
            guard->m_sequencer->readAccounts(
                guard->m_walletPublicAccountIds, refreshPublicData,
                [guard, request, network, walletOpen, refreshPublicData, config,
                 callback = std::move(callback)](
                    QVector<WalletAccountRead> walletReads) mutable {
                    if (!guard)
                        return;
                    const QJsonObject hints = QJsonObject::fromVariantMap(request);
                    QJsonArray configured;
                    for (const QString& id : network.tokenIds)
                        configured.append(id);
                    const QJsonArray recent = variantStringArray(
                        hints.value(QStringLiteral("recentTokenIds")).toVariant());
                    const QJsonArray resolved = variantStringArray(
                        hints.value(QStringLiteral("resolvedTokenIds")).toVariant());
                    const QJsonArray walletAccounts = accountReadsJson(walletReads);
                    const AmmClientResult tokenResult = guard->m_client->tokenIds(QJsonObject {
                        { QStringLiteral("ammProgramId"), network.ammProgramId },
                        { QStringLiteral("config"), config },
                        { QStringLiteral("walletAccounts"), walletAccounts },
                        { QStringLiteral("configuredTokenIds"), configured },
                        { QStringLiteral("recentTokenIds"), recent },
                        { QStringLiteral("resolvedTokenIds"), resolved },
                    });
                    if (!tokenResult.ok
                        || tokenResult.value.value(QStringLiteral("status")).toString()
                            != QStringLiteral("ok")) {
                        const QString code = tokenResult.ok
                            ? tokenResult.value.value(QStringLiteral("code")).toString()
                            : QStringLiteral("backend_error");
                        callback(contextState(QStringLiteral("error"), network,
                            code.isEmpty() ? QStringLiteral("backend_error") : code)
                            .toVariantMap());
                        return;
                    }

                    QStringList definitionIds;
                    for (const QJsonValue& id : tokenResult.value
                             .value(QStringLiteral("tokenIds")).toArray()) {
                        definitionIds.append(id.toString());
                    }
                    guard->m_sequencer->readAccounts(definitionIds, refreshPublicData,
                        [guard, network, walletOpen, config, walletAccounts,
                         configured, recent, resolved,
                         callback = std::move(callback)](
                            QVector<WalletAccountRead> definitions) mutable {
                            if (!guard)
                                return;
                            const AmmClientResult result = guard->m_client->context(QJsonObject {
                                { QStringLiteral("networkId"), network.id },
                                { QStringLiteral("networkFingerprint"), network.fingerprint },
                                { QStringLiteral("ammProgramId"), network.ammProgramId },
                                { QStringLiteral("walletAvailable"), walletOpen },
                                { QStringLiteral("config"), config },
                                { QStringLiteral("walletAccounts"), walletAccounts },
                                { QStringLiteral("tokenDefinitions"),
                                  accountReadsJson(definitions) },
                                { QStringLiteral("configuredTokenIds"), configured },
                                { QStringLiteral("recentTokenIds"), recent },
                                { QStringLiteral("resolvedTokenIds"), resolved },
                            });
                            callback((result.ok
                                ? result.value
                                : contextState(QStringLiteral("error"), network,
                                      QStringLiteral("backend_error"))).toVariantMap());
                        });
                });
        });
}

void NewPositionRuntime::buildQuoteInputAsync(
    const QVariantMap& request,
    const ActiveNetworkSnapshot& network,
    bool walletOpen,
    bool forceRefresh,
    std::function<void(QJsonObject, QJsonObject)> callback)
{
    if (network.status != QStringLiteral("ready")) {
        callback({}, publicError(network.status));
        return;
    }
    if (!m_sequencer || !m_sequencer->isConfigured()) {
        callback({}, publicError(QStringLiteral("sequencer_config_required")));
        return;
    }
    const AmmClientResult configResult = m_client->configId(
        QJsonObject { { QStringLiteral("ammProgramId"), network.ammProgramId } });
    if (!configResult.ok) {
        callback({}, publicError(QStringLiteral("backend_error")));
        return;
    }
    const QString configId = configResult.value.value(
        QStringLiteral("configId")).toString();
    QPointer<NewPositionRuntime> guard(this);
    m_sequencer->readAccounts({ configId }, forceRefresh,
        [guard, request, network, walletOpen, forceRefresh,
         callback = std::move(callback)](QVector<WalletAccountRead> configReads) mutable {
            if (!guard)
                return;
            const QJsonObject config = accountReadJson(configReads.value(0));
            const QJsonObject requestObject = QJsonObject::fromVariantMap(request);
            const AmmClientResult pairResult = guard->m_client->pairIds(QJsonObject {
                { QStringLiteral("ammProgramId"), network.ammProgramId },
                { QStringLiteral("config"), config },
                { QStringLiteral("tokenAId"),
                  requestObject.value(QStringLiteral("tokenAId")) },
                { QStringLiteral("tokenBId"),
                  requestObject.value(QStringLiteral("tokenBId")) },
            });
            if (!pairResult.ok
                || pairResult.value.value(QStringLiteral("status")).toString()
                    != QStringLiteral("ok")) {
                const QString code = pairResult.ok
                    ? pairResult.value.value(QStringLiteral("code")).toString()
                    : QStringLiteral("backend_error");
                callback({}, publicError(code.isEmpty()
                    ? QStringLiteral("backend_error") : code));
                return;
            }
            const QJsonObject pair = pairResult.value;
            const QStringList fixedIds {
                pair.value(QStringLiteral("tokenAId")).toString(),
                pair.value(QStringLiteral("tokenBId")).toString(),
                pair.value(QStringLiteral("poolId")).toString(),
                pair.value(QStringLiteral("vaultAId")).toString(),
                pair.value(QStringLiteral("vaultBId")).toString(),
                pair.value(QStringLiteral("lpDefinitionId")).toString(),
                pair.value(QStringLiteral("lpLockHoldingId")).toString(),
                pair.value(QStringLiteral("currentTickId")).toString(),
                pair.value(QStringLiteral("clockId")).toString(),
            };
            guard->m_sequencer->readAccounts(fixedIds, forceRefresh,
                [guard, requestObject, network, walletOpen, config, pair,
                 callback = std::move(callback)](
                    QVector<WalletAccountRead> fixedReads) mutable {
                    if (!guard)
                        return;
                    QStringList selectedIds;
                    for (const QString& key : {
                             QStringLiteral("holdingAId"),
                             QStringLiteral("holdingBId"),
                             QStringLiteral("lpHoldingId") }) {
                        const QString id = accountIdHex(
                            requestObject.value(key).toString());
                        if (!id.isEmpty() && !selectedIds.contains(id))
                            selectedIds.append(id);
                    }
                    QStringList walletIds = guard->m_walletPublicAccountIds;
                    for (const QString& id : selectedIds)
                        walletIds.removeAll(id);
                    guard->m_sequencer->readAccounts(
                        walletIds, false,
                        [guard, requestObject, network, walletOpen, config, pair,
                         selectedIds = std::move(selectedIds),
                         fixedReads = std::move(fixedReads),
                         callback = std::move(callback)](
                            QVector<WalletAccountRead> walletReads) mutable {
                            if (!guard)
                                return;
                            auto finish = [requestObject, network, walletOpen, config,
                                           fixedReads = std::move(fixedReads),
                                           walletReads = std::move(walletReads),
                                           callback = std::move(callback)](
                                              QVector<WalletAccountRead> selectedReads) mutable {
                                QHash<QString, WalletAccountRead> walletById;
                                for (const WalletAccountRead& read : walletReads)
                                    walletById.insert(read.accountId, read);
                                for (const WalletAccountRead& read : selectedReads)
                                    walletById.insert(read.accountId, read);
                                const QJsonArray walletAccounts = accountReadsJson(
                                    walletById.values());
                                const QJsonObject snapshot {
                                    { QStringLiteral("config"), config },
                                    { QStringLiteral("tokenA"), accountReadJson(fixedReads.value(0)) },
                                    { QStringLiteral("tokenB"), accountReadJson(fixedReads.value(1)) },
                                    { QStringLiteral("pool"), accountReadJson(fixedReads.value(2)) },
                                    { QStringLiteral("vaultA"), accountReadJson(fixedReads.value(3)) },
                                    { QStringLiteral("vaultB"), accountReadJson(fixedReads.value(4)) },
                                    { QStringLiteral("lpDefinition"), accountReadJson(fixedReads.value(5)) },
                                    { QStringLiteral("lpLockHolding"), accountReadJson(fixedReads.value(6)) },
                                    { QStringLiteral("currentTick"), accountReadJson(fixedReads.value(7)) },
                                    { QStringLiteral("clock"), accountReadJson(fixedReads.value(8)) },
                                    { QStringLiteral("walletAvailable"), walletOpen },
                                    { QStringLiteral("walletAccounts"), walletAccounts },
                                };
                                callback(QJsonObject {
                                    { QStringLiteral("networkId"), network.id },
                                    { QStringLiteral("networkFingerprint"), network.fingerprint },
                                    { QStringLiteral("ammProgramId"), network.ammProgramId },
                                    { QStringLiteral("request"), requestObject },
                                    { QStringLiteral("snapshot"), snapshot },
                                }, {});
                            };
                            if (selectedIds.isEmpty())
                                finish({});
                            else
                                guard->m_sequencer->readAccounts(
                                    selectedIds, true, std::move(finish));
                        });
                });
        });
}

void NewPositionRuntime::quoteAsync(const QVariantMap& request,
                                    const ActiveNetworkSnapshot& network,
                                    bool walletOpen,
                                    bool forceRefresh,
                                    ResultCallback callback)
{
    QPointer<NewPositionRuntime> guard(this);
    buildQuoteInputAsync(request, network, walletOpen, forceRefresh,
        [guard, callback = std::move(callback)](
            QJsonObject input, QJsonObject error) mutable {
            if (!guard)
                return;
            if (!error.isEmpty()) {
                callback(error.toVariantMap());
                return;
            }
            const AmmClientResult result = guard->m_client->quote(input);
            callback((result.ok ? result.value
                                : publicError(QStringLiteral("backend_error")))
                         .toVariantMap());
        });
}

void NewPositionRuntime::submitAsync(const QVariantMap& request,
                                     const QString& quoteHash,
                                     const ActiveNetworkSnapshot& network,
                                     bool walletCanSubmit,
                                     ResultCallback callback)
{
    if (m_submitInFlight) {
        callback(publicError(QStringLiteral("submit_in_progress")).toVariantMap());
        return;
    }
    if (!walletCanSubmit) {
        callback(publicError(QStringLiteral("wallet_syncing")).toVariantMap());
        return;
    }
    if (m_freshLpState == FreshLpState::Creating
        || m_freshLpState == FreshLpState::Submitting) {
        callback(publicError(QStringLiteral("submit_in_progress")).toVariantMap());
        return;
    }
    m_submitInFlight = true;
    const quint64 submitGeneration = ++m_submitGeneration;
    const quint64 walletGeneration = m_walletGeneration;
    m_submitCallback = std::move(callback);
    QPointer<NewPositionRuntime> guard(this);
    buildQuoteInputAsync(request, network, true, true,
        [guard, quoteHash, submitGeneration, walletGeneration](
            QJsonObject input, QJsonObject error) mutable {
            if (!guard
                || !guard->submitIsCurrent(submitGeneration, walletGeneration))
                return;
            if (!error.isEmpty()) {
                guard->finishSubmit(submitGeneration, error.toVariantMap());
                return;
            }
            const AmmClientResult quoteResult = guard->m_client->quote(input);
            if (!quoteResult.ok) {
                guard->finishSubmit(
                    submitGeneration,
                    publicError(QStringLiteral("backend_error")).toVariantMap());
                return;
            }
            const QJsonObject quote = quoteResult.value;
            if (quote.value(QStringLiteral("quoteHash")).toString() != quoteHash) {
                QJsonObject result = publicError(QStringLiteral("quote_changed"));
                result.insert(QStringLiteral("quote"), quote);
                guard->finishSubmit(submitGeneration, result.toVariantMap());
                return;
            }
            if (!quote.value(QStringLiteral("canSubmit")).toBool(false)) {
                QJsonObject result = publicError(QStringLiteral("quote_not_submittable"));
                result.insert(QStringLiteral("quote"), quote);
                guard->finishSubmit(submitGeneration, result.toVariantMap());
                return;
            }

            if (quote.value(QStringLiteral("requiresFreshLp")).toBool(false)) {
                guard->prepareFreshLpAsync(
                    std::move(input), quoteHash,
                    submitGeneration, walletGeneration);
                return;
            }
            guard->submitPlanAsync(
                std::move(input), quoteHash, {}, {},
                submitGeneration, walletGeneration);
        });
}

void NewPositionRuntime::prepareFreshLpAsync(QJsonObject input,
                                             const QString& quoteHash,
                                             quint64 submitGeneration,
                                             quint64 walletGeneration)
{
    if (!submitIsCurrent(submitGeneration, walletGeneration))
        return;
    if (m_freshLpState == FreshLpState::Ready
        && !m_pendingFreshLpAccountId.isEmpty()) {
        validatePendingFreshLpAsync(
            std::move(input), quoteHash,
            submitGeneration, walletGeneration);
        return;
    }
    if (m_freshLpState != FreshLpState::None) {
        finishSubmit(
            submitGeneration,
            publicError(QStringLiteral("submit_in_progress")).toVariantMap());
        return;
    }

    m_freshLpState = FreshLpState::Creating;
    QPointer<NewPositionRuntime> guard(this);
    m_wallet->createAccountAsync(
        true,
        [guard, input = std::move(input), quoteHash,
         submitGeneration, walletGeneration](
            WalletAccountCreation creation) mutable {
            if (!guard || walletGeneration != guard->m_walletGeneration)
                return;

            const bool reusable = creation.ok()
                && isLowerHex(creation.accountId, ACCOUNT_ID_HEX_SIZE);
            if (reusable)
                guard->rememberPendingFreshLp(creation.accountId);
            else
                guard->clearPendingFreshLp();
            if (!guard->submitIsCurrent(submitGeneration, walletGeneration))
                return;
            if (!reusable) {
                const QString code = creation.failure
                        == WalletFailure::WalletUnavailable
                    ? QStringLiteral("wallet_unavailable")
                    : QStringLiteral("wallet_submission_failed");
                guard->finishSubmit(
                    submitGeneration, publicError(code).toVariantMap());
                return;
            }

            WalletAccountRead read = std::move(creation.publicAccount);
            read.accountId = creation.accountId;
            if (isDefaultAccountRead(read, creation.accountId)) {
                guard->submitPlanAsync(
                    std::move(input), quoteHash, accountReadJson(read),
                    creation.accountId, submitGeneration, walletGeneration);
                return;
            }
            guard->validatePendingFreshLpAsync(
                std::move(input), quoteHash,
                submitGeneration, walletGeneration);
        });
}

void NewPositionRuntime::validatePendingFreshLpAsync(
    QJsonObject input,
    const QString& quoteHash,
    quint64 submitGeneration,
    quint64 walletGeneration)
{
    if (!submitIsCurrent(submitGeneration, walletGeneration))
        return;
    const QString accountId = m_pendingFreshLpAccountId;
    if (m_freshLpState != FreshLpState::Ready
        || accountId.isEmpty()
        || !m_sequencer
        || !m_sequencer->isConfigured()) {
        finishSubmit(
            submitGeneration,
            publicError(QStringLiteral("account_read_failed")).toVariantMap());
        return;
    }

    QPointer<NewPositionRuntime> guard(this);
    m_sequencer->readAccounts(
        { accountId }, true,
        [guard, input = std::move(input), quoteHash, accountId,
         submitGeneration, walletGeneration](
            QVector<WalletAccountRead> reads) mutable {
            if (!guard
                || !guard->submitIsCurrent(
                    submitGeneration, walletGeneration)) {
                return;
            }
            if (guard->m_pendingFreshLpAccountId != accountId) {
                guard->finishSubmit(
                    submitGeneration,
                    publicError(QStringLiteral("wallet_unavailable")).toVariantMap());
                return;
            }
            const WalletAccountRead read = reads.value(0);
            if (!read.ok()) {
                guard->finishSubmit(
                    submitGeneration,
                    publicError(QStringLiteral("account_read_failed")).toVariantMap());
                return;
            }
            if (!isDefaultAccountRead(read, accountId)) {
                guard->clearPendingFreshLp();
                guard->finishSubmit(
                    submitGeneration,
                    publicError(QStringLiteral("submission_status_unknown")).toVariantMap());
                return;
            }
            guard->submitPlanAsync(
                std::move(input), quoteHash, accountReadJson(read), accountId,
                submitGeneration, walletGeneration);
        });
}

void NewPositionRuntime::submitPlanAsync(QJsonObject input,
                                         const QString& quoteHash,
                                         QJsonValue freshLp,
                                         QString freshLpAccountId,
                                         quint64 submitGeneration,
                                         quint64 walletGeneration)
{
    if (!submitIsCurrent(submitGeneration, walletGeneration))
        return;

    input.insert(QStringLiteral("quoteHash"), quoteHash);
    input.insert(QStringLiteral("nowMs"), QDateTime::currentMSecsSinceEpoch());
    if (!freshLp.isUndefined() && !freshLp.isNull())
        input.insert(QStringLiteral("freshLp"), std::move(freshLp));
    const AmmClientResult planResult = m_client->plan(input);
    if (!planResult.ok) {
        finishSubmit(
            submitGeneration,
            publicError(QStringLiteral("backend_error")).toVariantMap());
        return;
    }
    const QJsonObject plan = planResult.value;
    if (plan.value(QStringLiteral("status")).toString()
        != QStringLiteral("ready")) {
        const QString code = plan.value(QStringLiteral("code")).toString();
        finishSubmit(
            submitGeneration,
            publicError(code.isEmpty()
                ? QStringLiteral("wallet_submission_failed") : code).toVariantMap());
        return;
    }

    bool deadlineValid = false;
    const qulonglong deadline = plan.value(QStringLiteral("deadlineMs"))
        .toString().toULongLong(&deadlineValid);
    if (!deadlineValid
        || static_cast<qulonglong>(QDateTime::currentMSecsSinceEpoch()) >= deadline) {
        finishSubmit(
            submitGeneration,
            publicError(QStringLiteral("transaction_deadline_expired")).toVariantMap());
        return;
    }
    if (!submitIsCurrent(submitGeneration, walletGeneration))
        return;

    WalletTransaction transaction {
        plan.value(QStringLiteral("programId")).toString(),
        jsonStringList(plan.value(QStringLiteral("accountIds")).toArray()),
        jsonBoolList(plan.value(QStringLiteral("signingRequirements")).toArray()),
        jsonUIntList(plan.value(QStringLiteral("instruction")).toArray()),
    };
    if (!freshLpAccountId.isEmpty()
        && freshLpAccountId == m_pendingFreshLpAccountId) {
        m_freshLpState = FreshLpState::Submitting;
    }
    QPointer<NewPositionRuntime> guard(this);
    m_wallet->submitPublicTransactionAsync(
        transaction,
        [guard, submitGeneration, walletGeneration,
         freshLpAccountId = std::move(freshLpAccountId), deadlineMs =
             plan.value(QStringLiteral("deadlineMs")), affectedAccountIds =
             plan.value(QStringLiteral("affectedAccountIds"))](
                WalletSubmission submission) mutable {
            if (!guard)
                return;
            const QString transactionId = submission.accepted()
                ? base58TransactionId(submission.nativeHash) : QString();
            if (walletGeneration == guard->m_walletGeneration
                && !freshLpAccountId.isEmpty()
                && freshLpAccountId == guard->m_pendingFreshLpAccountId) {
                if (submission.accepted())
                    guard->clearPendingFreshLp();
                else
                    guard->m_freshLpState = FreshLpState::Ready;
            }
            if (!guard->submitIsCurrent(submitGeneration, walletGeneration))
                return;
            if (transactionId.isEmpty()) {
                const QString code = submission.failure
                        == WalletFailure::WalletUnavailable
                    ? QStringLiteral("wallet_unavailable")
                    : QStringLiteral("wallet_submission_failed");
                guard->finishSubmit(
                    submitGeneration, publicError(code).toVariantMap());
                return;
            }
            guard->finishSubmit(submitGeneration, QJsonObject {
                { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
                { QStringLiteral("status"), QStringLiteral("submitted") },
                { QStringLiteral("transactionId"), transactionId },
                { QStringLiteral("nativeTransactionHash"), submission.nativeHash },
                { QStringLiteral("deadlineMs"), deadlineMs },
                { QStringLiteral("affectedAccountIds"),
                  affectedAccountIds },
            }.toVariantMap());
        });
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
    if (m_freshLpState == FreshLpState::Creating
        || m_freshLpState == FreshLpState::Submitting) {
        return publicError(QStringLiteral("submit_in_progress")).toVariantMap();
    }
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
    QString freshLpAccountId;
    if (quote.value(QStringLiteral("requiresFreshLp")).toBool(false)) {
        WalletAccountRead read;
        if (m_freshLpState == FreshLpState::None) {
            m_freshLpState = FreshLpState::Creating;
            const WalletAccountCreation creation = m_wallet->createAccount(true);
            if (!creation.ok()
                || !isLowerHex(creation.accountId, ACCOUNT_ID_HEX_SIZE)) {
                clearPendingFreshLp();
                return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
            }
            rememberPendingFreshLp(creation.accountId);
            read = creation.publicAccount;
            read.accountId = creation.accountId;
        } else if (m_freshLpState == FreshLpState::Ready
                   && !m_pendingFreshLpAccountId.isEmpty()) {
            read = m_wallet->readPublicAccount(m_pendingFreshLpAccountId);
        } else {
            return publicError(QStringLiteral("submit_in_progress")).toVariantMap();
        }
        freshLpAccountId = m_pendingFreshLpAccountId;
        if (!read.ok())
            return publicError(QStringLiteral("account_read_failed")).toVariantMap();
        if (!isDefaultAccountRead(read, freshLpAccountId)) {
            clearPendingFreshLp();
            return publicError(QStringLiteral("submission_status_unknown")).toVariantMap();
        }
        freshLp = accountReadJson(read);
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
    if (!freshLpAccountId.isEmpty()
        && freshLpAccountId == m_pendingFreshLpAccountId) {
        m_freshLpState = FreshLpState::Submitting;
    }
    const WalletSubmission submission = m_wallet->submitPublicTransaction({
        programId,
        accountIds,
        signingRequirements,
        instruction,
    });
    if (!submission.accepted()) {
        if (!freshLpAccountId.isEmpty()
            && freshLpAccountId == m_pendingFreshLpAccountId) {
            m_freshLpState = FreshLpState::Ready;
        }
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
    }
    if (!freshLpAccountId.isEmpty()
        && freshLpAccountId == m_pendingFreshLpAccountId) {
        clearPendingFreshLp();
    }
    const QString transactionId = base58TransactionId(submission.nativeHash);
    if (transactionId.isEmpty())
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();

    return QJsonObject {
        { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
        { QStringLiteral("status"), QStringLiteral("submitted") },
        { QStringLiteral("transactionId"), transactionId },
        { QStringLiteral("nativeTransactionHash"), submission.nativeHash },
        { QStringLiteral("deadlineMs"), plan.value(QStringLiteral("deadlineMs")) },
    }.toVariantMap();
}
