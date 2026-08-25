#include "LogosWalletProvider.h"

#include <QByteArray>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
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

    creation.snapshot = snapshot(true);
    creation.failure = creation.snapshot.failure;
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
    WalletAccountRead read;
    read.accountId = accountId;
    if (!m_impl->logos || !isHex(accountId, 64))
        return read;

    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(
        m_impl->logos->lez_core.get_account_public(accountId).toUtf8(),
        &parseError);
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

bool LogosWalletProvider::save() const
{
    return m_impl->logos
        && m_impl->logos->lez_core.save() == WALLET_FFI_SUCCESS;
}
