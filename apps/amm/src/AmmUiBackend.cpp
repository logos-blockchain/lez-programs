#include "AmmUiBackend.h"

#include <cctype>
#include <cstring>

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

extern "C" {
#include "amm_client_ffi.h"
}

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

    // Absolute path to the deployed AMM program's compiled ELF (amm.bin). The
    // app can't safely embed/derive this itself: the wallet module's bundled
    // AMM program may differ from whatever is actually deployed on the target
    // sequencer, and the program's ELF bytes are what determine its program id
    // (and therefore every PDA derived from it). See apps/amm/README.md.
    const char AMM_PROGRAM_BIN_ENV[] = "AMM_PROGRAM_BIN";

    // Absolute path to the JSON token-list config consumed by tokenList()
    // (see apps/amm/README.md). Config-driven so the Swap view's token picker
    // doesn't need a hardcoded/dummy token list.
    const char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";

    // Normalise file:// URLs and OS paths to a plain local path.
    QString toLocalPath(const QString& path) {
        if (path.startsWith("file://") || path.contains("/"))
            return QUrl::fromUserInput(path).toLocalFile();
        return path;
    }

    QString bytes32ToHex(const uint8_t (&b)[32]) {
        return QString::fromLatin1(QByteArray(reinterpret_cast<const char*>(b), 32).toHex());
    }

    bool hexToBytes32(const QString& hex, uint8_t (&out)[32]) {
        const QByteArray bytes = QByteArray::fromHex(hex.toUtf8());
        if (bytes.size() != 32)
            return false;
        for (int i = 0; i < 32; ++i)
            out[i] = static_cast<uint8_t>(bytes[i]);
        return true;
    }

    // Little-endian 16-byte u128 -> decimal string. QString has no direct u128
    // constructor, so accumulate into unsigned __int128 and extract digits.
    QString u128leToDecimal(const uint8_t (&le)[16]) {
        unsigned __int128 value = 0;
        for (int i = 15; i >= 0; --i)
            value = (value << 8) | static_cast<unsigned __int128>(le[i]);

        if (value == 0)
            return QStringLiteral("0");

        QString digits;
        while (value > 0) {
            const unsigned int digit = static_cast<unsigned int>(value % 10);
            digits.prepend(QChar(static_cast<char16_t>('0' + digit)));
            value /= 10;
        }
        return digits;
    }

    // Decimal string -> little-endian 16-byte u128. Inverse of u128leToDecimal.
    // Returns false (leaving `out` unwritten) on an empty string or a
    // non-digit character.
    bool decimalToU128Le(const QString& decimal, uint8_t (&out)[16]) {
        const QString trimmed = decimal.trimmed();
        if (trimmed.isEmpty())
            return false;

        unsigned __int128 value = 0;
        for (const QChar ch : trimmed) {
            if (!ch.isDigit())
                return false;
            value = value * 10 + static_cast<unsigned int>(ch.digitValue());
        }

        for (int i = 0; i < 16; ++i) {
            out[i] = static_cast<uint8_t>(value & 0xFF);
            value >>= 8;
        }
        return true;
    }

    // Base58 address of the fixed clock account consumed by SwapExactInput's
    // deadline check. Resolved to hex via the wallet module rather than
    // hardcoding a guessed hex encoding.
    const char CLOCK_ACCOUNT_BASE58[] = "4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU";

    // Extracts the hex-encoded raw account bytes from a get_account_public()
    // JSON reply. Returns an empty string if the account has no data (i.e. is
    // uninitialized/nonexistent) — note the module always includes the "data"
    // key, set to "" rather than omitted, when there's nothing to read.
    QString accountDataHex(const QString& accountJson) {
        const QJsonObject obj = QJsonDocument::fromJson(accountJson.toUtf8()).object();
        return obj.value(QStringLiteral("data")).toString();
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

QString AmmUiBackend::normalizeAccountId(const QString& id)
{
    const QString t = id.trimmed();
    // Already 64 hex chars?
    if (t.size() == 64) {
        bool allHex = true;
        for (const QChar c : t) {
            if (!std::isxdigit(static_cast<unsigned char>(c.toLatin1()))) {
                allHex = false;
                break;
            }
        }
        if (allHex)
            return t.toLower();
    }
    // Try base58 -> hex via the wallet module.
    const QString hex = m_logos->logos_execution_zone.account_id_from_base58(t);
    return hex.toLower(); // account_id_from_base58 returns "" on failure
}

QVariantMap AmmUiBackend::resolvePool(QString defAHex, QString defBHex)
{
    QVariantMap out;
    out[QStringLiteral("exists")] = false;

    // 1. Load the deployed AMM program's ELF. We can't derive this ourselves —
    // it must match whatever is actually deployed on the target sequencer.
    const QByteArray binPath = qgetenv(AMM_PROGRAM_BIN_ENV);
    if (binPath.isEmpty()) {
        qWarning() << "AmmUiBackend::resolvePool: AMM_PROGRAM_BIN not set";
        return out;
    }
    QFile elfFile(QString::fromLocal8Bit(binPath));
    if (!elfFile.open(QIODevice::ReadOnly)) {
        qWarning() << "AmmUiBackend::resolvePool: cannot read AMM_PROGRAM_BIN at" << elfFile.fileName();
        return out;
    }
    const QByteArray elf = elfFile.readAll();
    elfFile.close();
    if (elf.isEmpty()) {
        qWarning() << "AmmUiBackend::resolvePool: AMM_PROGRAM_BIN is empty";
        return out;
    }

    // 2. Derive the AMM program id from the ELF bytes.
    ProgramId ammId{};
    if (!amm_client_program_id_from_elf(reinterpret_cast<const uint8_t*>(elf.constData()),
                                        static_cast<uintptr_t>(elf.size()), &ammId)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_program_id_from_elf failed";
        return out;
    }

    // 3. Read + decode the AMM config account to discover the TWAP oracle
    // program id (needed for the current-tick PDA below). No data means the
    // AMM hasn't been initialized on this sequencer yet.
    uint8_t configPda[32];
    amm_client_config_pda(&ammId, &configPda);
    const QString configHex = bytes32ToHex(configPda);

    const QString configJson = m_logos->logos_execution_zone.get_account_public(configHex);
    const QString configDataHex = accountDataHex(configJson);
    if (configDataHex.isEmpty()) {
        qWarning() << "AmmUiBackend::resolvePool: AMM config account has no data (not initialized)";
        return out;
    }

    const QByteArray configBytes = QByteArray::fromHex(configDataHex.toUtf8());
    FfiConfigView configView{};
    if (!amm_client_decode_config(reinterpret_cast<const uint8_t*>(configBytes.constData()),
                                  static_cast<uintptr_t>(configBytes.size()), &configView)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_decode_config failed";
        return out;
    }

    // 4. Derive the pool PDA and read + decode its account. No data means
    // there's no pool (or no liquidity) for this token pair yet.
    uint8_t defA[32];
    uint8_t defB[32];
    if (!hexToBytes32(defAHex, defA) || !hexToBytes32(defBHex, defB)) {
        qWarning() << "AmmUiBackend::resolvePool: invalid defAHex/defBHex";
        return out;
    }

    uint8_t poolPda[32];
    amm_client_pool_pda(&ammId, &defA, &defB, &poolPda);
    const QString poolHex = bytes32ToHex(poolPda);

    const QString poolJson = m_logos->logos_execution_zone.get_account_public(poolHex);
    const QString poolDataHex = accountDataHex(poolJson);
    if (poolDataHex.isEmpty()) {
        // Not a warning: this is the normal "no liquidity yet" state.
        return out;
    }

    const QByteArray poolBytes = QByteArray::fromHex(poolDataHex.toUtf8());
    FfiPoolView poolView{};
    if (!amm_client_decode_pool(reinterpret_cast<const uint8_t*>(poolBytes.constData()),
                                static_cast<uintptr_t>(poolBytes.size()), &poolView)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_decode_pool failed";
        return out;
    }

    // 5. Derive the TWAP oracle's current-tick PDA for this pool.
    uint8_t currentTickPda[32];
    amm_client_current_tick_pda(&configView.twap_oracle_program_id, &poolPda, &currentTickPda);

    // 6. Assemble the result.
    out[QStringLiteral("exists")] = true;
    out[QStringLiteral("configHex")] = configHex;
    out[QStringLiteral("poolIdHex")] = poolHex;
    out[QStringLiteral("defAHex")] = bytes32ToHex(poolView.def_a);
    out[QStringLiteral("defBHex")] = bytes32ToHex(poolView.def_b);
    out[QStringLiteral("vaultAHex")] = bytes32ToHex(poolView.vault_a);
    out[QStringLiteral("vaultBHex")] = bytes32ToHex(poolView.vault_b);
    out[QStringLiteral("currentTickHex")] = bytes32ToHex(currentTickPda);
    out[QStringLiteral("reserveA")] = u128leToDecimal(poolView.reserve_a);
    out[QStringLiteral("reserveB")] = u128leToDecimal(poolView.reserve_b);
    out[QStringLiteral("feeBps")] = static_cast<int>(poolView.fees);
    return out;
}

QString AmmUiBackend::swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                      QString userOutputHoldingHex, QString amountInDecimal,
                                      QString minOutDecimal, QString deadlineDecimal)
{
    // 1. Resolve the pool's PDAs; refuse if there's no pool (or no liquidity)
    // for this token pair.
    const QVariantMap pool = resolvePool(defAHex, defBHex);
    if (!pool.value(QStringLiteral("exists")).toBool()) {
        qWarning() << "AmmUiBackend::swapExactInput: no pool for the given token pair";
        return QString();
    }

    // 2. Load the deployed AMM program's ELF (must match resolvePool's — both
    // read the same AMM_PROGRAM_BIN — since the instruction is proven against
    // this exact binary's image id).
    const QByteArray binPath = qgetenv(AMM_PROGRAM_BIN_ENV);
    if (binPath.isEmpty()) {
        qWarning() << "AmmUiBackend::swapExactInput: AMM_PROGRAM_BIN not set";
        return QString();
    }
    QFile elfFile(QString::fromLocal8Bit(binPath));
    if (!elfFile.open(QIODevice::ReadOnly)) {
        qWarning() << "AmmUiBackend::swapExactInput: cannot read AMM_PROGRAM_BIN at" << elfFile.fileName();
        return QString();
    }
    const QByteArray elf = elfFile.readAll();
    elfFile.close();
    if (elf.isEmpty()) {
        qWarning() << "AmmUiBackend::swapExactInput: AMM_PROGRAM_BIN is empty";
        return QString();
    }

    // 3. Convert amounts/deadline to the wire types amm_client_swap_words expects.
    uint8_t amtIn[16];
    uint8_t minOut[16];
    if (!decimalToU128Le(amountInDecimal, amtIn) || !decimalToU128Le(minOutDecimal, minOut)) {
        qWarning() << "AmmUiBackend::swapExactInput: invalid amountInDecimal/minOutDecimal";
        return QString();
    }
    bool deadlineOk = false;
    const quint64 deadline = deadlineDecimal.toULongLong(&deadlineOk);
    if (!deadlineOk) {
        qWarning() << "AmmUiBackend::swapExactInput: invalid deadlineDecimal";
        return QString();
    }

    // 4. Build the RISC0 instruction words for SwapExactInput.
    const AmmWords w = amm_client_swap_words(&amtIn, &minOut, deadline);
    if (!w.ok) {
        qWarning() << "AmmUiBackend::swapExactInput: amm_client_swap_words failed";
        return QString();
    }
    const std::vector<uint32_t> instruction(w.ptr, w.ptr + w.len);
    amm_client_free_words(w);

    // 5. Assemble the account id list (exact IDL order) and parallel signer
    // flags. The swap's direction is derived from the input holding's own
    // token, so the two user holdings occupy fixed role slots: user_input_holding
    // (the token being sold) then user_output_holding (received). Only the input
    // holding signs — the guest debits it via the downstream token transfer; the
    // output holding only receives and needs no signature.
    const QString clockHex =
        m_logos->logos_execution_zone.account_id_from_base58(QString::fromLatin1(CLOCK_ACCOUNT_BASE58));
    if (clockHex.isEmpty()) {
        qWarning() << "AmmUiBackend::swapExactInput: failed to resolve clock account id";
        return QString();
    }

    const QStringList accounts = {
        pool.value(QStringLiteral("configHex")).toString(),
        pool.value(QStringLiteral("poolIdHex")).toString(),
        pool.value(QStringLiteral("vaultAHex")).toString(),
        pool.value(QStringLiteral("vaultBHex")).toString(),
        userInputHoldingHex,  // user_input_holding — the token being sold (signed)
        userOutputHoldingHex, // user_output_holding — receives the token being bought
        pool.value(QStringLiteral("currentTickHex")).toString(),
        clockHex,
    };
    const QVariantList signers = { false, false, false, false, true, false, false, false };

    // 6. Submit through the wallet module's generic public-transaction entry
    // point. program_dependencies is empty: the sequencer resolves the AMM's
    // chained token/twap calls on-chain for a public tx.
    const QString resultJson = m_logos->logos_execution_zone.send_generic_public_transaction(
        accounts, signers, QVariant::fromValue(instruction), QVariant::fromValue(elf),
        QVariant::fromValue(std::vector<std::vector<uint8_t>>()));

    const QJsonObject obj = QJsonDocument::fromJson(resultJson.toUtf8()).object();
    if (!obj.value(QStringLiteral("success")).toBool()) {
        qWarning() << "AmmUiBackend::swapExactInput: transaction failed:" << resultJson;
        return QString();
    }

    refreshBalances();
    return obj.value(QStringLiteral("tx_hash")).toString();
}

QVariantList AmmUiBackend::tokenList()
{
    QVariantList out;

    const QByteArray path = qgetenv(TOKENS_CONFIG_ENV);
    if (path.isEmpty()) {
        qWarning() << "AmmUiBackend::tokenList: TOKENS_CONFIG not set";
        return out;
    }

    QFile file(QString::fromLocal8Bit(path));
    if (!file.open(QIODevice::ReadOnly)) {
        qWarning() << "AmmUiBackend::tokenList: cannot read TOKENS_CONFIG at" << file.fileName();
        return out;
    }
    const QByteArray json = file.readAll();
    file.close();

    QJsonParseError parseError{};
    const QJsonDocument doc = QJsonDocument::fromJson(json, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isArray()) {
        qWarning() << "AmmUiBackend::tokenList: TOKENS_CONFIG at" << file.fileName()
                   << "is not a valid JSON array:" << parseError.errorString();
        return out;
    }

    const QJsonArray arr = doc.array();
    for (const QJsonValue& entry : arr) {
        if (!entry.isObject()) {
            qWarning() << "AmmUiBackend::tokenList: skipping non-object entry in TOKENS_CONFIG";
            continue;
        }
        const QJsonObject obj = entry.toObject();
        const QString symbol = obj.value(QStringLiteral("symbol")).toString();

        // TOKENS_CONFIG entries may give definitionId/holding as hex or
        // base58 (the wallet/runbook display base58) — normalize both to
        // lowercase hex here so every downstream consumer (resolvePool,
        // swapExactInput, and the QML's hex comparisons) can assume hex.
        const QString definitionId =
            normalizeAccountId(obj.value(QStringLiteral("definitionId")).toString());
        const QString holding = normalizeAccountId(obj.value(QStringLiteral("holding")).toString());
        if (definitionId.isEmpty() || holding.isEmpty()) {
            qWarning() << "AmmUiBackend::tokenList: skipping token" << symbol
                       << "— cannot normalize definitionId/holding to hex (not valid hex or base58)";
            continue;
        }

        QVariantMap token;
        token[QStringLiteral("symbol")] = symbol;
        token[QStringLiteral("name")] = obj.value(QStringLiteral("name")).toString();
        token[QStringLiteral("definitionId")] = definitionId;
        token[QStringLiteral("holding")] = holding;
        token[QStringLiteral("decimals")] = obj.value(QStringLiteral("decimals")).toInt();
        out.append(token);
    }
    return out;
}
