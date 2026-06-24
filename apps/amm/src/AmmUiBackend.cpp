#include "AmmUiBackend.h"

#include <QClipboard>
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "logos_api.h"
#include "logos_sdk.h"

namespace {
    const char SETTINGS_ORG[] = "Logos";
    const char SETTINGS_APP[] = "AmmUI";
    // Sticky "user pressed Disconnect" flag so the wallet stays locked across
    // relaunches until the user reconnects.
    const char DISCONNECTED_KEY[] = "disconnected";
    const int WALLET_FFI_SUCCESS = 0;

    // Wallet home env override. Mirrors LEZ's own var so the app shares the
    // canonical wallet (~/.lee/wallet) used by the wallet UI and other apps.
    const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";

    // Normalise file:// URLs and OS paths to a plain local path.
    QString toLocalPath(const QString& path) {
        if (path.startsWith("file://") || path.contains("/"))
            return QUrl::fromUserInput(path).toLocalFile();
        return path;
    }
}

QString AmmUiBackend::defaultWalletHome()
{
    const QByteArray override = qgetenv(WALLET_HOME_ENV);
    if (!override.isEmpty())
        return QString::fromLocal8Bit(override);
    // LEZ's canonical wallet home, shared with the wallet UI and other LEZ apps
    // (matches lez/wallet get_home_default_path()).
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
      m_accountModel(new AccountModel(this)),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_logos(new LogosModules(m_logosAPI)),
      m_net(new QNetworkAccessManager(this)),
      m_reachabilityTimer(new QTimer(this))
{
    // PROP defaults via the generated setters.
    setIsWalletOpen(false);
    setLastSyncedBlock(0);
    setCurrentBlockHeight(0);
    setWalletHome(defaultWalletHome());
    // Assume reachable until a probe proves otherwise (avoids a startup flash).
    setSequencerReachable(true);

    // Periodically re-probe the sequencer so the banner reacts to a node going
    // up/down while the app is running. Probes are no-ops until a wallet (and
    // thus a sequencer address) is open.
    m_reachabilityTimer->setInterval(10000);
    connect(m_reachabilityTimer, &QTimer::timeout, this, [this]() { checkReachability(); });
    m_reachabilityTimer->start();

    // Always resolve against the canonical wallet home (LEE_WALLET_HOME_DIR or
    // ~/.lee/wallet). We intentionally don't seed config/storage paths from
    // QSettings anymore: a previously-persisted per-app path (~/.lee/amm-wallet)
    // would otherwise override the default and pin the app to the old keystore.

    // A wallet exists on disk if its storage file is present (drives whether
    // the navbar "Connect" reconnects or offers to create a wallet).
    const QString effectiveStorage = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    setWalletExists(QFileInfo::exists(effectiveStorage));

    // ui-host runs our constructor inside initLogos(), synchronously, BEFORE
    // it enables remoting and emits READY. Any blocking RPC here would stall
    // ui-host startup past its ready watchdog. Defer the open+refresh chain to
    // the first event-loop tick so ui-host finishes wiring itself up first.
    QTimer::singleShot(0, this, [this]() { openOrAdoptWallet(); });

    // Save wallet on quit; host may not call destructors so this is best-effort.
    connect(qApp, &QCoreApplication::aboutToQuit, this,
            [this]() { saveWallet(); }, Qt::DirectConnection);
}

AmmUiBackend::~AmmUiBackend()
{
    saveWallet();
    delete m_logos;
}

void AmmUiBackend::openOrAdoptWallet()
{
    // Respect an explicit user disconnect: stay locked, show "Connect".
    if (QSettings(SETTINGS_ORG, SETTINGS_APP).value(DISCONNECTED_KEY, false).toBool())
        return;

    // In Basecamp the logos_execution_zone module is a single shared instance,
    // so the wallet may already be open (e.g. opened by the dedicated wallet
    // app). Adopt that wallet instead of fighting over it: mirror its state
    // rather than re-opening from disk, which could clobber unsaved in-memory
    // accounts the other app holds. A freshly-created shared wallet can be open
    // with zero accounts, so we can't key off list_accounts() alone (see
    // sharedWalletIsOpen).
    if (sharedWalletIsOpen()) {
        const QJsonArray existing = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
        qDebug() << "AmmUiBackend: adopting already-open shared wallet"
                 << existing.size() << "accounts";
        setIsWalletOpen(true);
        m_accountModel->replaceFromJsonArray(existing);
        refreshBalances();
        refreshSequencerAddr();
        return;
    }

    // Standalone (own core instance): auto-open a previously-created wallet.
    // Use persisted paths if the user picked custom ones, else the per-app
    // default. Only open if the storage actually exists, otherwise stay closed
    // so QML shows the "Connect" entry point (no noisy FFI errors on first run).
    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString stg = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    if (!QFileInfo::exists(stg))
        return; // No wallet yet — QML shows "Connect".

    qDebug() << "AmmUiBackend: opening wallet with config" << cfg << "storage" << stg;
    const int err = m_logos->logos_execution_zone.open(cfg, stg);
    if (err == WALLET_FFI_SUCCESS) {
        persistConfigPath(cfg);
        persistStoragePath(stg);
        setIsWalletOpen(true);
        refreshAccounts();
        refreshBlockHeights();
        refreshSequencerAddr();
    } else {
        qWarning() << "AmmUiBackend: wallet open failed, code" << err;
    }
}

bool AmmUiBackend::sharedWalletIsOpen()
{
    // list_accounts() is non-empty only once the wallet holds accounts, so it
    // can't distinguish "no wallet open" from "open but empty" (a wallet that
    // was just created and hasn't had an account added yet). Fall back to a
    // handle-dependent, account-independent signal: an open wallet always has a
    // sequencer address (from its config, defaulted on open), while a closed
    // core returns an empty string. This lets us adopt a freshly-created shared
    // wallet instead of falling through and re-opening it from disk.
    if (!QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts()).isEmpty())
        return true;
    return !m_logos->logos_execution_zone.get_sequencer_addr().isEmpty();
}

QString AmmUiBackend::createNewDefault(QString password)
{
    QDir().mkpath(defaultWalletHome());
    return createNew(defaultConfigPath(), defaultStoragePath(), password);
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    const QString localConfig = toLocalPath(configPath);
    const QString localStorage = toLocalPath(storagePath);
    // create_new returns the new wallet's BIP39 mnemonic (empty on failure). We
    // hand it back to the caller instead of discarding it: wallet creation is
    // the only moment the seed phrase is recoverable, so the UI must force a
    // backup step before the user can proceed.
    const QString mnemonic = m_logos->logos_execution_zone.create_new(localConfig, localStorage, password);
    if (mnemonic.isEmpty()) {
        qWarning() << "AmmUiBackend: create_new failed (empty mnemonic)";
        return QString();
    }

    persistConfigPath(localConfig);
    persistStoragePath(localStorage);
    setWalletExists(true);
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    setIsWalletOpen(true);
    refreshAccounts();
    refreshBlockHeights();
    refreshSequencerAddr();
    return mnemonic;
}

bool AmmUiBackend::openExisting()
{
    // Adopt a shared open wallet (Basecamp), else open our own from disk. A
    // freshly-created shared wallet can be open with zero accounts, so probe
    // open-ness rather than keying off list_accounts() alone.
    if (sharedWalletIsOpen()) {
        const QJsonArray existing = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
        setIsWalletOpen(true);
        m_accountModel->replaceFromJsonArray(existing);
        refreshBalances();
        refreshSequencerAddr();
        QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
        return true;
    }

    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString stg = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    if (!QFileInfo::exists(stg))
        return false;

    const int err = m_logos->logos_execution_zone.open(cfg, stg);
    if (err != WALLET_FFI_SUCCESS) {
        qWarning() << "AmmUiBackend: openExisting failed, code" << err;
        return false;
    }
    persistConfigPath(cfg);
    persistStoragePath(stg);
    setIsWalletOpen(true);
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    refreshAccounts();
    refreshBlockHeights();
    refreshSequencerAddr();
    return true;
}

void AmmUiBackend::disconnectWallet()
{
    // UI-local lock: persist wallet state, drop our view of it, and remember
    // the choice. We do NOT close the core module's wallet handle — in Basecamp
    // that instance is shared with other apps.
    saveWallet();
    setIsWalletOpen(false);
    m_accountModel->replaceFromJsonArray(QJsonArray());
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, true);
}

QString AmmUiBackend::createAccountPublic()
{
    const QString result = m_logos->logos_execution_zone.create_account_public();
    if (!result.isEmpty())
        refreshAccounts();
    return result;
}

QString AmmUiBackend::createAccountPrivate()
{
    const QString result = m_logos->logos_execution_zone.create_account_private();
    if (!result.isEmpty())
        refreshAccounts();
    return result;
}

void AmmUiBackend::refreshAccounts()
{
    const QJsonArray arr = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
    m_accountModel->replaceFromJsonArray(arr);
    refreshBalances();
}

void AmmUiBackend::refreshBalances()
{
    refreshBlockHeights();
    if (currentBlockHeight() > 0)
        m_logos->logos_execution_zone.sync_to_block(static_cast<quint64>(currentBlockHeight()));

    for (int i = 0; i < m_accountModel->count(); ++i) {
        const QModelIndex idx = m_accountModel->index(i, 0);
        const QString addr = m_accountModel->data(idx, AccountModel::AddressRole).toString();
        const bool isPub = m_accountModel->data(idx, AccountModel::IsPublicRole).toBool();
        m_accountModel->setBalanceByAddress(addr, getBalance(addr, isPub));
    }
    saveWallet();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_logos->logos_execution_zone.get_balance(accountIdHex, isPublic);
}

void AmmUiBackend::refreshBlockHeights()
{
    const int lastVal = m_logos->logos_execution_zone.get_last_synced_block();
    const int currentVal = m_logos->logos_execution_zone.get_current_block_height();
    if (lastSyncedBlock() != lastVal)
        setLastSyncedBlock(lastVal);
    if (currentBlockHeight() != currentVal)
        setCurrentBlockHeight(currentVal);
}

void AmmUiBackend::refreshSequencerAddr()
{
    const QString addr = m_logos->logos_execution_zone.get_sequencer_addr();
    if (sequencerAddr() != addr)
        setSequencerAddr(addr);
    // Probe right away so the banner reflects the (possibly new) endpoint
    // without waiting for the next periodic tick.
    checkReachability();
}

void AmmUiBackend::checkReachability()
{
    const QString addr = sequencerAddr();
    if (addr.isEmpty())
        return;

    QNetworkRequest req{QUrl(addr)};
    req.setTransferTimeout(4000);
    QNetworkReply* reply = m_net->get(req);
    connect(reply, &QNetworkReply::finished, this, [this, reply]() {
        // Any HTTP response (even a 404) means the node is up; only a transport
        // failure (connection refused, host not found, timeout) counts as down.
        const bool gotHttpStatus =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        const bool reachable = gotHttpStatus || reply->error() == QNetworkReply::NoError;
        if (sequencerReachable() != reachable)
            setSequencerReachable(reachable);
        reply->deleteLater();
    });
}

void AmmUiBackend::saveWallet()
{
    if (isWalletOpen())
        m_logos->logos_execution_zone.save();
}

// These only update the in-session PROPs (so subsequent open/refresh calls
// reuse the same path). They are no longer written to QSettings: the app
// always resolves against the canonical wallet home, so there's nothing to
// remember across launches.
void AmmUiBackend::persistConfigPath(const QString& path)
{
    setConfigPath(toLocalPath(path));
}

void AmmUiBackend::persistStoragePath(const QString& path)
{
    setStoragePath(toLocalPath(path));
}

bool AmmUiBackend::changeSequencerAddr(QString url)
{
    const QString trimmed = url.trimmed();
    if (trimmed.isEmpty()) {
        qWarning() << "AmmUiBackend: refusing to set empty sequencer_addr";
        return false;
    }

    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();

    // Preserve the other config fields (poll timeouts, retries) — only swap the
    // endpoint. The wallet reads this file on open via from_path_or_initialize_default.
    QJsonObject obj;
    QFile in(cfg);
    if (in.open(QIODevice::ReadOnly)) {
        obj = QJsonDocument::fromJson(in.readAll()).object();
        in.close();
    }
    obj.insert(QStringLiteral("sequencer_addr"), trimmed);

    QFile out(cfg);
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        qWarning() << "AmmUiBackend: cannot write wallet config" << cfg;
        return false;
    }
    out.write(QJsonDocument(obj).toJson(QJsonDocument::Indented));
    out.close();

    // Re-open so the live wallet uses the new endpoint right away.
    if (isWalletOpen()) {
        const QString stg = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
        const int err = m_logos->logos_execution_zone.open(cfg, stg);
        if (err != WALLET_FFI_SUCCESS) {
            qWarning() << "AmmUiBackend: reopen after sequencer change failed, code" << err;
            return false;
        }
        refreshSequencerAddr();
        refreshAccounts();
    }
    return true;
}

void AmmUiBackend::copyToClipboard(QString text)
{
    if (QGuiApplication::clipboard())
        QGuiApplication::clipboard()->setText(text);
}
