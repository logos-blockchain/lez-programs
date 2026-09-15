#include "LogosWalletProvider.h"

#include <utility>

#include <QByteArray>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QThread>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>

#include "logos_sdk.h"

namespace {
constexpr int WALLET_FFI_SUCCESS = 0;
constexpr int CAPABILITY_WARMUP_RETRY_MS = 50;
constexpr int CAPABILITY_WARMUP_MAX_ATTEMPTS = 100;
constexpr quint64 SNAPSHOT_SYNC_CHUNK_BLOCKS = 512;

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

void reportSyncProgress(const WalletProvider::ProgressCallback& progress,
                        quint64 currentBlock,
                        quint64 targetBlock)
{
    if (!progress)
        return;

    WalletSyncProgress update;
    update.known = targetBlock > 0;
    update.currentBlock = qMin(currentBlock, targetBlock);
    update.targetBlock = targetBlock;
    update.remainingBlocks = targetBlock > update.currentBlock
        ? targetBlock - update.currentBlock : 0;
    progress(update);
}

bool waitForCapability(LogosModules* logos)
{
    for (int attempt = 1; attempt <= CAPABILITY_WARMUP_MAX_ATTEMPTS; ++attempt) {
        if (!logos->lez_core.version().isEmpty())
            return true;
        if (attempt < CAPABILITY_WARMUP_MAX_ATTEMPTS)
            QThread::msleep(CAPABILITY_WARMUP_RETRY_MS);
    }
    return false;
}

WalletAccountRead parsePublicAccount(const QString& accountId,
                                     const QString& payload)
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
    if (m_connected)
        save();
}

WalletSession LogosWalletProvider::connect(const WalletPaths& paths)
{
    clearSnapshot();
    if (!m_impl->logos)
        return failedSession(WalletFailure::WalletUnavailable);
    if (!waitForCapability(m_impl->logos))
        return failedSession(WalletFailure::CapabilityUnavailable);

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

void LogosWalletProvider::connectAsync(const WalletPaths& paths,
                                       SessionCallback callback,
                                       ProgressCallback progress)
{
    clearSnapshot();
    ++m_generation;
    const quint64 generation = m_generation;
    const auto sharedCallback = std::make_shared<SessionCallback>(std::move(callback));
    if (!m_impl->logos) {
        QTimer::singleShot(0, [sharedCallback]() {
            (*sharedCallback)(failedSession(WalletFailure::WalletUnavailable));
        });
        return;
    }

    const auto sharedProgress = std::make_shared<ProgressCallback>(std::move(progress));
    retryCapabilityAsync(paths, 1, generation, sharedCallback, sharedProgress);
}

void LogosWalletProvider::retryCapabilityAsync(
    const WalletPaths& paths,
    int attempt,
    quint64 generation,
    const std::shared_ptr<SessionCallback>& callback,
    const std::shared_ptr<ProgressCallback>& progress)
{
    if (generation != m_generation)
        return;

    m_impl->logos->lez_core.versionAsync(
        [this, paths, attempt, generation, callback, progress](QString version) {
            if (generation != m_generation)
                return;
            if (version.isEmpty()) {
                if (attempt >= CAPABILITY_WARMUP_MAX_ATTEMPTS) {
                    (*callback)(failedSession(WalletFailure::CapabilityUnavailable));
                    return;
                }
                QTimer::singleShot(CAPABILITY_WARMUP_RETRY_MS,
                                   [this, paths, attempt, generation, callback, progress]() {
                    retryCapabilityAsync(paths, attempt + 1, generation, callback, progress);
                });
                return;
            }

            openAfterCapabilityAsync(paths, generation, callback, progress);
        });
}

void LogosWalletProvider::openAfterCapabilityAsync(
    const WalletPaths& paths,
    quint64 generation,
    const std::shared_ptr<SessionCallback>& callback,
    const std::shared_ptr<ProgressCallback>& progress)
{
    if (generation != m_generation)
        return;

    const auto finishOpen = std::make_shared<std::function<void(bool, WalletFailure)>>();
    *finishOpen = [this, generation, callback, progress](bool adopted,
                                                         WalletFailure failure) {
        if (generation != m_generation)
            return;
        if (failure != WalletFailure::None) {
            (*callback)(failedSession(failure));
            return;
        }

        m_connected = true;
        loadSnapshotAsync(generation,
            [this, generation, adopted, callback](WalletSnapshot snapshot) {
                if (generation != m_generation)
                    return;
                WalletSession session;
                session.adopted = adopted;
                session.failure = snapshot.failure;
                session.snapshot = std::move(snapshot);
                (*callback)(std::move(session));
            }, *progress);
    };

    const auto openStored = [this, generation, paths, finishOpen]() {
        if (generation != m_generation)
            return;
        if (!QFileInfo::exists(paths.storage)) {
            (*finishOpen)(false, WalletFailure::WalletMissing);
            return;
        }
        m_impl->logos->lez_core.openAsync(
            paths.config, paths.storage, paths.statistics,
            [this, generation, finishOpen](int result) {
                if (generation != m_generation)
                    return;
                (*finishOpen)(false, result == WALLET_FFI_SUCCESS
                    ? WalletFailure::None : WalletFailure::OpenFailed);
            });
    };

    m_impl->logos->lez_core.get_sequencer_addrAsync(
        [this, generation, finishOpen, openStored](QString address) {
            if (generation != m_generation)
                return;
            if (!address.isEmpty()) {
                (*finishOpen)(true, WalletFailure::None);
                return;
            }
            m_impl->logos->lez_core.list_accountsAsync(
                [this, generation, finishOpen, openStored](QVariantList accounts) {
                    if (generation != m_generation)
                        return;
                    if (!accounts.isEmpty())
                        (*finishOpen)(true, WalletFailure::None);
                    else
                        openStored();
                });
        });
}

WalletCreation LogosWalletProvider::createWallet(const WalletPaths& paths,
                                                  const QString& password)
{
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

void LogosWalletProvider::snapshotAsync(bool forceRefresh,
                                        SnapshotCallback callback,
                                        ProgressCallback progress)
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
    loadSnapshotAsync(++m_generation, std::move(callback), std::move(progress));
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

    clearSnapshot();
    creation.snapshot = snapshot(true);
    return creation;
}

WalletAccountRead LogosWalletProvider::readPublicAccount(const QString& accountId) const
{
    if (!m_impl->logos)
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
    if (!isHex(transaction.programId, 64)
        || transaction.accountIds.size() != transaction.signingRequirements.size()) {
        submission.failure = WalletFailure::InvalidRequest;
        return submission;
    }
    for (const QString& accountId : transaction.accountIds) {
        if (!isHex(accountId, 64)) {
            submission.failure = WalletFailure::InvalidRequest;
            return submission;
        }
    }

    QVariantList signingRequirements;
    signingRequirements.reserve(transaction.signingRequirements.size());
    for (bool required : transaction.signingRequirements)
        signingRequirements.append(required);

    // `send_generic_public_transaction`'s `instruction` param is a byte string
    // (bstr). Passing a QVariantList<u32> makes the module's QtRO glue mangle it,
    // so the guest reads a garbage Instruction variant. Send the little-endian
    // bytes of the u32 words instead — same encoding the AMM swap path uses.
    // See docs/amm-swap-qtro-serialization-bug.md.
    QByteArray instructionBytes;
    instructionBytes.reserve(
        static_cast<int>(transaction.instruction.size() * sizeof(quint32)));
    for (const quint32 word : transaction.instruction) {
        instructionBytes.append(static_cast<char>(word & 0xff));
        instructionBytes.append(static_cast<char>((word >> 8) & 0xff));
        instructionBytes.append(static_cast<char>((word >> 16) & 0xff));
        instructionBytes.append(static_cast<char>((word >> 24) & 0xff));
    }

    const QString response =
        m_impl->logos->lez_core.send_generic_public_transaction(
            transaction.accountIds,
            signingRequirements,
            QVariant::fromValue(instructionBytes),
            transaction.programId);

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
    if (!m_impl->logos->lez_core.get_sequencer_addr().isEmpty())
        return true;
    return !m_impl->logos->lez_core.list_accounts().isEmpty();
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
        account.isPublic = entry.value(QStringLiteral("is_public"), true).toBool();
        if (account.isPublic) {
            const WalletAccountRead read = readPublicAccount(address);
            result.publicAccountReads.append(read);
            account.balance = read.ok()
                ? littleEndianU128ToDecimal(read.balanceHex)
                : m_impl->logos->lez_core.get_balance(address, true);
        } else {
            account.balance = m_impl->logos->lez_core.get_balance(address, false);
        }
        result.accounts.append(account);
    }

    if (!save())
        result.failure = WalletFailure::SaveFailed;
    return result;
}

void LogosWalletProvider::loadSnapshotAsync(quint64 generation,
                                            SnapshotCallback callback,
                                            ProgressCallback progress)
{
    if (!m_impl->logos || generation != m_generation)
        return;

    const auto completed = std::make_shared<SnapshotCallback>(std::move(callback));
    const auto progressCallback = std::make_shared<ProgressCallback>(std::move(progress));
    const auto failed = std::make_shared<std::function<void(WalletFailure)>>();
    *failed = [this, generation, completed](WalletFailure failure) {
        if (generation != m_generation)
            return;
        WalletSnapshot snapshot;
        snapshot.failure = failure;
        (*completed)(std::move(snapshot));
    };

    const auto loadAccounts = std::make_shared<std::function<void(int)>>();
    *loadAccounts = [this, generation, completed, failed](int currentHeight) {
        if (generation != m_generation)
            return;

        m_impl->logos->lez_core.get_last_synced_blockAsync(
            [this, generation, currentHeight, completed, failed](int lastSynced) {
                if (generation != m_generation)
                    return;
                m_impl->logos->lez_core.get_sequencer_addrAsync(
                    [this, generation, currentHeight, lastSynced, completed, failed](
                        QString address) {
                        if (generation != m_generation)
                            return;
                        m_impl->logos->lez_core.list_accountsAsync(
                            [this, generation, currentHeight, lastSynced,
                             address = std::move(address), completed, failed](
                                QVariantList entries) {
                                if (generation != m_generation)
                                    return;

                                struct SnapshotState {
                                    WalletSnapshot snapshot;
                                    QVector<WalletAccountRead> publicReads;
                                    QVector<bool> publicFlags;
                                    qsizetype remaining = 0;
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

                                for (qsizetype index = 0; index < entries.size(); ++index) {
                                    const QVariantMap entry = entries.at(index).toMap();
                                    const QString accountId = entry
                                        .value(QStringLiteral("account_id")).toString();
                                    if (entry.isEmpty() || !isHex(accountId, 64)) {
                                        (*failed)(WalletFailure::ReadFailed);
                                        return;
                                    }
                                    state->snapshot.accounts[index] = WalletAccount {
                                        accountId,
                                        {},
                                        entry.value(QStringLiteral("is_public"), true).toBool(),
                                    };
                                    state->publicFlags[index] =
                                        state->snapshot.accounts.at(index).isPublic;
                                }

                                auto finishOne = std::make_shared<std::function<void()>>();
                                *finishOne = [this, generation, state, finishOne, completed]() mutable {
                                    if (generation != m_generation || --state->remaining > 0)
                                        return;
                                    for (qsizetype index = 0;
                                         index < state->publicReads.size(); ++index) {
                                        if (state->publicFlags.at(index))
                                            state->snapshot.publicAccountReads.append(
                                                state->publicReads.at(index));
                                    }
                                    m_impl->logos->lez_core.saveAsync(
                                        [this, generation, state, completed](int result) mutable {
                                            if (generation != m_generation)
                                                return;
                                            if (result != WALLET_FFI_SUCCESS)
                                                state->snapshot.failure = WalletFailure::SaveFailed;
                                            if (state->snapshot.ok()) {
                                                m_snapshot = state->snapshot;
                                                m_snapshotReady = true;
                                            }
                                            (*completed)(std::move(state->snapshot));
                                        });
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

    m_impl->logos->lez_core.get_current_block_heightAsync(
        [this, generation, failed, loadAccounts, progressCallback](
            int currentHeight) {
            if (generation != m_generation)
                return;

            m_impl->logos->lez_core.get_last_synced_blockAsync(
                [this, generation, currentHeight, failed, loadAccounts, progressCallback](
                    int lastSynced) {
                    if (generation != m_generation)
                        return;

                    const quint64 target = static_cast<quint64>(qMax(0, currentHeight));
                    const quint64 starting = qMin(
                        target, static_cast<quint64>(qMax(0, lastSynced)));
                    if (target <= starting) {
                        reportSyncProgress(*progressCallback, target, target);
                        (*loadAccounts)(currentHeight);
                        return;
                    }

                    reportSyncProgress(*progressCallback, starting, target);
                    const auto syncNext = std::make_shared<std::function<void(quint64)>>();
                    *syncNext = [this, generation, currentHeight, target, failed,
                                 loadAccounts, progressCallback, syncNext](quint64 current) {
                        if (generation != m_generation)
                            return;
                        if (current >= target) {
                            (*loadAccounts)(currentHeight);
                            return;
                        }

                        const quint64 next = qMin(target, current + SNAPSHOT_SYNC_CHUNK_BLOCKS);
                        m_impl->logos->lez_core.sync_to_blockAsync(
                            static_cast<int>(next),
                            [this, generation, target, next, failed,
                             progressCallback, syncNext](int result) {
                                if (generation != m_generation)
                                    return;
                                if (result != WALLET_FFI_SUCCESS) {
                                    (*failed)(WalletFailure::ReadFailed);
                                    return;
                                }
                                reportSyncProgress(*progressCallback, next, target);
                                (*syncNext)(next);
                            });
                    };
                    (*syncNext)(starting);
                });
        });
}

bool LogosWalletProvider::save() const
{
    return m_impl->logos
        && m_impl->logos->lez_core.save() == WALLET_FFI_SUCCESS;
}
