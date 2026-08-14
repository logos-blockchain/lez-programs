#ifndef WALLET_BACKEND_LOGIC_H
#define WALLET_BACKEND_LOGIC_H

// Shared wallet backend logic for LEZ ui_qml apps (apps/common/wallet-ui).
//
// Every app's backend derives from a per-app QtRO SimpleSource generated from
// its own <App>UiBackend.rep. Those .rep files are byte-identical except for
// the class name, so the generated SimpleSources expose an identical property
// surface (isWalletOpen, walletExists, configPath, storagePath, walletHome,
// lastSyncedBlock, currentBlockHeight, sequencerAddr, sequencerReachable) and
// the same wallet slots. That lets the whole implementation live here once,
// parameterised on the generated SimpleSource: WalletBackendLogic<Base> derives
// from Base, so it can reach Base's protected PROP setters and override Base's
// pure-virtual .rep slots directly.
//
// A host backend is then a near-empty shell — it only adds Q_OBJECT, the
// accountModel Q_PROPERTY, and a constructor that names the module:
//
//   class FooUiBackend : public WalletBackendLogic<FooUiBackendSimpleSource> {
//       Q_OBJECT
//       Q_PROPERTY(AccountModel* accountModel READ accountModel CONSTANT)
//   public:
//       explicit FooUiBackend(LogosAPI* api = nullptr, QObject* parent = nullptr)
//         : WalletBackendLogic(api, parent, "foo_ui", "FooUI") {}
//   };

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
#include <QObject>
#include <QSettings>
#include <QString>
#include <QTimer>
#include <QUrl>

#include "logos_api.h"
#include "logos_sdk.h"

#include "AccountModel.h"

// Base is the generated <App>UiBackendSimpleSource. We inherit it so we can
// access its protected PROP setters and override its pure-virtual .rep slots.
template <class Base>
class WalletBackendLogic : public Base {
    static constexpr const char* SETTINGS_ORG = "Logos";
    // Sticky "user pressed Disconnect" flag so the wallet stays locked across
    // relaunches until the user reconnects.
    static constexpr const char* DISCONNECTED_KEY = "disconnected";
    static constexpr int WALLET_FFI_SUCCESS = 0;
    // Wallet home env override. Mirrors LEZ's own var so the app shares the
    // canonical wallet (~/.lee/wallet) used by the wallet UI and other apps.
    static constexpr const char* WALLET_HOME_ENV = "LEE_WALLET_HOME_DIR";

    // Normalise file:// URLs and OS paths to a plain local path.
    static QString toLocalPath(const QString& path) {
        if (path.startsWith("file://") || path.contains("/"))
            return QUrl::fromUserInput(path).toLocalFile();
        return path;
    }

public:
    // moduleName: the LEZ module name (e.g. "token_ui"), used for the fallback
    // LogosAPI. settingsApp: the QSettings application key (e.g. "TokenUI").
    WalletBackendLogic(LogosAPI* logosAPI, QObject* parent,
                       const char* moduleName, const char* settingsApp)
        : Base(parent),
          m_settingsApp(settingsApp),
          m_accountModel(new AccountModel(this)),
          m_logosAPI(logosAPI ? logosAPI : new LogosAPI(moduleName, this)),
          m_logos(new LogosModules(m_logosAPI)),
          m_net(new QNetworkAccessManager(this)),
          m_reachabilityTimer(new QTimer(this))
    {
        // PROP defaults via the generated (protected) setters.
        this->setIsWalletOpen(false);
        this->setLastSyncedBlock(0);
        this->setCurrentBlockHeight(0);
        this->setWalletHome(defaultWalletHome());
        // Assume reachable until a probe proves otherwise (avoids a startup flash).
        this->setSequencerReachable(true);

        // Periodically re-probe the sequencer so the banner reacts to a node
        // going up/down while the app is running. Probes are no-ops until a
        // wallet (and thus a sequencer address) is open.
        m_reachabilityTimer->setInterval(10000);
        QObject::connect(m_reachabilityTimer, &QTimer::timeout, this,
                         [this]() { checkReachability(); });
        m_reachabilityTimer->start();

        // Always resolve against the canonical wallet home (LEE_WALLET_HOME_DIR
        // or ~/.lee/wallet). We intentionally don't seed config/storage paths
        // from QSettings: a previously-persisted per-app path would otherwise
        // override the default and pin the app to the old keystore.

        // A wallet exists on disk if its storage file is present (drives whether
        // the navbar "Connect" reconnects or offers to create a wallet).
        const QString effectiveStorage =
            this->storagePath().isEmpty() ? defaultStoragePath() : this->storagePath();
        this->setWalletExists(QFileInfo::exists(effectiveStorage));

        // ui-host runs our constructor inside initLogos(), synchronously, BEFORE
        // it enables remoting and emits READY. Any blocking RPC here would stall
        // ui-host startup past its ready watchdog. Defer the open+refresh chain
        // to the first event-loop tick so ui-host finishes wiring itself up.
        QTimer::singleShot(0, this, [this]() { openOrAdoptWallet(); });

        // Save wallet on quit; host may not call destructors so this is
        // best-effort.
        QObject::connect(qApp, &QCoreApplication::aboutToQuit, this,
                         [this]() { saveWallet(); }, Qt::DirectConnection);
    }

    ~WalletBackendLogic() override
    {
        saveWallet();
        delete m_logos;
    }

    AccountModel* accountModel() const { return m_accountModel; }

    // ── .rep slot overrides ──────────────────────────────────────────────────

    QString createAccountPublic() override
    {
        const QString result = m_logos->logos_execution_zone.create_account_public();
        if (!result.isEmpty())
            refreshAccounts();
        return result;
    }

    QString createAccountPrivate() override
    {
        const QString result = m_logos->logos_execution_zone.create_account_private();
        if (!result.isEmpty())
            refreshAccounts();
        return result;
    }

    void refreshAccounts() override
    {
        const QJsonArray arr = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
        m_accountModel->replaceFromJsonArray(arr);
        refreshBalances();
    }

    void refreshBalances() override
    {
        refreshBlockHeights();
        if (this->currentBlockHeight() > 0)
            m_logos->logos_execution_zone.sync_to_block(static_cast<quint64>(this->currentBlockHeight()));

        for (int i = 0; i < m_accountModel->count(); ++i) {
            const QModelIndex idx = m_accountModel->index(i, 0);
            const QString addr = m_accountModel->data(idx, AccountModel::AddressRole).toString();
            const bool isPub = m_accountModel->data(idx, AccountModel::IsPublicRole).toBool();
            m_accountModel->setBalanceByAddress(addr, getBalance(addr, isPub));
        }
        saveWallet();
    }

    QString getBalance(QString accountIdHex, bool isPublic) override
    {
        return m_logos->logos_execution_zone.get_balance(accountIdHex, isPublic);
    }

    QString createNewDefault(QString password) override
    {
        QDir().mkpath(defaultWalletHome());
        return createNew(defaultConfigPath(), defaultStoragePath(), password);
    }

    QString createNew(QString configPath, QString storagePath, QString password) override
    {
        const QString localConfig = toLocalPath(configPath);
        const QString localStorage = toLocalPath(storagePath);
        // create_new returns the new wallet's BIP39 mnemonic (empty on failure).
        // We hand it back to the caller instead of discarding it: wallet creation
        // is the only moment the seed phrase is recoverable, so the UI must force
        // a backup step before the user can proceed.
        const QString mnemonic = m_logos->logos_execution_zone.create_new(localConfig, localStorage, password);
        if (mnemonic.isEmpty()) {
            qWarning() << m_settingsApp << "backend: create_new failed (empty mnemonic)";
            return QString();
        }

        persistConfigPath(localConfig);
        persistStoragePath(localStorage);
        this->setWalletExists(true);
        QSettings(SETTINGS_ORG, m_settingsApp).setValue(DISCONNECTED_KEY, false);
        this->setIsWalletOpen(true);
        refreshAccounts();
        refreshBlockHeights();
        refreshSequencerAddr();
        return mnemonic;
    }

    bool openExisting() override
    {
        // Adopt a shared open wallet (Basecamp), else open our own from disk. A
        // freshly-created shared wallet can be open with zero accounts, so probe
        // open-ness rather than keying off list_accounts() alone.
        if (sharedWalletIsOpen()) {
            const QJsonArray existing = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
            this->setIsWalletOpen(true);
            m_accountModel->replaceFromJsonArray(existing);
            refreshBalances();
            refreshSequencerAddr();
            QSettings(SETTINGS_ORG, m_settingsApp).setValue(DISCONNECTED_KEY, false);
            return true;
        }

        const QString cfg = this->configPath().isEmpty() ? defaultConfigPath() : this->configPath();
        const QString stg = this->storagePath().isEmpty() ? defaultStoragePath() : this->storagePath();
        if (!QFileInfo::exists(stg))
            return false;

        const int err = m_logos->logos_execution_zone.open(cfg, stg);
        if (err != WALLET_FFI_SUCCESS) {
            qWarning() << m_settingsApp << "backend: openExisting failed, code" << err;
            return false;
        }
        persistConfigPath(cfg);
        persistStoragePath(stg);
        this->setIsWalletOpen(true);
        QSettings(SETTINGS_ORG, m_settingsApp).setValue(DISCONNECTED_KEY, false);
        refreshAccounts();
        refreshBlockHeights();
        refreshSequencerAddr();
        return true;
    }

    void disconnectWallet() override
    {
        // UI-local lock: persist wallet state, drop our view of it, and remember
        // the choice. We do NOT close the core module's wallet handle — in
        // Basecamp that instance is shared with other apps.
        saveWallet();
        this->setIsWalletOpen(false);
        m_accountModel->replaceFromJsonArray(QJsonArray());
        QSettings(SETTINGS_ORG, m_settingsApp).setValue(DISCONNECTED_KEY, true);
    }

    bool changeSequencerAddr(QString url) override
    {
        const QString trimmed = url.trimmed();
        if (trimmed.isEmpty()) {
            qWarning() << m_settingsApp << "backend: refusing to set empty sequencer_addr";
            return false;
        }

        const QString cfg = this->configPath().isEmpty() ? defaultConfigPath() : this->configPath();

        // Preserve the other config fields (poll timeouts, retries) — only swap
        // the endpoint. The wallet reads this file on open via
        // from_path_or_initialize_default.
        QJsonObject obj;
        QFile in(cfg);
        if (in.open(QIODevice::ReadOnly)) {
            obj = QJsonDocument::fromJson(in.readAll()).object();
            in.close();
        }
        obj.insert(QStringLiteral("sequencer_addr"), trimmed);

        QFile out(cfg);
        if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            qWarning() << m_settingsApp << "backend: cannot write wallet config" << cfg;
            return false;
        }
        out.write(QJsonDocument(obj).toJson(QJsonDocument::Indented));
        out.close();

        // Re-open so the live wallet uses the new endpoint right away.
        if (this->isWalletOpen()) {
            const QString stg = this->storagePath().isEmpty() ? defaultStoragePath() : this->storagePath();
            const int err = m_logos->logos_execution_zone.open(cfg, stg);
            if (err != WALLET_FFI_SUCCESS) {
                qWarning() << m_settingsApp << "backend: reopen after sequencer change failed, code" << err;
                return false;
            }
            refreshSequencerAddr();
            refreshAccounts();
        }
        return true;
    }

    void copyToClipboard(QString text) override
    {
        if (QGuiApplication::clipboard())
            QGuiApplication::clipboard()->setText(text);
    }

private:
    // ── Internal helpers (not part of the .rep slot surface) ─────────────────

    static QString defaultWalletHome()
    {
        const QByteArray override = qgetenv(WALLET_HOME_ENV);
        if (!override.isEmpty())
            return QString::fromLocal8Bit(override);
        // LEZ's canonical wallet home, shared with the wallet UI and other LEZ
        // apps (matches lez/wallet get_home_default_path()).
        return QDir::homePath() + QStringLiteral("/.lee/wallet");
    }

    QString defaultConfigPath() const
    {
        return defaultWalletHome() + QStringLiteral("/wallet_config.json");
    }

    QString defaultStoragePath() const
    {
        return defaultWalletHome() + QStringLiteral("/storage.json");
    }

    void openOrAdoptWallet()
    {
        // Respect an explicit user disconnect: stay locked, show "Connect".
        if (QSettings(SETTINGS_ORG, m_settingsApp).value(DISCONNECTED_KEY, false).toBool())
            return;

        // In Basecamp the logos_execution_zone module is a single shared
        // instance, so the wallet may already be open (e.g. opened by the
        // dedicated wallet app). Adopt that wallet instead of fighting over it:
        // mirror its state rather than re-opening from disk, which could clobber
        // unsaved in-memory accounts the other app holds. A freshly-created
        // shared wallet can be open with zero accounts, so we can't key off
        // list_accounts() alone (see sharedWalletIsOpen).
        if (sharedWalletIsOpen()) {
            const QJsonArray existing = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
            qDebug() << m_settingsApp << "backend: adopting already-open shared wallet"
                     << existing.size() << "accounts";
            this->setIsWalletOpen(true);
            m_accountModel->replaceFromJsonArray(existing);
            refreshBalances();
            refreshSequencerAddr();
            return;
        }

        // Standalone (own core instance): auto-open a previously-created wallet.
        // Use persisted paths if the user picked custom ones, else the per-app
        // default. Only open if the storage actually exists, otherwise stay
        // closed so QML shows the "Connect" entry point (no noisy FFI errors on
        // first run).
        const QString cfg = this->configPath().isEmpty() ? defaultConfigPath() : this->configPath();
        const QString stg = this->storagePath().isEmpty() ? defaultStoragePath() : this->storagePath();
        if (!QFileInfo::exists(stg))
            return; // No wallet yet — QML shows "Connect".

        qDebug() << m_settingsApp << "backend: opening wallet with config" << cfg << "storage" << stg;
        const int err = m_logos->logos_execution_zone.open(cfg, stg);
        if (err == WALLET_FFI_SUCCESS) {
            persistConfigPath(cfg);
            persistStoragePath(stg);
            this->setIsWalletOpen(true);
            refreshAccounts();
            refreshBlockHeights();
            refreshSequencerAddr();
        } else {
            qWarning() << m_settingsApp << "backend: wallet open failed, code" << err;
        }
    }

    bool sharedWalletIsOpen()
    {
        // list_accounts() is non-empty only once the wallet holds accounts, so
        // it can't distinguish "no wallet open" from "open but empty" (a wallet
        // that was just created and hasn't had an account added yet). Fall back
        // to a handle-dependent, account-independent signal: an open wallet
        // always has a sequencer address (from its config, defaulted on open),
        // while a closed core returns an empty string. This lets us adopt a
        // freshly-created shared wallet instead of re-opening it from disk.
        if (!QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts()).isEmpty())
            return true;
        return !m_logos->logos_execution_zone.get_sequencer_addr().isEmpty();
    }

    void refreshBlockHeights()
    {
        const int lastVal = m_logos->logos_execution_zone.get_last_synced_block();
        const int currentVal = m_logos->logos_execution_zone.get_current_block_height();
        if (this->lastSyncedBlock() != lastVal)
            this->setLastSyncedBlock(lastVal);
        if (this->currentBlockHeight() != currentVal)
            this->setCurrentBlockHeight(currentVal);
    }

    void refreshSequencerAddr()
    {
        const QString addr = m_logos->logos_execution_zone.get_sequencer_addr();
        if (this->sequencerAddr() != addr)
            this->setSequencerAddr(addr);
        // Probe right away so the banner reflects the (possibly new) endpoint
        // without waiting for the next periodic tick.
        checkReachability();
    }

    void checkReachability()
    {
        const QString addr = this->sequencerAddr();
        if (addr.isEmpty())
            return;

        QNetworkRequest req{QUrl(addr)};
        req.setTransferTimeout(4000);
        QNetworkReply* reply = m_net->get(req);
        QObject::connect(reply, &QNetworkReply::finished, this, [this, reply]() {
            // Any HTTP response (even a 404) means the node is up; only a
            // transport failure (connection refused, host not found, timeout)
            // counts as down.
            const bool gotHttpStatus =
                reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
            const bool reachable = gotHttpStatus || reply->error() == QNetworkReply::NoError;
            if (this->sequencerReachable() != reachable)
                this->setSequencerReachable(reachable);
            reply->deleteLater();
        });
    }

    void saveWallet()
    {
        if (this->isWalletOpen())
            m_logos->logos_execution_zone.save();
    }

    // These only update the in-session PROPs (so subsequent open/refresh calls
    // reuse the same path). They are not written to QSettings: the app always
    // resolves against the canonical wallet home, so there's nothing to remember
    // across launches.
    void persistConfigPath(const QString& path) { this->setConfigPath(toLocalPath(path)); }
    void persistStoragePath(const QString& path) { this->setStoragePath(toLocalPath(path)); }

    QString m_settingsApp;

    AccountModel* m_accountModel;

    LogosAPI* m_logosAPI;
    LogosModules* m_logos;

    QNetworkAccessManager* m_net;
    QTimer* m_reachabilityTimer;
};

#endif // WALLET_BACKEND_LOGIC_H
