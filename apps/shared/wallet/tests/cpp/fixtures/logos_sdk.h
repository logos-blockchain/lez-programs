#pragma once

#include <QHash>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>

#include <functional>
#include <utility>

class LogosAPI;

class FakeExecutionZone {
public:
    int openResult = 0;
    int saveResult = 0;
    int syncResult = 0;
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
    int saveCalls = 0;
    int syncCalls = 0;
    int listCalls = 0;
    int publicReadCalls = 0;
    int submitCalls = 0;
    bool deferPublicAccountCreation = false;
    bool deferSubmission = false;
    std::function<void(QString)> pendingPublicAccountCreation;
    std::function<void(QString)> pendingSubmission;
    QString openedConfig;
    QString openedStorage;
    QString createdConfig;
    QString createdStorage;
    QString createdPassword;
    QStringList submittedAccountIds;
    QVariantList submittedSigningRequirements;
    QVariant submittedInstruction;
    QString submittedProgramId;

    int open(const QString& config, const QString& storage)
    {
        ++openCalls;
        openedConfig = config;
        openedStorage = storage;
        return openResult;
    }

    void openAsync(const QString& config,
                   const QString& storage,
                   std::function<void(int)> callback)
    {
        callback(open(config, storage));
    }

    QString create_new(const QString& config,
                       const QString& storage,
                       const QString& password)
    {
        createdConfig = config;
        createdStorage = storage;
        createdPassword = password;
        return mnemonic;
    }

    int save()
    {
        ++saveCalls;
        return saveResult;
    }

    void saveAsync(std::function<void(int)> callback) { callback(save()); }

    QString create_account_public() { return publicAccountId; }
    QString create_account_private() { return privateAccountId; }
    void create_account_publicAsync(std::function<void(QString)> callback)
    {
        if (deferPublicAccountCreation)
            pendingPublicAccountCreation = std::move(callback);
        else
            callback(create_account_public());
    }
    void create_account_privateAsync(std::function<void(QString)> callback)
    {
        callback(create_account_private());
    }

    int get_last_synced_block() const { return lastSyncedBlock; }
    int get_current_block_height() const { return currentBlockHeight; }
    void get_last_synced_blockAsync(std::function<void(int)> callback)
    {
        callback(get_last_synced_block());
    }
    void get_current_block_heightAsync(std::function<void(int)> callback)
    {
        callback(get_current_block_height());
    }

    int sync_to_block(quint64)
    {
        ++syncCalls;
        return syncResult;
    }
    void sync_to_blockAsync(int blockId, std::function<void(int)> callback)
    {
        callback(sync_to_block(static_cast<quint64>(blockId)));
    }

    QString get_sequencer_addr() const { return sequencerAddress; }
    void get_sequencer_addrAsync(std::function<void(QString)> callback)
    {
        callback(get_sequencer_addr());
    }

    QVariantList list_accounts()
    {
        ++listCalls;
        return accounts;
    }
    void list_accountsAsync(std::function<void(QVariantList)> callback)
    {
        callback(list_accounts());
    }

    QString get_account_public(const QString& accountId)
    {
        ++publicReadCalls;
        return publicAccounts.value(accountId);
    }
    void get_account_publicAsync(const QString& accountId,
                                std::function<void(QString)> callback)
    {
        callback(get_account_public(accountId));
    }

    QString get_balance(const QString& accountId, bool) const
    {
        return balances.value(accountId);
    }
    void get_balanceAsync(const QString& accountId,
                         bool isPublic,
                         std::function<void(QString)> callback)
    {
        callback(get_balance(accountId, isPublic));
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

    void send_generic_public_transactionAsync(
        const QStringList& accountIds,
        const QVariantList& signingRequirements,
        const QVariant& instruction,
        const QString& programId,
        std::function<void(QString)> callback)
    {
        ++submitCalls;
        submittedAccountIds = accountIds;
        submittedSigningRequirements = signingRequirements;
        submittedInstruction = instruction;
        submittedProgramId = programId;
        if (deferSubmission)
            pendingSubmission = std::move(callback);
        else
            callback(transactionResponse);
    }

    void finishPublicAccountCreation()
    {
        auto callback = std::move(pendingPublicAccountCreation);
        if (callback)
            callback(publicAccountId);
    }

    void finishSubmission()
    {
        auto callback = std::move(pendingSubmission);
        if (callback)
            callback(transactionResponse);
    }
};

struct LogosModules {
    LogosModules() = default;
    explicit LogosModules(LogosAPI*) { }

    FakeExecutionZone logos_execution_zone;
};
