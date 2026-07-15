#include "AmmUiBackend.h"

#include <QDebug>
#include <QDir>
#include <QFileInfo>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "LogosWalletProvider.h"
#include "logos_api.h"

namespace {
const char SETTINGS_ORG[] = "Logos";
const char SETTINGS_APP[] = "AmmUI";
const char DISCONNECTED_KEY[] = "disconnected";
const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";

QString toLocalPath(const QString& path)
{
    if (path.startsWith(QStringLiteral("file://")) || path.contains(QLatin1Char('/')))
        return QUrl::fromUserInput(path).toLocalFile();
    return path;
}
}

QString AmmUiBackend::defaultWalletHome()
{
    const QByteArray override = qgetenv(WALLET_HOME_ENV);
    if (!override.isEmpty())
        return QString::fromLocal8Bit(override);
    return QDir::homePath() + QStringLiteral("/.lee/wallet");
}

QString AmmUiBackend::defaultConfigPath() const
{
    return defaultWalletHome() + QStringLiteral("/wallet_config.json");
}

QString AmmUiBackend::defaultStoragePath() const
{
    return defaultWalletHome() + QStringLiteral("/storage.json");
}

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_accountModel(new WalletAccountModel(this)),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_net(new QNetworkAccessManager(this)),
      m_reachabilityTimer(new QTimer(this))
{
    setIsWalletOpen(false);
    setLastSyncedBlock(0);
    setCurrentBlockHeight(0);
    setWalletHome(defaultWalletHome());
    setSequencerReachable(true);
    setWalletExists(QFileInfo::exists(defaultStoragePath()));

    m_reachabilityTimer->setInterval(10000);
    connect(m_reachabilityTimer, &QTimer::timeout,
            this, &AmmUiBackend::checkReachability);
    m_reachabilityTimer->start();

    QTimer::singleShot(0, this, &AmmUiBackend::openOrAdoptWallet);
}

AmmUiBackend::~AmmUiBackend() = default;

void AmmUiBackend::openOrAdoptWallet()
{
    if (QSettings(SETTINGS_ORG, SETTINGS_APP).value(DISCONNECTED_KEY, false).toBool())
        return;

    const QString config = defaultConfigPath();
    const QString storage = defaultStoragePath();
    const WalletSession session = m_wallet->connect({ config, storage });
    if (session.failure == WalletFailure::WalletMissing)
        return;
    if (!session.ok()) {
        qWarning() << "AmmUiBackend: wallet connection failed"
                   << walletFailureCode(session.failure);
        return;
    }

    persistConfigPath(config);
    persistStoragePath(storage);
    setWalletExists(QFileInfo::exists(storage) || session.adopted);
    setIsWalletOpen(true);
    applySnapshot(session.snapshot);
}

QString AmmUiBackend::createNewDefault(QString password)
{
    return createNew(defaultConfigPath(), defaultStoragePath(), password);
}

QString AmmUiBackend::createNew(QString configPath,
                                QString storagePath,
                                QString password)
{
    const QString config = toLocalPath(configPath);
    const QString storage = toLocalPath(storagePath);
    const WalletCreation creation = m_wallet->createWallet(
        { config, storage }, password);
    if (creation.mnemonic.isEmpty()) {
        qWarning() << "AmmUiBackend: wallet creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }

    persistConfigPath(config);
    persistStoragePath(storage);
    setWalletExists(true);
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    if (!creation.ok()) {
        qWarning() << "AmmUiBackend: wallet creation failed"
                   << walletFailureCode(creation.failure);
        return creation.mnemonic;
    }

    setIsWalletOpen(true);
    applySnapshot(creation.snapshot);
    return creation.mnemonic;
}

bool AmmUiBackend::openExisting()
{
    const QString config = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString storage = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    const WalletSession session = m_wallet->connect({ config, storage });
    if (!session.ok()) {
        qWarning() << "AmmUiBackend: wallet open failed"
                   << walletFailureCode(session.failure);
        return false;
    }

    persistConfigPath(config);
    persistStoragePath(storage);
    setWalletExists(true);
    setIsWalletOpen(true);
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    applySnapshot(session.snapshot);
    return true;
}

void AmmUiBackend::disconnectWallet()
{
    m_wallet->disconnect();
    setIsWalletOpen(false);
    m_accountModel->replaceAccounts({});
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, true);
}

QString AmmUiBackend::createAccountPublic()
{
    const WalletAccountCreation creation = m_wallet->createAccount(true);
    if (!creation.ok()) {
        qWarning() << "AmmUiBackend: public account creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }
    if (creation.snapshot.ok())
        applySnapshot(creation.snapshot);
    else
        qWarning() << "AmmUiBackend: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
    return creation.accountId;
}

QString AmmUiBackend::createAccountPrivate()
{
    const WalletAccountCreation creation = m_wallet->createAccount(false);
    if (!creation.ok()) {
        qWarning() << "AmmUiBackend: private account creation failed"
                   << walletFailureCode(creation.failure);
        return {};
    }
    if (creation.snapshot.ok())
        applySnapshot(creation.snapshot);
    else
        qWarning() << "AmmUiBackend: account refresh failed"
                   << walletFailureCode(creation.snapshot.failure);
    return creation.accountId;
}

void AmmUiBackend::refreshAccounts()
{
    const WalletSnapshot next = m_wallet->snapshot(true);
    if (next.ok())
        applySnapshot(next);
    else
        qWarning() << "AmmUiBackend: wallet refresh failed"
                   << walletFailureCode(next.failure);
}

void AmmUiBackend::refreshBalances()
{
    refreshAccounts();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    const WalletSnapshot current = m_wallet->snapshot();
    for (const WalletAccount& account : current.accounts) {
        if (account.address == accountIdHex && account.isPublic == isPublic)
            return account.balance;
    }
    return {};
}

void AmmUiBackend::applySnapshot(const WalletSnapshot& snapshot)
{
    m_accountModel->replaceAccounts(snapshot.accounts);
    setLastSyncedBlock(static_cast<int>(snapshot.lastSyncedBlock));
    setCurrentBlockHeight(static_cast<int>(snapshot.currentBlockHeight));
    setSequencerAddr(snapshot.sequencerAddress);
    checkReachability();
}

void AmmUiBackend::checkReachability()
{
    if (sequencerAddr().isEmpty())
        return;

    QNetworkRequest request{QUrl(sequencerAddr())};
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_net->get(request);
    connect(reply, &QNetworkReply::finished, this, [this, reply]() {
        const bool receivedHttp =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        setSequencerReachable(receivedHttp || reply->error() == QNetworkReply::NoError);
        reply->deleteLater();
    });
}

void AmmUiBackend::persistConfigPath(const QString& path)
{
    setConfigPath(toLocalPath(path));
}

void AmmUiBackend::persistStoragePath(const QString& path)
{
    setStoragePath(toLocalPath(path));
}
