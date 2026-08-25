#pragma once

#include <QString>
#include <QStringList>
#include <QVector>

enum class WalletFailure {
    None,
    WalletMissing,
    WalletUnavailable,
    OpenFailed,
    CreateFailed,
    SaveFailed,
    ReadFailed,
    InvalidRequest,
    SubmissionFailed,
};

QString walletFailureCode(WalletFailure failure);

struct WalletPaths {
    QString config;
    QString storage;
    // Sequencer-calibration statistics cache (v0.2.1 lez_core open/create_new take
    // this path). It need not exist — the wallet starts from empty stats and
    // recalibrates; it just lives alongside config/storage in the wallet home.
    QString statistics;
};

struct WalletAccountRead {
    QString accountId;
    QString status = QStringLiteral("read_failed");
    QString programOwner;
    QString balanceHex;
    QString nonceHex;
    QString dataHex;

    bool ok() const { return status == QStringLiteral("ok"); }
};

struct WalletAccount {
    QString address;
    QString balance;
    bool isPublic = true;
};

struct WalletSnapshot {
    WalletFailure failure = WalletFailure::None;
    QVector<WalletAccount> accounts;
    QVector<WalletAccountRead> publicAccountReads;
    quint64 lastSyncedBlock = 0;
    quint64 currentBlockHeight = 0;
    QString sequencerAddress;

    bool ok() const { return failure == WalletFailure::None; }
};

struct WalletSession {
    WalletFailure failure = WalletFailure::None;
    WalletSnapshot snapshot;
    bool adopted = false;

    bool ok() const { return failure == WalletFailure::None; }
};

struct WalletCreation {
    WalletFailure failure = WalletFailure::None;
    QString mnemonic;
    WalletSnapshot snapshot;

    bool ok() const { return failure == WalletFailure::None; }
};

struct WalletAccountCreation {
    WalletFailure failure = WalletFailure::None;
    QString accountId;
    WalletAccountRead publicAccount;
    WalletSnapshot snapshot;

    bool ok() const { return failure == WalletFailure::None; }
};

struct WalletTransaction {
    QString programId;
    QStringList accountIds;
    QVector<bool> signingRequirements;
    QVector<quint32> instruction;
};

struct WalletSubmission {
    WalletFailure failure = WalletFailure::None;
    QString nativeHash;

    bool accepted() const { return failure == WalletFailure::None && !nativeHash.isEmpty(); }
};

class WalletProvider {
public:
    virtual ~WalletProvider() = default;

    virtual WalletSession connect(const WalletPaths& paths) = 0;
    virtual WalletCreation createWallet(const WalletPaths& paths,
                                        const QString& password) = 0;
    virtual WalletSnapshot snapshot(bool forceRefresh = false) = 0;
    virtual void clearSnapshot() = 0;
    virtual WalletAccountCreation createAccount(bool isPublic) = 0;
    virtual WalletAccountRead readPublicAccount(const QString& accountId) const = 0;
    virtual WalletSubmission submitPublicTransaction(
        const WalletTransaction& transaction) = 0;
    virtual void disconnect() = 0;
};
