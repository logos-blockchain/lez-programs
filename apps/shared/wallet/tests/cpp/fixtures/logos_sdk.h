#pragma once

#include <QHash>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QVariant>
#include <QVariantList>

#include <functional>
#include <utility>
#include <vector>

struct Timeout {
    explicit Timeout(int = 20000) { }
};

class LogosAPI;

class FakeLezCore {
public:
    // Match the generated lez_core caller contract; delay selected replies to
    // exercise module teardown while IPC is in flight.
    QString deferredMethod;
    int deferredOccurrence = 1;
    std::vector<std::function<void()>> pendingReplies;

    void completePendingReplies()
    {
        auto replies = std::move(pendingReplies);
        pendingReplies.clear();
        for (auto& reply : replies)
            reply();
    }

    QString versionValue = QStringLiteral("1.0");
    int versionFailuresRemaining = 0;
    int openResult = 0;
    int saveResult = 0;
    int syncResult = 0;
    bool deferSync = false;
    std::function<void(int)> pendingSync;
    int lastSyncedBlock = 0;
    int currentBlockHeight = 0;
    QString sequencerAddress;
    QString mnemonic = QStringLiteral("one two three");
    QString publicAccountId;
    QString privateAccountId;
    QString transactionResponse;
    QVariantList accounts;
    QHash<QString, QString> publicAccounts;
    QHash<QString, QString> balances;

    int openCalls = 0;
    int versionCalls = 0;
    int saveCalls = 0;
    int createAsyncCalls = 0;
    int syncCalls = 0;
    int listCalls = 0;
    int publicReadCalls = 0;
    int submitCalls = 0;
    QString openedConfig;
    QString openedStorage;
    QString openedStatistics;
    QString createdConfig;
    QString createdStorage;
    QString createdStatistics;
    QString createdPassword;
    QStringList submittedAccountIds;
    QVariantList submittedSigningRequirements;
    QVariant submittedInstruction;
    QString submittedProgramId;

    QString version()
    {
        ++versionCalls;
        if (versionFailuresRemaining != 0) {
            if (versionFailuresRemaining > 0)
                --versionFailuresRemaining;
            return {};
        }
        return versionValue;
    }

    void versionAsync(std::function<void(QString)> callback)
    {
        deliver(QStringLiteral("version"), std::move(callback), version());
    }

    int open(const QString& config, const QString& storage, const QString& statistics)
    {
        ++openCalls;
        openedConfig = config;
        openedStorage = storage;
        openedStatistics = statistics;
        return openResult;
    }

    void openAsync(const QString& config,
                   const QString& storage,
                   const QString& statistics,
                   std::function<void(int)> callback)
    {
        deliver(QStringLiteral("open"), std::move(callback), open(config, storage, statistics));
    }

    QString create_new(const QString& config,
                       const QString& storage,
                       const QString& statistics,
                       const QString& password)
    {
        createdConfig = config;
        createdStorage = storage;
        createdStatistics = statistics;
        createdPassword = password;
        return mnemonic;
    }

    void create_newAsync(const QString& config,
                         const QString& storage,
                         const QString& statistics,
                         const QString& password,
                         std::function<void(QString)> callback,
                         Timeout = Timeout())
    {
        ++createAsyncCalls;
        QTimer::singleShot(0, [this, config, storage, statistics, password,
                               callback = std::move(callback)]() mutable {
            callback(create_new(config, storage, statistics, password));
        });
    }

    int save()
    {
        ++saveCalls;
        return saveResult;
    }

    void saveAsync(std::function<void(int)> callback)
    {
        deliver(QStringLiteral("save"), std::move(callback), save());
    }

    QString create_account_public() { return publicAccountId; }
    QString create_account_private() { return privateAccountId; }

    int get_last_synced_block() const { return lastSyncedBlock; }
    int get_current_block_height() const { return currentBlockHeight; }

    void get_last_synced_blockAsync(std::function<void(int)> callback)
    {
        deliver(QStringLiteral("get_last_synced_block"), std::move(callback), get_last_synced_block());
    }

    void get_current_block_heightAsync(std::function<void(int)> callback)
    {
        deliver(QStringLiteral("get_current_block_height"), std::move(callback), get_current_block_height());
    }

    int sync_to_block(int)
    {
        ++syncCalls;
        return syncResult;
    }

    void sync_to_blockAsync(int blockId, std::function<void(int)> callback)
    {
        const int result = sync_to_block(blockId);
        if (deferSync)
            pendingSync = std::move(callback);
        else
            deliver(QStringLiteral("sync_to_block"), std::move(callback), result);
    }

    void finishSync()
    {
        auto callback = std::move(pendingSync);
        if (callback)
            callback(syncResult);
    }

    QString get_sequencer_addr() const { return sequencerAddress; }

    void get_sequencer_addrAsync(std::function<void(QString)> callback)
    {
        deliver(QStringLiteral("get_sequencer_addr"), std::move(callback), get_sequencer_addr());
    }

    QVariantList list_accounts()
    {
        ++listCalls;
        return accounts;
    }

    void list_accountsAsync(std::function<void(QVariantList)> callback)
    {
        deliver(QStringLiteral("list_accounts"), std::move(callback), list_accounts());
    }

    QString get_account_public(const QString& accountId)
    {
        ++publicReadCalls;
        return publicAccounts.value(accountId);
    }

    void get_account_publicAsync(const QString& accountId,
                                 std::function<void(QString)> callback)
    {
        deliver(QStringLiteral("get_account_public"), std::move(callback), get_account_public(accountId));
    }

    QString get_balance(const QString& accountId, bool) const
    {
        return balances.value(accountId);
    }

    void get_balanceAsync(const QString& accountId,
                          bool isPublic,
                          std::function<void(QString)> callback)
    {
        deliver(QStringLiteral("get_balance"), std::move(callback), get_balance(accountId, isPublic));
    }

    QString send_generic_public_transaction(
        const QStringList& accountIds,
        const QVariantList& signingRequirements,
        const QVariant& instruction,
        const QString& programId)
    {
        ++submitCalls;
        submittedAccountIds = accountIds;
        submittedSigningRequirements = signingRequirements;
        submittedInstruction = instruction;
        submittedProgramId = programId;
        return transactionResponse;
    }

private:
    template<typename Result>
    void deliver(const QString& method, std::function<void(Result)> callback, Result result)
    {
        if (method == deferredMethod && --deferredOccurrence == 0) {
            pendingReplies.push_back([callback = std::move(callback),
                                      result = std::move(result)]() mutable {
                callback(std::move(result));
            });
            return;
        }
        callback(std::move(result));
    }
};

struct LogosModules {
    LogosModules() = default;
    explicit LogosModules(LogosAPI*) { }

    FakeLezCore lez_core;
};
