#pragma once

#include <memory>

#include "WalletProvider.h"

class LogosAPI;
struct LogosModules;

class LogosWalletProvider final : public WalletProvider {
public:
    explicit LogosWalletProvider(LogosAPI* api);
    explicit LogosWalletProvider(LogosModules* logos);
    ~LogosWalletProvider() override;

    WalletSession connect(const WalletPaths& paths) override;
    WalletCreation createWallet(const WalletPaths& paths,
                                const QString& password) override;
    WalletSnapshot snapshot(bool forceRefresh = false) override;
    void clearSnapshot() override;
    WalletAccountCreation createAccount(bool isPublic) override;
    WalletAccountRead readPublicAccount(const QString& accountId) const override;
    WalletSubmission submitPublicTransaction(
        const WalletTransaction& transaction) override;
    void disconnect() override;

private:
    bool sharedWalletIsOpen() const;
    WalletSnapshot loadSnapshot();
    bool save() const;

    struct Impl;
    std::unique_ptr<Impl> m_impl;
    WalletSnapshot m_snapshot;
    bool m_snapshotReady = false;
    bool m_connected = false;
};
