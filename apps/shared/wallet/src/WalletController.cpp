#include "WalletController.h"

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
const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";
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
      m_reachabilityTimer(new QTimer(this))
{
    m_state.walletHome = defaultWalletHome();
    m_state.configPath = defaultConfigPath();
    m_state.storagePath = defaultStoragePath();
    m_state.walletExists = QFileInfo::exists(defaultStoragePath());
    m_state.sequencerAddress = configuredSequencer(defaultConfigPath());

    m_reachabilityTimer->setInterval(10000);
    connect(m_reachabilityTimer, &QTimer::timeout,
            this, &WalletController::checkReachability);
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
    if (m_state.syncStatus == QStringLiteral("opening")
        || m_state.syncStatus == QStringLiteral("syncing")) {
        return false;
    }

    const QString statistics = QFileInfo(config).absolutePath()
        + QStringLiteral("/statistics.json");
    const quint64 generation = ++m_operationGeneration;
    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.syncStatus = QStringLiteral("opening");
    m_state.syncError.clear();
    const QString endpoint = configuredSequencer(config);
    if (!endpoint.isEmpty())
        m_state.sequencerAddress = endpoint;
    emit stateChanged();

    QTimer::singleShot(0, this, [this, generation]() {
        if (generation == m_operationGeneration
            && m_state.syncStatus == QStringLiteral("opening")) {
            m_state.syncStatus = QStringLiteral("syncing");
            emit stateChanged();
        }
    });
    const QPointer<WalletController> guard(this);
    m_wallet.connectAsync({ config, storage, statistics },
        [guard, generation, config, storage](WalletSession session) {
            if (!guard || generation != guard->m_operationGeneration)
                return;
            if (session.failure == WalletFailure::WalletMissing) {
                guard->m_state.syncStatus = QStringLiteral("closed");
                guard->m_state.walletExists = false;
                emit guard->stateChanged();
                return;
            }
            if (!session.ok()) {
                qWarning() << "WalletController: wallet connection failed"
                           << walletFailureCode(session.failure);
                guard->m_state.syncStatus = QStringLiteral("error");
                guard->m_state.syncError = walletFailureCode(session.failure);
                emit guard->stateChanged();
                return;
            }

            guard->m_state.configPath = config;
            guard->m_state.storagePath = storage;
            guard->m_state.walletExists = QFileInfo::exists(storage) || session.adopted;
            guard->m_state.isWalletOpen = true;
            guard->m_state.syncStatus = QStringLiteral("ready");
            guard->applySnapshot(session.snapshot);
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
    const WalletCreation creation = m_wallet.createWallet(
        { config, storage, statistics }, password);
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
    m_state.isWalletOpen = true;
    m_state.syncStatus = QStringLiteral("syncing");
    m_state.syncError.clear();
    m_accountModel->replaceAccounts({});
    emit stateChanged();

    const QPointer<WalletController> guard(this);
    QTimer::singleShot(0, this, [guard, generation]() {
        if (!guard || generation != guard->m_operationGeneration)
            return;
        guard->m_wallet.snapshotAsync(true,
            [guard, generation](WalletSnapshot snapshot) {
                if (!guard || generation != guard->m_operationGeneration)
                    return;
                if (snapshot.ok()) {
                    guard->m_state.syncStatus = QStringLiteral("ready");
                    guard->applySnapshot(snapshot);
                    return;
                }

                qWarning() << "WalletController: initial wallet sync failed"
                           << walletFailureCode(snapshot.failure);
                guard->m_state.syncStatus = QStringLiteral("error");
                guard->m_state.syncError = walletFailureCode(snapshot.failure);
                emit guard->stateChanged();
            });
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

void WalletController::disconnect()
{
    ++m_operationGeneration;
    stopReachability();
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
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
    const WalletAccountCreation creation = m_wallet.createAccount(isPublic);
    if (!creation.ok()) {
        qWarning() << "WalletController: account creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }
    if (creation.snapshot.ok()) {
        m_state.syncStatus = QStringLiteral("ready");
        m_state.syncError.clear();
        applySnapshot(creation.snapshot);
    } else {
        qWarning() << "WalletController: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
        m_state.syncStatus = QStringLiteral("error");
        m_state.syncError = walletFailureCode(creation.snapshot.failure);
        emit stateChanged();
    }
    return creation.accountId;
}

void WalletController::refresh()
{
    if (!m_state.isWalletOpen || m_state.syncStatus == QStringLiteral("syncing"))
        return;
    const quint64 generation = ++m_operationGeneration;
    m_state.syncStatus = QStringLiteral("syncing");
    m_state.syncError.clear();
    emit stateChanged();
    const QPointer<WalletController> guard(this);
    m_wallet.snapshotAsync(true, [guard, generation](WalletSnapshot next) {
        if (!guard || generation != guard->m_operationGeneration)
            return;
        if (next.ok()) {
            guard->m_state.syncStatus = QStringLiteral("ready");
            guard->applySnapshot(next);
        } else {
            qWarning() << "WalletController: wallet refresh failed"
                       << walletFailureCode(next.failure);
            guard->m_state.syncStatus = QStringLiteral("error");
            guard->m_state.syncError = walletFailureCode(next.failure);
            emit guard->stateChanged();
        }
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
    m_snapshot = snapshot;
    m_aliases = loadAliases();
    QString primary = loadPrimaryAccount();
    m_accountModel->replaceAccounts(snapshot.accounts, m_aliases, primary);
    if (!m_accountModel->canBePrimary(primary))
        primary = m_accountModel->firstAutomaticPrimary();
    m_accountModel->setPrimaryAddress(primary);
    storePrimaryAccount(primary);
    updatePrimaryState(primary);
    m_state.lastSyncedBlock = static_cast<int>(snapshot.lastSyncedBlock);
    m_state.currentBlockHeight = static_cast<int>(snapshot.currentBlockHeight);
    if (!snapshot.sequencerAddress.isEmpty())
        m_state.sequencerAddress = snapshot.sequencerAddress;
    emit snapshotChanged();
    emit stateChanged();
    if (!m_reachabilityTimer->isActive())
        m_reachabilityTimer->start();
    checkReachability();
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
