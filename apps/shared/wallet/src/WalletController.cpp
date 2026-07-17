#include "WalletController.h"

#include <utility>

#include <QDebug>
#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
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
    return document.object().value(QStringLiteral("sequencer_addr")).toString();
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
    if (m_state.syncStatus == QStringLiteral("opening")
        || m_state.syncStatus == QStringLiteral("syncing")) {
        return false;
    }

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
    m_wallet.connectAsync({ config, storage },
        [this, generation, config, storage](WalletSession session) {
            if (generation != m_operationGeneration)
                return;
            if (session.failure == WalletFailure::WalletMissing) {
                m_state.syncStatus = QStringLiteral("closed");
                m_state.walletExists = false;
                emit stateChanged();
                return;
            }
            if (!session.ok()) {
                qWarning() << "WalletController: wallet connection failed"
                           << walletFailureCode(session.failure);
                m_state.syncStatus = QStringLiteral("error");
                m_state.syncError = walletFailureCode(session.failure);
                emit stateChanged();
                return;
            }

            m_state.configPath = config;
            m_state.storagePath = storage;
            m_state.walletExists = QFileInfo::exists(storage) || session.adopted;
            m_state.isWalletOpen = true;
            m_state.syncStatus = QStringLiteral("ready");
            applySnapshot(session.snapshot);
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
    const bool createdButUnreadable = creation.failure == WalletFailure::ReadFailed;
    if (creation.mnemonic.isEmpty()
        || (!creation.ok() && !createdButUnreadable)) {
        qWarning() << "WalletController: wallet creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }

    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.walletExists = true;
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, false);
    m_state.isWalletOpen = true;
    if (!creation.snapshot.ok()) {
        qWarning() << "WalletController: wallet creation refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
        m_state.syncStatus = QStringLiteral("error");
        m_state.syncError = walletFailureCode(creation.snapshot.failure);
        emit stateChanged();
        return creation.mnemonic;
    }

    m_state.syncStatus = QStringLiteral("ready");
    m_state.syncError.clear();
    applySnapshot(creation.snapshot);
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
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
    m_state.syncStatus = QStringLiteral("closed");
    m_state.syncError.clear();
    m_state.primaryAccountAddress.clear();
    m_state.primaryAccountName.clear();
    m_snapshot = {};
    m_aliases.clear();
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
    m_accountModel->applyPresentations(presentations);
    QString primary = m_state.primaryAccountAddress;
    if (!m_accountModel->canBePrimary(primary))
        primary = m_accountModel->firstAutomaticPrimary();
    m_accountModel->setPrimaryAddress(primary);
    storePrimaryAccount(primary);
    updatePrimaryState(primary);
    emit stateChanged();
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
    m_wallet.snapshotAsync(true, [this, generation](WalletSnapshot next) {
        if (generation != m_operationGeneration)
            return;
        if (next.ok()) {
            m_state.syncStatus = QStringLiteral("ready");
            applySnapshot(next);
        } else {
            qWarning() << "WalletController: wallet refresh failed"
                       << walletFailureCode(next.failure);
            m_state.syncStatus = QStringLiteral("error");
            m_state.syncError = walletFailureCode(next.failure);
            emit stateChanged();
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
