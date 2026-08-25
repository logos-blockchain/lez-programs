#pragma once

#include <QHash>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>

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

    int open(const QString& config, const QString& storage, const QString& statistics)
    {
        ++openCalls;
        openedConfig = config;
        openedStorage = storage;
        openedStatistics = statistics;
        return openResult;
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

    int save()
    {
        ++saveCalls;
        return saveResult;
    }

    QString create_account_public() { return publicAccountId; }
    QString create_account_private() { return privateAccountId; }

    int get_last_synced_block() const { return lastSyncedBlock; }
    int get_current_block_height() const { return currentBlockHeight; }

    int sync_to_block(quint64)
    {
        ++syncCalls;
        return syncResult;
    }

    QString get_sequencer_addr() const { return sequencerAddress; }

    QVariantList list_accounts()
    {
        ++listCalls;
        return accounts;
    }

    QString get_account_public(const QString& accountId)
    {
        ++publicReadCalls;
        return publicAccounts.value(accountId);
    }

    QString get_balance(const QString& accountId, bool) const
    {
        return balances.value(accountId);
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
};

struct LogosModules {
    LogosModules() = default;
    explicit LogosModules(LogosAPI*) { }

    FakeExecutionZone lez_core;
};
