#pragma once

#include <utility>

#include "WalletProvider.h"

class FakeWalletProvider final : public WalletProvider {
public:
    WalletSession connectResult;
    WalletCreation createWalletResult;
    WalletSnapshot snapshotResult;
    WalletAccountCreation createAccountResult;
    WalletAccountRead readResult;
    WalletSubmission submissionResult;

    int connectCalls = 0;
    int createWalletCalls = 0;
    int snapshotCalls = 0;
    int clearCalls = 0;
    int createAccountCalls = 0;
    mutable int readCalls = 0;
    int submitCalls = 0;
    int disconnectCalls = 0;
    bool lastForceRefresh = false;
    bool lastAccountWasPublic = false;
    WalletPaths lastPaths;
    WalletTransaction lastTransaction;
    bool deferAsync = false;
    SessionCallback pendingConnectCallback;
    SnapshotCallback pendingSnapshotCallback;

    WalletSession connect(const WalletPaths& paths) override
    {
        ++connectCalls;
        lastPaths = paths;
        return connectResult;
    }

    void connectAsync(const WalletPaths& paths, SessionCallback callback) override
    {
        ++connectCalls;
        lastPaths = paths;
        if (deferAsync)
            pendingConnectCallback = std::move(callback);
        else
            callback(connectResult);
    }

    WalletCreation createWallet(const WalletPaths& paths,
                                const QString&) override
    {
        ++createWalletCalls;
        lastPaths = paths;
        return createWalletResult;
    }

    WalletSnapshot snapshot(bool forceRefresh) override
    {
        ++snapshotCalls;
        lastForceRefresh = forceRefresh;
        return snapshotResult;
    }

    void snapshotAsync(bool forceRefresh, SnapshotCallback callback) override
    {
        ++snapshotCalls;
        lastForceRefresh = forceRefresh;
        if (deferAsync)
            pendingSnapshotCallback = std::move(callback);
        else
            callback(snapshotResult);
    }

    void clearSnapshot() override { ++clearCalls; }

    WalletAccountCreation createAccount(bool isPublic) override
    {
        ++createAccountCalls;
        lastAccountWasPublic = isPublic;
        return createAccountResult;
    }

    WalletAccountRead readPublicAccount(const QString& accountId) const override
    {
        ++readCalls;
        WalletAccountRead result = readResult;
        result.accountId = accountId;
        return result;
    }

    WalletSubmission submitPublicTransaction(
        const WalletTransaction& transaction) override
    {
        ++submitCalls;
        lastTransaction = transaction;
        return submissionResult;
    }

    void disconnect() override { ++disconnectCalls; }

    void finishConnect()
    {
        SessionCallback callback = std::move(pendingConnectCallback);
        if (callback)
            callback(connectResult);
    }

    void finishSnapshot()
    {
        SnapshotCallback callback = std::move(pendingSnapshotCallback);
        if (callback)
            callback(snapshotResult);
    }

};
