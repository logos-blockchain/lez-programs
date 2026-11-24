#include "WalletController.h"

#include <limits>
#include <utility>

#include <QDebug>
#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QPointer>
#include <QSaveFile>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "WalletAccountModel.h"

namespace {
const char SETTINGS_ORG[] = "Logos";
const char DISCONNECTED_KEY[] = "disconnected";
const char FEE_PAYER_KEY[] = "feePayer";
const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";
constexpr int SNAPSHOT_POLL_INTERVAL_MS = 10000;
constexpr int SNAPSHOT_RETRY_INITIAL_MS = 1000;
constexpr int SNAPSHOT_RETRY_MAX_MS = 10000;
// Failed background refreshes in a row before the wallet reports an error.
constexpr int REFRESH_FAILURES_BEFORE_ERROR = 3;

int blockValue(quint64 value)
{
    return value > static_cast<quint64>(std::numeric_limits<int>::max())
        ? std::numeric_limits<int>::max() : static_cast<int>(value);
}
const char WALLET_SETTINGS_GROUP[] = "wallets";
const char ALIASES_KEY[] = "aliases";
const char PRIMARY_ACCOUNT_KEY[] = "primaryAccount";
constexpr qsizetype MAX_ALIAS_LENGTH = 40;

QString toLocalPath(const QString& path)
{
    if (path.startsWith(QStringLiteral("file://")) || path.contains(QLatin1Char('/')))
        return QUrl::fromUserInput(path).toLocalFile();
    return path;
}

QString configuredSequencer(const QString& path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
    if (!document.isObject())
        return {};
    const QJsonObject config = document.object();
    const QJsonArray sequencers = config.value(QStringLiteral("sequencers")).toArray();
    if (!sequencers.isEmpty()) {
        return sequencers.first().toObject()
            .value(QStringLiteral("sequencer_addr"))
            .toString();
    }
    return config.value(QStringLiteral("sequencer_addr")).toString();
}

QString canonicalStoragePath(const QString& path)
{
    const QFileInfo info(path);
    const QString canonical = info.canonicalFilePath();
    return canonical.isEmpty()
        ? QDir::cleanPath(info.absoluteFilePath())
        : canonical;
}
}

WalletController::WalletController(WalletProvider& wallet,
                                   QString settingsApplication,
                                   QObject* parent)
    : QObject(parent),
      m_wallet(wallet),
      m_settingsApplication(std::move(settingsApplication)),
      m_accountModel(new WalletAccountModel(this)),
      m_network(new QNetworkAccessManager(this)),
      m_reachabilityTimer(new QTimer(this)),
      m_snapshotPollTimer(new QTimer(this))
{
    m_state.walletHome = defaultWalletHome();
    m_state.configPath = defaultConfigPath();
    m_state.storagePath = defaultStoragePath();
    m_state.walletExists = QFileInfo::exists(defaultStoragePath());
    m_state.sequencerAddress = configuredSequencer(defaultConfigPath());

    m_reachabilityTimer->setObjectName(QStringLiteral("walletReachabilityTimer"));
    m_reachabilityTimer->setInterval(10000);
    connect(m_reachabilityTimer, &QTimer::timeout,
            this, &WalletController::checkReachability);

    m_snapshotPollTimer->setObjectName(QStringLiteral("walletSnapshotPollTimer"));
    m_snapshotPollTimer->setSingleShot(true);
    connect(m_snapshotPollTimer, &QTimer::timeout,
            this, &WalletController::pollSnapshot);
}

WalletController::~WalletController() = default;

QString WalletController::defaultWalletHome()
{
    const QByteArray override = qgetenv(WALLET_HOME_ENV);
    if (!override.isEmpty())
        return QString::fromLocal8Bit(override);
    return QDir::homePath() + QStringLiteral("/.lee/wallet");
}

QString WalletController::defaultConfigPath() const
{
    return m_state.walletHome + QStringLiteral("/wallet_config.json");
}

QString WalletController::defaultStoragePath() const
{
    return m_state.walletHome + QStringLiteral("/storage.json");
}

void WalletController::setDefaultSequencerAddress(const QString& address)
{
    const QString normalized = address.trimmed();
    const QUrl endpoint(normalized);
    const QString scheme = endpoint.scheme().toLower();
    if (endpoint.isValid()
        && !endpoint.host().isEmpty()
        && (scheme == QStringLiteral("http") || scheme == QStringLiteral("https"))) {
        m_defaultSequencerAddress = normalized;
    } else {
        m_defaultSequencerAddress.clear();
    }
}

bool WalletController::seedDefaultWalletConfig(const QString& configPath) const
{
    if (m_defaultSequencerAddress.isEmpty() || QFileInfo::exists(configPath))
        return true;

    const QFileInfo configInfo(configPath);
    if (!QDir().mkpath(configInfo.absolutePath())) {
        qWarning() << "WalletController: failed to create wallet configuration directory";
            return false;
    }

    QSaveFile config(configPath);
    if (!config.open(QIODevice::WriteOnly)) {
        qWarning() << "WalletController: failed to open wallet configuration";
            return false;
    }

    const QJsonObject sequencer {
        { QStringLiteral("sequencer_addr"), m_defaultSequencerAddress },
    };
    const QByteArray contents = QJsonDocument(QJsonObject {
        { QStringLiteral("sequencers"), QJsonArray { sequencer } },
        { QStringLiteral("seq_poll_timeout"), QStringLiteral("12s") },
        { QStringLiteral("seq_tx_poll_max_blocks"), 5 },
        { QStringLiteral("seq_poll_max_retries"), 5 },
        { QStringLiteral("seq_block_poll_max_amount"), 100 },
    }).toJson(QJsonDocument::Compact);
    if (config.write(contents) != contents.size() || !config.commit()) {
        qWarning() << "WalletController: failed to save wallet configuration";
            return false;
    }
    return true;
}

void WalletController::start()
{
    if (m_started)
        return;
    m_started = true;
    QTimer::singleShot(0, this, &WalletController::openOnStartup);
}

void WalletController::openOnStartup()
{
    if (QSettings(SETTINGS_ORG, m_settingsApplication)
            .value(DISCONNECTED_KEY, false).toBool()) {
        return;
    }

    const QString config = defaultConfigPath();
    const QString storage = defaultStoragePath();
    beginOpen(config, storage);
}

bool WalletController::beginOpen(const QString& config, const QString& storage)
{
    if (m_state.isWalletOpen
        || m_state.syncStatus == QStringLiteral("opening")
        || m_state.syncStatus == QStringLiteral("syncing")) {
        return false;
    }

    const quint64 generation = ++m_operationGeneration;
    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    const QString endpoint = configuredSequencer(config);
    if (!endpoint.isEmpty())
        m_state.sequencerAddress = endpoint;
    m_state.initialSync = true;
    m_state.syncStatus = QStringLiteral("opening");
    m_state.syncError.clear();
    emit stateChanged();

    QTimer::singleShot(0, this, [this, alive = QPointer<WalletController>(this), generation]() {
        if (alive && generation == m_operationGeneration
            && m_state.syncStatus == QStringLiteral("opening")) {
            m_state.syncStatus = QStringLiteral("syncing");
            emit stateChanged();
        }
    });

    const QString statistics = QFileInfo(config).absolutePath()
        + QStringLiteral("/statistics.json");
    m_wallet.connectAsync({ config, storage, statistics },
        [this, alive = QPointer<WalletController>(this), generation, config, storage](WalletSession session) {
            if (!alive || generation != m_operationGeneration)
                return;
            if (session.failure == WalletFailure::WalletMissing) {
                m_state.isWalletOpen = false;
                m_state.initialSync = false;
                m_state.syncStatus = QStringLiteral("closed");
                m_state.syncError.clear();
                m_state.walletExists = false;
                emit stateChanged();
                return;
            }
            if (!session.ok()) {
                qWarning() << "WalletController: wallet connection failed"
                           << walletFailureCode(session.failure);
                m_state.isWalletOpen = false;
                m_state.initialSync = false;
                m_state.syncStatus = QStringLiteral("error");
                m_state.syncError = walletFailureCode(session.failure);
                emit stateChanged();
                return;
            }

            m_state.configPath = config;
            m_state.storagePath = storage;
            m_state.walletExists = QFileInfo::exists(storage) || session.adopted;
            m_state.isWalletOpen = true;
            applySnapshot(session.snapshot, true);
        }, [this, alive = QPointer<WalletController>(this), generation](WalletSyncProgress progress) {
            if (!alive || generation != m_operationGeneration)
                return;
            applySyncProgress(progress);
        });
    return true;
}

QString WalletController::createDefaultWallet(const QString& password)
{
    const QString config = defaultConfigPath();
    if (!seedDefaultWalletConfig(config))
        return {};
    return createWallet(config, defaultStoragePath(), password);
}

QString WalletController::createWallet(const QString& configPath,
                                       const QString& storagePath,
                                       const QString& password)
{
    const quint64 generation = ++m_operationGeneration;
    const QString config = toLocalPath(configPath);
    const QString storage = toLocalPath(storagePath);
    const QString statistics = QFileInfo(config).absolutePath()
        + QStringLiteral("/statistics.json");
    const QPointer<WalletController> alive(this);
    const WalletCreation creation = m_wallet.createWallet(
        { config, storage, statistics }, password);
    if (!alive)
        return {};
    if (creation.mnemonic.isEmpty()) {
        qWarning() << "WalletController: wallet creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }
    stopReachability();

    m_state.configPath = config;
    m_state.storagePath = storage;
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, false);
    if (!creation.ok()) {
        qWarning() << "WalletController: wallet creation failed"
                   << walletFailureCode(creation.failure);
        m_state.walletExists = QFileInfo::exists(storage);
        m_state.isWalletOpen = false;
        m_state.syncStatus = QStringLiteral("error");
        m_state.syncError = walletFailureCode(creation.failure);
        emit stateChanged();
        return creation.mnemonic;
    }

    m_state.walletExists = true;
    m_snapshotPollTimer->stop();
    m_refreshInFlight = false;
    m_refreshFailures = 0;
    m_state.isWalletOpen = true;
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    m_state.initialSync = true;
    m_state.syncStatus = QStringLiteral("syncing");
    m_state.syncError.clear();
    emit stateChanged();
    m_wallet.snapshotAsync(true,
        [this, alive = QPointer<WalletController>(this), generation](WalletSnapshot snapshot) {
            if (!alive || generation != m_operationGeneration)
                return;
            if (snapshot.ok()) {
                applySnapshot(snapshot, true);
                return;
            }
            qWarning() << "WalletController: initial wallet sync failed"
                       << walletFailureCode(snapshot.failure);
            m_state.isWalletOpen = false;
            m_state.initialSync = false;
            m_state.syncStatus = QStringLiteral("error");
            m_state.syncError = walletFailureCode(snapshot.failure);
            emit stateChanged();
        }, [this, alive = QPointer<WalletController>(this), generation](WalletSyncProgress progress) {
            if (!alive || generation != m_operationGeneration)
                return;
            applySyncProgress(progress);
        });
    return creation.mnemonic;
}

bool WalletController::open()
{
    const QString config = m_state.configPath.isEmpty()
        ? defaultConfigPath() : m_state.configPath;
    const QString storage = m_state.storagePath.isEmpty()
        ? defaultStoragePath() : m_state.storagePath;
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, false);
    return beginOpen(config, storage);
}

void WalletController::cancelSync()
{
    if (!m_state.initialSync
        || (m_state.syncStatus != QStringLiteral("opening")
        && m_state.syncStatus != QStringLiteral("syncing"))) {
        return;
    }

    ++m_operationGeneration;
    m_snapshotPollTimer->stop();
    m_refreshInFlight = false;
    m_refreshFailures = 0;
    stopReachability();
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
    ++m_state.sessionRevision;
    m_accountIdentities.clear();
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    m_state.initialSync = false;
    m_state.syncStatus = QStringLiteral("error");
    m_state.syncError = QStringLiteral("sync_cancelled");
    m_accountModel->replaceAccounts({});
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, true);
    emit stateChanged();
}

void WalletController::disconnect()
{
    ++m_operationGeneration;
    m_snapshotPollTimer->stop();
    m_refreshInFlight = false;
    m_refreshFailures = 0;
    stopReachability();
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
    ++m_state.sessionRevision;
    m_accountIdentities.clear();
    m_state.feePayerId.clear();
    m_accounts.clear();
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    m_state.initialSync = false;
    m_state.syncStatus = QStringLiteral("closed");
    m_state.syncError.clear();
    m_accountModel->replaceAccounts({});
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, true);
    emit stateChanged();
    emit snapshotChanged();
}

bool WalletController::setAccountAlias(const QString& address, const QString& alias)
{
    if (!m_accountModel->contains(address))
        return false;
    const QString normalized = alias.trimmed();
    if (normalized.size() > MAX_ALIAS_LENGTH)
        return false;
    if (normalized.isEmpty())
        m_aliases.remove(address);
    else
        m_aliases.insert(address, normalized);
    m_accountModel->setAlias(address, normalized);
    storeAliases(m_aliases);
    updatePrimaryState(m_state.primaryAccountAddress);
    emit stateChanged();
    return true;
}

bool WalletController::setPrimaryAccount(const QString& address)
{
    if (!m_accountModel->canBePrimary(address))
        return false;
    m_accountModel->setPrimaryAddress(address);
    storePrimaryAccount(address);
    updatePrimaryState(address);
    emit stateChanged();
    return true;
}

void WalletController::applyAccountPresentations(
    const QVector<WalletAccountPresentation>& presentations)
{
    if (!m_accountModel->applyPresentations(presentations))
        return;

    const QString previousPrimary = m_state.primaryAccountAddress;
    const QString previousPrimaryName = m_state.primaryAccountName;
    QString primary = m_state.primaryAccountAddress;
    if (!m_accountModel->canBePrimary(primary))
        primary = m_accountModel->firstAutomaticPrimary();
    m_accountModel->setPrimaryAddress(primary);
    if (primary != previousPrimary)
        storePrimaryAccount(primary);
    updatePrimaryState(primary);
    if (m_state.primaryAccountAddress != previousPrimary
        || m_state.primaryAccountName != previousPrimaryName) {
        emit stateChanged();
    }
}

QString WalletController::createAccount(bool isPublic)
{
    if (!m_state.canSubmit())
        return {};

    const WalletAccountCreation creation = m_wallet.createAccount(isPublic);
    if (!creation.ok()) {
        qWarning() << "WalletController: account creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }
    ++m_operationGeneration;
    const bool restartRefresh = m_refreshInFlight;
    m_refreshInFlight = false;
    if (creation.snapshot.ok()) {
        // Creation updates the account cache without completing a network sync.
        applySnapshotData(creation.snapshot);
        emit snapshotChanged();
        emit stateChanged();
        checkReachability();
    } else {
        qWarning() << "WalletController: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
        m_state.syncStatus = QStringLiteral("error");
        m_state.syncError = walletFailureCode(creation.snapshot.failure);
        emit stateChanged();
    }
    if (restartRefresh)
        refreshSnapshot();
    return creation.accountId;
}

// A user refresh from the error state reports "syncing" so its outcome always
// changes syncStatus; otherwise it runs like a background poll.
void WalletController::refresh()
{
    if (!m_state.isWalletOpen || m_state.initialSync)
        return;
    if (m_state.syncStatus == QStringLiteral("error")) {
        m_state.syncStatus = QStringLiteral("syncing");
        m_state.syncError.clear();
        emit stateChanged();
    }
    refreshSnapshot();
}

// Background refreshes keep syncStatus at "ready", so the UI does not treat
// every poll as a reconnect.
void WalletController::refreshSnapshot()
{
    if (!m_state.isWalletOpen || m_state.initialSync || m_refreshInFlight)
        return;

    m_snapshotPollTimer->stop();
    m_refreshInFlight = true;
    const quint64 generation = ++m_operationGeneration;
    m_wallet.snapshotAsync(true, [this, alive = QPointer<WalletController>(this), generation](WalletSnapshot next) {
        if (!alive || generation != m_operationGeneration)
            return;
        m_refreshInFlight = false;
        if (next.ok()) {
            applySnapshot(next);
            return;
        }
        qWarning() << "WalletController: wallet refresh failed"
                   << walletFailureCode(next.failure);
        const bool failed = ++m_refreshFailures >= REFRESH_FAILURES_BEFORE_ERROR;
        if ((failed && m_state.syncStatus == QStringLiteral("ready"))
            || m_state.syncStatus == QStringLiteral("syncing")) {
            m_state.syncStatus = QStringLiteral("error");
            m_state.syncError = walletFailureCode(next.failure);
            emit stateChanged();
        }
        scheduleSnapshotPoll(true);
    });
}

QString WalletController::balance(const QString& accountId, bool isPublic)
{
    const WalletSnapshot current = m_wallet.snapshot();
    for (const WalletAccount& account : current.accounts) {
        if (account.address == accountId && account.isPublic == isPublic)
            return account.balance;
    }
    return {};
}

bool WalletController::setFeePayer(const QString& accountId)
{
    const QString id = accountId.trimmed().toLower();
    for (const WalletAccount& account : m_accounts) {
        if (account.address.toLower() != id || !canPayFees(account))
            continue;
        QSettings(SETTINGS_ORG, m_settingsApplication).setValue(FEE_PAYER_KEY, id);
        if (m_state.feePayerId != id) {
            m_state.feePayerId = id;
            emit stateChanged();
        }
        return true;
    }
    return false;
}

// Keeps the persisted choice while it covers the minimum fee reserve; otherwise
// defaults to the best-funded public account, without persisting that default.
void WalletController::selectFeePayer(const QVector<WalletAccount>& accounts)
{
    const QString saved = QSettings(SETTINGS_ORG, m_settingsApplication)
        .value(FEE_PAYER_KEY).toString();
    const WalletAccount* best = nullptr;
    for (const WalletAccount& account : accounts) {
        if (!canPayFees(account))
            continue;
        if (account.address.toLower() == saved) {
            m_state.feePayerId = saved;
            return;
        }
        if (!best || balanceLess(best->balance, account.balance))
            best = &account;
    }
    m_state.feePayerId = best ? best->address.toLower() : QString();
}

void WalletController::applySnapshotData(const WalletSnapshot& snapshot, bool newSession)
{
    // Balance-only refreshes keep the session; a new one or changed accounts end it.
    QStringList identities;
    identities.reserve(snapshot.accounts.size());
    for (const WalletAccount& account : snapshot.accounts) {
        identities.append(account.address + (account.isPublic
            ? QStringLiteral(":public") : QStringLiteral(":private")));
    }
    identities.sort();
    if (newSession || identities != m_accountIdentities)
        ++m_state.sessionRevision;
    m_accountIdentities = std::move(identities);

    m_accounts = snapshot.accounts;
    selectFeePayer(m_accounts);
    m_snapshot = snapshot;
    m_aliases = loadAliases();
    QString primary = loadPrimaryAccount();
    m_accountModel->replaceAccounts(snapshot.accounts, m_aliases, primary);
    if (!m_accountModel->canBePrimary(primary))
        primary = m_accountModel->firstAutomaticPrimary();
    m_accountModel->setPrimaryAddress(primary);
    storePrimaryAccount(primary);
    updatePrimaryState(primary);
    m_state.lastSyncedBlock = blockValue(snapshot.lastSyncedBlock);
    m_state.currentBlockHeight = blockValue(snapshot.currentBlockHeight);
    m_state.sequencerAddress = snapshot.sequencerAddress;
}

void WalletController::applySnapshot(const WalletSnapshot& snapshot, bool newSession)
{
    applySnapshotData(snapshot, newSession);
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    m_state.initialSync = false;
    m_state.syncStatus = QStringLiteral("ready");
    m_state.syncError.clear();
    m_snapshotRetryDelayMs = SNAPSHOT_RETRY_INITIAL_MS;
    m_refreshFailures = 0;
    emit snapshotChanged();
    emit stateChanged();
    if (!m_reachabilityTimer->isActive())
        m_reachabilityTimer->start();
    checkReachability();
    scheduleSnapshotPoll(false);
}

void WalletController::applySyncProgress(const WalletSyncProgress& progress)
{
    const bool changed = m_state.syncProgressKnown != progress.known
        || m_state.syncCurrentBlock != blockValue(progress.currentBlock)
        || m_state.syncTargetBlock != blockValue(progress.targetBlock)
        || m_state.syncRemainingBlocks != blockValue(progress.remainingBlocks);
    if (!changed)
        return;

    m_state.syncProgressKnown = progress.known;
    m_state.syncCurrentBlock = blockValue(progress.currentBlock);
    m_state.syncTargetBlock = blockValue(progress.targetBlock);
    m_state.syncRemainingBlocks = blockValue(progress.remainingBlocks);
    if (m_state.syncStatus == QStringLiteral("opening"))
        m_state.syncStatus = QStringLiteral("syncing");
    emit stateChanged();
}

// Only a new block can change balances or accounts, so a poll reads the chain
// height first and skips the full sync (and its wallet save) when nothing is new.
void WalletController::pollSnapshot()
{
    if (!m_state.isWalletOpen || m_refreshInFlight)
        return;

    const quint64 generation = m_operationGeneration;
    m_wallet.currentBlockHeightAsync([this, alive = QPointer<WalletController>(this), generation](qint64 height) {
        if (!alive || generation != m_operationGeneration || !m_state.isWalletOpen)
            return;
        if (height > 0 && height <= m_state.lastSyncedBlock
            && m_state.syncStatus == QStringLiteral("ready")) {
            scheduleSnapshotPoll(false);
            return;
        }
        refreshSnapshot();
    });
}

void WalletController::scheduleSnapshotPoll(bool retry)
{
    if (!m_state.isWalletOpen)
        return;

    const int delay = retry ? m_snapshotRetryDelayMs : SNAPSHOT_POLL_INTERVAL_MS;
    m_snapshotPollTimer->start(delay);
    if (retry)
        m_snapshotRetryDelayMs = qMin(m_snapshotRetryDelayMs * 2,
                                      SNAPSHOT_RETRY_MAX_MS);
}

QString WalletController::walletSettingsGroup() const
{
    const QByteArray hash = QCryptographicHash::hash(
        canonicalStoragePath(m_state.storagePath).toUtf8(),
        QCryptographicHash::Sha256).toHex();
    return QStringLiteral("%1/%2")
        .arg(QString::fromLatin1(WALLET_SETTINGS_GROUP), QString::fromLatin1(hash));
}

QHash<QString, QString> WalletController::loadAliases() const
{
    QSettings settings(SETTINGS_ORG, m_settingsApplication);
    settings.beginGroup(walletSettingsGroup());
    const QVariantMap stored = settings.value(ALIASES_KEY).toMap();
    QHash<QString, QString> aliases;
    for (auto iterator = stored.cbegin(); iterator != stored.cend(); ++iterator) {
        const QString alias = iterator.value().toString().trimmed();
        if (!alias.isEmpty() && alias.size() <= MAX_ALIAS_LENGTH)
            aliases.insert(iterator.key(), alias);
    }
    return aliases;
}

QString WalletController::loadPrimaryAccount() const
{
    QSettings settings(SETTINGS_ORG, m_settingsApplication);
    settings.beginGroup(walletSettingsGroup());
    return settings.value(PRIMARY_ACCOUNT_KEY).toString();
}

void WalletController::storeAliases(const QHash<QString, QString>& aliases) const
{
    QVariantMap stored;
    for (auto iterator = aliases.cbegin(); iterator != aliases.cend(); ++iterator)
        stored.insert(iterator.key(), iterator.value());
    QSettings settings(SETTINGS_ORG, m_settingsApplication);
    settings.beginGroup(walletSettingsGroup());
    settings.setValue(ALIASES_KEY, stored);
}

void WalletController::storePrimaryAccount(const QString& address) const
{
    QSettings settings(SETTINGS_ORG, m_settingsApplication);
    settings.beginGroup(walletSettingsGroup());
    if (address.isEmpty())
        settings.remove(PRIMARY_ACCOUNT_KEY);
    else
        settings.setValue(PRIMARY_ACCOUNT_KEY, address);
}

void WalletController::updatePrimaryState(const QString& address)
{
    m_state.primaryAccountAddress = address;
    m_state.primaryAccountName.clear();
    const int row = m_accountModel->indexOf(address);
    if (row >= 0) {
        m_state.primaryAccountName = m_accountModel->data(
            m_accountModel->index(row), WalletAccountModel::NameRole).toString();
    }
}

void WalletController::stopReachability()
{
    m_reachabilityTimer->stop();
    ++m_reachabilityGeneration;
    if (m_reachabilityReply) {
        QNetworkReply* reply = m_reachabilityReply;
        m_reachabilityReply = nullptr;
        m_reachabilityEndpoint.clear();
        reply->abort();
    }
}

void WalletController::checkReachability()
{
    if (!m_state.isWalletOpen || m_state.sequencerAddress.isEmpty())
        return;

    const QString endpoint = m_state.sequencerAddress;
    if (m_reachabilityReply && endpoint == m_reachabilityEndpoint)
        return;

    const quint64 generation = ++m_reachabilityGeneration;
    if (m_reachabilityReply)
        m_reachabilityReply->abort();
    QNetworkRequest request{QUrl(endpoint)};
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_network->get(request);
    m_reachabilityReply = reply;
    m_reachabilityEndpoint = endpoint;
    connect(reply, &QNetworkReply::finished, this,
            [this, reply, generation, endpoint]() {
        if (m_reachabilityReply == reply) {
            m_reachabilityReply = nullptr;
            m_reachabilityEndpoint.clear();
        }
        if (!m_state.isWalletOpen
            || generation != m_reachabilityGeneration
            || endpoint != m_state.sequencerAddress) {
            reply->deleteLater();
            return;
        }
        const bool receivedHttp =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        const bool reachable = receivedHttp || reply->error() == QNetworkReply::NoError;
        if (m_state.sequencerReachable != reachable) {
            m_state.sequencerReachable = reachable;
            emit stateChanged();
        }
        reply->deleteLater();
    });
}
