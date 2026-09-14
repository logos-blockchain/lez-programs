#include "WalletController.h"

#include <limits>
#include <utility>

#include <QDebug>
#include <QDir>
#include <QFileInfo>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "WalletAccountModel.h"

namespace {
const char SETTINGS_ORG[] = "Logos";
const char DISCONNECTED_KEY[] = "disconnected";
const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";
constexpr int SNAPSHOT_POLL_INTERVAL_MS = 10000;
constexpr int SNAPSHOT_RETRY_INITIAL_MS = 1000;
constexpr int SNAPSHOT_RETRY_MAX_MS = 10000;

int blockValue(quint64 value)
{
    return value > static_cast<quint64>(std::numeric_limits<int>::max())
        ? std::numeric_limits<int>::max() : static_cast<int>(value);
}

QString toLocalPath(const QString& path)
{
    if (path.startsWith(QStringLiteral("file://")) || path.contains(QLatin1Char('/')))
        return QUrl::fromUserInput(path).toLocalFile();
    return path;
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
    m_state.walletExists = QFileInfo::exists(defaultStoragePath());

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

void WalletController::start()
{
    if (m_started)
        return;
    m_started = true;
    m_reachabilityTimer->start();
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
    m_state.initialSync = true;
    m_state.syncStatus = QStringLiteral("opening");
    m_state.syncError.clear();
    emit stateChanged();

    QTimer::singleShot(0, this, [this, generation]() {
        if (generation == m_operationGeneration
            && m_state.syncStatus == QStringLiteral("opening")) {
            m_state.syncStatus = QStringLiteral("syncing");
            emit stateChanged();
        }
    });

    m_wallet.connectAsync({ config, storage },
            [this, generation, config, storage](WalletSession session) {
            if (generation != m_operationGeneration)
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
            applySnapshot(session.snapshot);
        }, [this, generation](WalletSyncProgress progress) {
            if (generation != m_operationGeneration)
                return;
            applySyncProgress(progress);
        });
    return true;
}

QString WalletController::createDefaultWallet(const QString& password)
{
    return createWallet(defaultConfigPath(), defaultStoragePath(), password);
}

QString WalletController::createWallet(const QString& configPath,
                                       const QString& storagePath,
                                       const QString& password)
{
    const QString config = toLocalPath(configPath);
    const QString storage = toLocalPath(storagePath);
    const WalletCreation creation = m_wallet.createWallet(
        { config, storage }, password);
    if (creation.mnemonic.isEmpty()) {
        qWarning() << "WalletController: wallet creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }

    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.walletExists = true;
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, false);
    if (!creation.ok()) {
        qWarning() << "WalletController: wallet creation failed"
                   << walletFailureCode(creation.failure);
        emit stateChanged();
        return creation.mnemonic;
    }

    const quint64 generation = ++m_operationGeneration;
    m_snapshotPollTimer->stop();
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
        [this, generation](WalletSnapshot snapshot) {
            if (generation != m_operationGeneration)
                return;
            if (snapshot.ok()) {
                applySnapshot(snapshot);
                return;
            }
            qWarning() << "WalletController: initial wallet sync failed"
                       << walletFailureCode(snapshot.failure);
            m_state.isWalletOpen = false;
            m_state.initialSync = false;
            m_state.syncStatus = QStringLiteral("error");
            m_state.syncError = walletFailureCode(snapshot.failure);
            emit stateChanged();
        }, [this, generation](WalletSyncProgress progress) {
            if (generation != m_operationGeneration)
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
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
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
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
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
    if (creation.snapshot.ok()) {
        applySnapshot(creation.snapshot);
    } else {
        qWarning() << "WalletController: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
    }
    return creation.accountId;
}

void WalletController::refresh()
{
    if (!m_state.isWalletOpen || m_state.syncStatus == QStringLiteral("syncing"))
        return;

    m_snapshotPollTimer->stop();
    const quint64 generation = ++m_operationGeneration;
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    // Refreshes after the first successful snapshot retain the last usable wallet
    // state; only initial open/create synchronization blocks submissions.
    m_state.syncStatus = QStringLiteral("syncing");
    m_state.syncError.clear();
    emit stateChanged();
    m_wallet.snapshotAsync(true, [this, generation](WalletSnapshot next) {
        if (generation != m_operationGeneration)
            return;
        if (next.ok()) {
            applySnapshot(next);
        } else {
            qWarning() << "WalletController: wallet refresh failed"
                       << walletFailureCode(next.failure);
            m_state.syncStatus = QStringLiteral("error");
            m_state.syncError = walletFailureCode(next.failure);
            emit stateChanged();
            scheduleSnapshotPoll(true);
        }
    }, [this, generation](WalletSyncProgress progress) {
        if (generation != m_operationGeneration)
            return;
        applySyncProgress(progress);
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

void WalletController::applySnapshot(const WalletSnapshot& snapshot)
{
    m_accountModel->replaceAccounts(snapshot.accounts);
    m_state.lastSyncedBlock = blockValue(snapshot.lastSyncedBlock);
    m_state.currentBlockHeight = blockValue(snapshot.currentBlockHeight);
    m_state.syncProgressKnown = false;
    m_state.syncCurrentBlock = 0;
    m_state.syncTargetBlock = 0;
    m_state.syncRemainingBlocks = 0;
    m_state.initialSync = false;
    m_state.sequencerAddress = snapshot.sequencerAddress;
    m_state.syncStatus = QStringLiteral("ready");
    m_state.syncError.clear();
    m_snapshotRetryDelayMs = SNAPSHOT_RETRY_INITIAL_MS;
    emit stateChanged();
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

void WalletController::pollSnapshot()
{
    refresh();
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

void WalletController::checkReachability()
{
    if (!m_state.isWalletOpen || m_state.sequencerAddress.isEmpty())
        return;

    QNetworkRequest request{QUrl(m_state.sequencerAddress)};
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_network->get(request);
    connect(reply, &QNetworkReply::finished, this, [this, reply]() {
        if (!m_state.isWalletOpen) {
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
