#include "WalletController.h"

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
      m_reachabilityTimer(new QTimer(this))
{
    m_state.walletHome = defaultWalletHome();
    m_state.walletExists = QFileInfo::exists(defaultStoragePath());

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
    const WalletSession session = m_wallet.connect({ config, storage });
    if (session.failure == WalletFailure::WalletMissing)
        return;
    if (!session.ok()) {
        qWarning() << "WalletController: wallet connection failed"
                   << walletFailureCode(session.failure);
        return;
    }

    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.walletExists = QFileInfo::exists(storage) || session.adopted;
    m_state.isWalletOpen = true;
    applySnapshot(session.snapshot);
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

    m_state.isWalletOpen = true;
    applySnapshot(creation.snapshot);
    return creation.mnemonic;
}

bool WalletController::open()
{
    const QString config = m_state.configPath.isEmpty()
        ? defaultConfigPath() : m_state.configPath;
    const QString storage = m_state.storagePath.isEmpty()
        ? defaultStoragePath() : m_state.storagePath;
    const WalletSession session = m_wallet.connect({ config, storage });
    if (!session.ok()) {
        qWarning() << "WalletController: wallet open failed"
                   << walletFailureCode(session.failure);
        return false;
    }

    m_state.configPath = config;
    m_state.storagePath = storage;
    m_state.walletExists = true;
    m_state.isWalletOpen = true;
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, false);
    applySnapshot(session.snapshot);
    return true;
}

void WalletController::disconnect()
{
    m_wallet.disconnect();
    m_state.isWalletOpen = false;
    m_accountModel->replaceAccounts({});
    QSettings(SETTINGS_ORG, m_settingsApplication).setValue(DISCONNECTED_KEY, true);
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
        applySnapshot(creation.snapshot);
    } else {
        qWarning() << "WalletController: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
    }
    return creation.accountId;
}

void WalletController::refresh()
{
    const WalletSnapshot next = m_wallet.snapshot(true);
    if (next.ok()) {
        applySnapshot(next);
    } else {
        qWarning() << "WalletController: wallet refresh failed"
                   << walletFailureCode(next.failure);
    }
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
    m_state.lastSyncedBlock = static_cast<int>(snapshot.lastSyncedBlock);
    m_state.currentBlockHeight = static_cast<int>(snapshot.currentBlockHeight);
    m_state.sequencerAddress = snapshot.sequencerAddress;
    emit stateChanged();
    checkReachability();
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
