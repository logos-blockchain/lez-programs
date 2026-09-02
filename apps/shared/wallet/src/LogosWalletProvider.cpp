#include "LogosWalletProvider.h"

#include <QByteArray>
#include <algorithm>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>

#include "logos_sdk.h"

namespace {
constexpr int WALLET_FFI_SUCCESS = 0;

bool isHex(const QString& value, qsizetype size, bool lowercaseOnly = true)
{
    if (value.size() != size)
        return false;

    for (const QChar character : value) {
        const bool digit = character >= QLatin1Char('0')
            && character <= QLatin1Char('9');
        const bool lowercase = character >= QLatin1Char('a')
            && character <= QLatin1Char('f');
        const bool uppercase = character >= QLatin1Char('A')
            && character <= QLatin1Char('F');
        if (!digit && !lowercase && (!uppercase || lowercaseOnly))
            return false;
    }
    return true;
}

QString littleEndianU128ToDecimal(const QString& value)
{
    if (!isHex(value, 32))
        return {};

    QString decimal = QStringLiteral("0");
    for (int byteIndex = 15; byteIndex >= 0; --byteIndex) {
        bool parsed = false;
        int carry = value.mid(byteIndex * 2, 2).toInt(&parsed, 16);
        if (!parsed)
            return {};

        for (qsizetype digit = decimal.size(); digit-- > 0;) {
            const int next = (decimal.at(digit).unicode() - QLatin1Char('0').unicode())
                * 256 + carry;
            decimal[digit] = QLatin1Char(static_cast<char>('0' + next % 10));
            carry = next / 10;
        }
        while (carry > 0) {
            decimal.prepend(QLatin1Char(static_cast<char>('0' + carry % 10)));
            carry /= 10;
        }
    }

    return decimal;
}

WalletSession failedSession(WalletFailure failure)
{
    WalletSession session;
    session.failure = failure;
    session.snapshot.failure = failure;
    return session;
}

WalletCreation failedCreation(WalletFailure failure)
{
    WalletCreation creation;
    creation.failure = failure;
    creation.snapshot.failure = failure;
    return creation;
}

WalletAccountRead parsePublicAccount(const QString& accountId, const QString& payload)
{
    WalletAccountRead read;
    read.accountId = accountId;
    if (!isHex(accountId, 64))
        return read;

    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(payload.toUtf8(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject())
        return read;

    const QJsonObject account = document.object();
    const QString owner = account.value(QStringLiteral("program_owner")).toString();
    const QString balance = account.value(QStringLiteral("balance")).toString();
    const QString nonce = account.value(QStringLiteral("nonce")).toString();
    const QString data = account.value(QStringLiteral("data")).toString();
    if (!isHex(owner, 64)
        || !isHex(balance, 32)
        || !isHex(nonce, 32)
        || data.size() % 2 != 0
        || !isHex(data, data.size())) {
        return read;
    }

    read.status = QStringLiteral("ok");
    read.programOwner = owner;
    read.balanceHex = balance;
    read.nonceHex = nonce;
    read.dataHex = data;
    return read;
}

void applyPublicRead(WalletAccount& account, const WalletAccountRead& read)
{
    account.readStatus = read.status;
    account.programOwner = read.programOwner;
    account.dataHex = read.dataHex;
}

bool encodeTransaction(const WalletTransaction& transaction,
                       QVariantList* signingRequirements,
                       QByteArray* instruction)
{
    if (!isHex(transaction.programId, 64)
        || transaction.accountIds.size() != transaction.signingRequirements.size()) {
        return false;
    }
    for (const QString& accountId : transaction.accountIds) {
        if (!isHex(accountId, 64))
            return false;
    }

    signingRequirements->reserve(transaction.signingRequirements.size());
    for (bool required : transaction.signingRequirements)
        signingRequirements->append(required);

    instruction->reserve(
        static_cast<int>(transaction.instruction.size() * sizeof(quint32)));
    for (const quint32 word : transaction.instruction) {
        instruction->append(static_cast<char>(word & 0xff));
        instruction->append(static_cast<char>((word >> 8) & 0xff));
        instruction->append(static_cast<char>((word >> 16) & 0xff));
        instruction->append(static_cast<char>((word >> 24) & 0xff));
    }
    return true;
}

WalletSubmission parseSubmission(const QString& response)
{
    WalletSubmission submission;
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(response.toUtf8(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        submission.failure = WalletFailure::SubmissionFailed;
        return submission;
    }

    const QJsonObject result = document.object();
    const QJsonValue success = result.value(QStringLiteral("success"));
    const QJsonValue error = result.value(QStringLiteral("error"));
    const QString hash = result.value(QStringLiteral("tx_hash")).toString();
    const bool emptyError = error.isUndefined()
        || error.isNull()
        || (error.isString() && error.toString().isEmpty());
    if (!success.isBool()
        || !success.toBool()
        || !emptyError
        || !isHex(hash, 64, false)) {
        submission.failure = WalletFailure::SubmissionFailed;
        return submission;
    }

    submission.nativeHash = hash.toLower();
    return submission;
}
}

struct LogosWalletProvider::Impl {
    explicit Impl(LogosAPI* api)
        : ownedLogos(std::make_unique<LogosModules>(api)), logos(ownedLogos.get())
    {
    }

    explicit Impl(LogosModules* value)
        : logos(value)
    {
    }

    std::unique_ptr<LogosModules> ownedLogos;
    LogosModules* logos = nullptr;
};

LogosWalletProvider::LogosWalletProvider(LogosAPI* api)
    : m_impl(std::make_unique<Impl>(api))
{
}

LogosWalletProvider::LogosWalletProvider(LogosModules* logos)
    : m_impl(std::make_unique<Impl>(logos))
{
}

LogosWalletProvider::~LogosWalletProvider()
{
    ++m_generation;
    if (m_connected)
        save();
}

WalletSession LogosWalletProvider::connect(const WalletPaths& paths)
{
    ++m_generation;
    clearSnapshot();
    if (!m_impl->logos)
        return failedSession(WalletFailure::WalletUnavailable);

    WalletSession session;
    if (sharedWalletIsOpen()) {
        session.adopted = true;
    } else {
        if (!QFileInfo::exists(paths.storage))
            return failedSession(WalletFailure::WalletMissing);
        if (m_impl->logos->lez_core.open(paths.config, paths.storage, paths.statistics)
            != WALLET_FFI_SUCCESS) {
            return failedSession(WalletFailure::OpenFailed);
        }
    }

    m_connected = true;
    session.snapshot = snapshot(true);
    session.failure = session.snapshot.failure;
    return session;
}

void LogosWalletProvider::connectAsync(const WalletPaths& paths, SessionCallback callback)
{
    clearSnapshot();
    const quint64 generation = ++m_generation;
    if (!m_impl->logos) {
        QTimer::singleShot(0, [callback = std::move(callback)]() mutable {
            callback(failedSession(WalletFailure::WalletUnavailable));
        });
        return;
    }

    auto finishOpen = [this, generation, callback = std::move(callback)](
                          bool adopted, WalletFailure failure) mutable {
        if (generation != m_generation)
            return;
        if (failure != WalletFailure::None) {
            callback(failedSession(failure));
            return;
        }
        m_connected = true;
        loadSnapshotAsync(generation,
            [this, generation, adopted, callback = std::move(callback)](
                WalletSnapshot snapshot) mutable {
                if (generation != m_generation)
                    return;
                WalletSession session;
                session.adopted = adopted;
                session.failure = snapshot.failure;
                session.snapshot = std::move(snapshot);
                callback(std::move(session));
            });
    };

    auto openStored = [this, generation, paths, finishOpen]() mutable {
        if (generation != m_generation)
            return;
        if (!QFileInfo::exists(paths.storage)) {
            finishOpen(false, WalletFailure::WalletMissing);
            return;
        }
        m_impl->logos->lez_core.openAsync(
            paths.config, paths.storage, paths.statistics,
            [this, generation, finishOpen](int result) mutable {
                if (generation != m_generation)
                    return;
                finishOpen(false, result == WALLET_FFI_SUCCESS
                    ? WalletFailure::None : WalletFailure::OpenFailed);
            });
    };

    m_impl->logos->lez_core.get_sequencer_addrAsync(
        [this, generation, finishOpen, openStored](QString address) mutable {
            if (generation != m_generation)
                return;
            if (!address.isEmpty()) {
                finishOpen(true, WalletFailure::None);
                return;
            }
            openStored();
        });
}

WalletCreation LogosWalletProvider::createWallet(const WalletPaths& paths,
                                                  const QString& password)
{
    ++m_generation;
    clearSnapshot();
    if (!m_impl->logos)
        return failedCreation(WalletFailure::WalletUnavailable);

    const QFileInfo configInfo(paths.config);
    const QFileInfo storageInfo(paths.storage);
    if (!QDir().mkpath(configInfo.absolutePath())
        || !QDir().mkpath(storageInfo.absolutePath())) {
        return failedCreation(WalletFailure::CreateFailed);
    }

    WalletCreation creation;
    creation.mnemonic = m_impl->logos->lez_core.create_new(
        paths.config, paths.storage, paths.statistics, password);
    if (creation.mnemonic.isEmpty())
        return failedCreation(WalletFailure::CreateFailed);

    m_connected = true;
    if (!save()) {
        creation.failure = WalletFailure::SaveFailed;
        creation.snapshot.failure = creation.failure;
        return creation;
    }

    return creation;
}

WalletSnapshot LogosWalletProvider::snapshot(bool forceRefresh)
{
    if (m_snapshotReady && !forceRefresh)
        return m_snapshot;
    if (!m_connected) {
        WalletSnapshot result;
        result.failure = WalletFailure::WalletUnavailable;
        return result;
    }

    WalletSnapshot result = loadSnapshot();
    if (result.ok()) {
        m_snapshot = result;
        m_snapshotReady = true;
    }
    return result;
}

void LogosWalletProvider::snapshotAsync(bool forceRefresh, SnapshotCallback callback)
{
    if (m_snapshotReady && !forceRefresh) {
        const WalletSnapshot snapshot = m_snapshot;
        QTimer::singleShot(0, [callback = std::move(callback), snapshot]() mutable {
            callback(snapshot);
        });
        return;
    }
    if (!m_connected) {
        WalletSnapshot snapshot;
        snapshot.failure = WalletFailure::WalletUnavailable;
        QTimer::singleShot(0, [callback = std::move(callback), snapshot]() mutable {
            callback(snapshot);
        });
        return;
    }
    loadSnapshotAsync(++m_generation, std::move(callback));
}

void LogosWalletProvider::clearSnapshot()
{
    m_snapshot = {};
    m_snapshotReady = false;
}

WalletAccountCreation LogosWalletProvider::createAccount(bool isPublic)
{
    WalletAccountCreation creation;
    if (!m_connected || !m_impl->logos) {
        creation.failure = WalletFailure::WalletUnavailable;
        return creation;
    }

    creation.accountId = isPublic
        ? m_impl->logos->lez_core.create_account_public()
        : m_impl->logos->lez_core.create_account_private();
    if (!isHex(creation.accountId, 64)) {
        creation.failure = WalletFailure::CreateFailed;
        return creation;
    }
    if (!save()) {
        creation.failure = WalletFailure::SaveFailed;
        return creation;
    }

    if (isPublic)
        creation.publicAccount = readPublicAccount(creation.accountId);
    if (m_snapshotReady) {
        WalletAccount account;
        account.address = creation.accountId;
        account.displayAddress =
            m_impl->logos->lez_core.account_id_to_base58(creation.accountId);
        account.isPublic = isPublic;
        if (isPublic && creation.publicAccount.ok()) {
            account.balance = littleEndianU128ToDecimal(creation.publicAccount.balanceHex);
            auto read = std::find_if(
                m_snapshot.publicAccountReads.begin(),
                m_snapshot.publicAccountReads.end(),
                [&creation](const WalletAccountRead& existing) {
                    return existing.accountId == creation.accountId;
                });
            if (read == m_snapshot.publicAccountReads.end())
                m_snapshot.publicAccountReads.append(creation.publicAccount);
            else
                *read = creation.publicAccount;
        } else {
            account.balance = m_impl->logos->lez_core.get_balance(
                creation.accountId, isPublic);
        }
        auto existing = std::find_if(
            m_snapshot.accounts.begin(), m_snapshot.accounts.end(),
            [&creation](const WalletAccount& candidate) {
                return candidate.address == creation.accountId;
            });
        if (existing == m_snapshot.accounts.end())
            m_snapshot.accounts.append(account);
        else
            *existing = account;
        creation.snapshot = m_snapshot;
    }
    return creation;
}

WalletAccountRead LogosWalletProvider::readPublicAccount(const QString& accountId) const
{
    if (!m_impl->logos || !isHex(accountId, 64))
        return WalletAccountRead { accountId };
    return parsePublicAccount(
        accountId,
        m_impl->logos->lez_core.get_account_public(accountId));
}

WalletSubmission LogosWalletProvider::submitPublicTransaction(
    const WalletTransaction& transaction)
{
    WalletSubmission submission;
    if (!m_connected || !m_impl->logos) {
        submission.failure = WalletFailure::WalletUnavailable;
        return submission;
    }
    QVariantList signingRequirements;
    QByteArray instruction;
    if (!encodeTransaction(transaction, &signingRequirements, &instruction)) {
        submission.failure = WalletFailure::InvalidRequest;
        return submission;
    }

    const QString response =
        m_impl->logos->lez_core.send_generic_public_transaction(
            transaction.accountIds,
            signingRequirements,
            QVariant::fromValue(instruction),
            transaction.programId);
    return parseSubmission(response);
}

void LogosWalletProvider::disconnect()
{
    ++m_generation;
    if (m_connected)
        save();
    clearSnapshot();
    m_connected = false;
}

bool LogosWalletProvider::sharedWalletIsOpen() const
{
    if (!m_impl->logos)
        return false;
    // A live wallet always has a configured, non-empty sequencer URL.
    return !m_impl->logos->lez_core.get_sequencer_addr().isEmpty();
}

WalletSnapshot LogosWalletProvider::loadSnapshot()
{
    WalletSnapshot result;
    result.currentBlockHeight = static_cast<quint64>(
        qMax(0, m_impl->logos->lez_core.get_current_block_height()));
    if (result.currentBlockHeight > 0
        && m_impl->logos->lez_core.sync_to_block(result.currentBlockHeight)
            != WALLET_FFI_SUCCESS) {
        result.failure = WalletFailure::ReadFailed;
        return result;
    }
    result.lastSyncedBlock = static_cast<quint64>(
        qMax(0, m_impl->logos->lez_core.get_last_synced_block()));
    result.sequencerAddress = m_impl->logos->lez_core.get_sequencer_addr();

    const QVariantList entries = m_impl->logos->lez_core.list_accounts();
    result.accounts.reserve(entries.size());
    result.publicAccountReads.reserve(entries.size());
    for (const QVariant& value : entries) {
        const QVariantMap entry = value.toMap();
        const QString address = entry.value(QStringLiteral("account_id")).toString();
        if (entry.isEmpty() || !isHex(address, 64)) {
            result.failure = WalletFailure::ReadFailed;
            return result;
        }

        WalletAccount account;
        account.address = address;
        account.displayAddress =
            m_impl->logos->lez_core.account_id_to_base58(address);
        account.isPublic = entry.value(QStringLiteral("is_public"), true).toBool();
        if (account.isPublic) {
            const WalletAccountRead read = readPublicAccount(address);
            result.publicAccountReads.append(read);
            applyPublicRead(account, read);
            account.balance = read.ok()
                ? littleEndianU128ToDecimal(read.balanceHex)
                : m_impl->logos->lez_core.get_balance(address, true);
        } else {
            account.readStatus = QStringLiteral("private");
            account.balance = m_impl->logos->lez_core.get_balance(address, false);
        }
        result.accounts.append(account);
    }

    return result;
}

void LogosWalletProvider::loadSnapshotAsync(quint64 generation, SnapshotCallback callback)
{
    if (!m_impl->logos || generation != m_generation)
        return;

    m_impl->logos->lez_core.get_current_block_heightAsync(
        [this, generation, callback = std::move(callback)](int currentHeight) mutable {
            if (generation != m_generation)
                return;

            auto afterSync = [this, generation, currentHeight,
                              callback = std::move(callback)](int syncResult) mutable {
                if (generation != m_generation)
                    return;
                if (syncResult != WALLET_FFI_SUCCESS) {
                    WalletSnapshot failed;
                    failed.failure = WalletFailure::ReadFailed;
                    callback(std::move(failed));
                    return;
                }

                m_impl->logos->lez_core.get_last_synced_blockAsync(
                    [this, generation, currentHeight,
                     callback = std::move(callback)](int lastSynced) mutable {
                        if (generation != m_generation)
                            return;
                        m_impl->logos->lez_core.get_sequencer_addrAsync(
                            [this, generation, currentHeight, lastSynced,
                             callback = std::move(callback)](QString address) mutable {
                                if (generation != m_generation)
                                    return;
                                m_impl->logos->lez_core.list_accountsAsync(
                                    [this, generation, currentHeight, lastSynced,
                                     address = std::move(address),
                                     callback = std::move(callback)](
                                        QVariantList entries) mutable {
                                        if (generation != m_generation)
                                            return;

                                        struct SnapshotState {
                                            WalletSnapshot snapshot;
                                            QVector<WalletAccountRead> publicReads;
                                            QVector<bool> publicFlags;
                                            qsizetype remaining = 0;
                                            SnapshotCallback callback;
                                        };
                                        auto state = std::make_shared<SnapshotState>();
                                        state->snapshot.currentBlockHeight = static_cast<quint64>(
                                            qMax(0, currentHeight));
                                        state->snapshot.lastSyncedBlock = static_cast<quint64>(
                                            qMax(0, lastSynced));
                                        state->snapshot.sequencerAddress = std::move(address);
                                        state->snapshot.accounts.resize(entries.size());
                                        state->publicReads.resize(entries.size());
                                        state->publicFlags.resize(entries.size());
                                        state->remaining = entries.size();
                                        state->callback = std::move(callback);

                                        for (qsizetype index = 0; index < entries.size(); ++index) {
                                            const QVariantMap entry = entries.at(index).toMap();
                                            const QString accountId = entry
                                                .value(QStringLiteral("account_id")).toString();
                                            if (entry.isEmpty() || !isHex(accountId, 64)) {
                                                state->snapshot.failure = WalletFailure::ReadFailed;
                                                state->callback(std::move(state->snapshot));
                                                return;
                                            }
                                            state->snapshot.accounts[index] = WalletAccount {
                                                accountId,
                                                {},
                                                entry.value(QStringLiteral("is_public"), true).toBool(),
                                            };
                                            state->snapshot.accounts[index].displayAddress =
                                                m_impl->logos->lez_core
                                                    .account_id_to_base58(accountId);
                                            state->publicFlags[index] =
                                                state->snapshot.accounts.at(index).isPublic;
                                        }

                                        auto finishOne = std::make_shared<std::function<void()>>();
                                        *finishOne = [this, generation, state]() mutable {
                                            if (generation != m_generation || --state->remaining > 0)
                                                return;
                                            for (qsizetype index = 0;
                                                 index < state->publicReads.size(); ++index) {
                                                if (state->publicFlags.at(index))
                                                    state->snapshot.publicAccountReads.append(
                                                        state->publicReads.at(index));
                                            }
                                            if (state->snapshot.ok()) {
                                                m_snapshot = state->snapshot;
                                                m_snapshotReady = true;
                                            }
                                            state->callback(std::move(state->snapshot));
                                        };

                                        if (entries.isEmpty()) {
                                            state->remaining = 1;
                                            (*finishOne)();
                                            return;
                                        }

                                        for (qsizetype index = 0; index < entries.size(); ++index) {
                                            const WalletAccount account = state->snapshot.accounts.at(index);
                                            if (!account.isPublic) {
                                                m_impl->logos->lez_core.get_balanceAsync(
                                                    account.address, false,
                                                    [state, finishOne, index](QString balance) {
                                                        state->snapshot.accounts[index].balance =
                                                            std::move(balance);
                                                        (*finishOne)();
                                                    });
                                                continue;
                                            }

                                            m_impl->logos->lez_core.get_account_publicAsync(
                                                account.address,
                                                [this, state, finishOne, index,
                                                 accountId = account.address](QString payload) {
                                                    const WalletAccountRead read =
                                                        parsePublicAccount(accountId, payload);
                                                    state->publicReads[index] = read;
                                                    applyPublicRead(
                                                        state->snapshot.accounts[index], read);
                                                    if (read.ok()) {
                                                        state->snapshot.accounts[index].balance =
                                                            littleEndianU128ToDecimal(read.balanceHex);
                                                        (*finishOne)();
                                                        return;
                                                    }
                                                    m_impl->logos->lez_core.get_balanceAsync(
                                                        accountId, true,
                                                        [state, finishOne, index](QString balance) {
                                                            state->snapshot.accounts[index].balance =
                                                                std::move(balance);
                                                            (*finishOne)();
                                                        });
                                                });
                                        }
                                    });
                            });
                    });
            };

            if (currentHeight > 0) {
                m_impl->logos->lez_core.sync_to_blockAsync(
                    currentHeight, std::move(afterSync));
            } else {
                afterSync(WALLET_FFI_SUCCESS);
            }
        });
}

bool LogosWalletProvider::save() const
{
    return m_impl->logos
        && m_impl->logos->lez_core.save() == WALLET_FFI_SUCCESS;
}
