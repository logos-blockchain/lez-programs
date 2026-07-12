#include "AmmUiBackend.h"

#include <QClipboard>
#include <QCoreApplication>
#include <QDateTime>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QRegularExpression>
#include <QScopedValueRollback>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "logos_api.h"
#include "logos_sdk.h"

#include "amm_client.h"

namespace {
    const char SETTINGS_ORG[] = "Logos";
    const char SETTINGS_APP[] = "AmmUI";
    const char DISCONNECTED_KEY[] = "disconnected";
    const char WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";
    const char NETWORK_ENV[] = "AMM_UI_NETWORK";
    const char DEVNET_FILE_ENV[] = "AMM_UI_DEVNET_FILE";
    const int WALLET_FFI_SUCCESS = 0;
    const int CHECKPOINT_BLOCK_ID = 10;
    const int BLOCK_HASH_OFFSET = 40;
    const int BLOCK_HASH_SIZE = 32;
    const char SCHEMA[] = "new-position.v1";

    using AmmClientOperation = char* (*)(const char*);

    QString toLocalPath(const QString& path)
    {
        if (path.startsWith(QStringLiteral("file://")) || path.contains(QLatin1Char('/')))
            return QUrl::fromUserInput(path).toLocalFile();
        return path;
    }

    bool isLowerHex(const QString& value, int size)
    {
        if (value.size() != size)
            return false;
        for (const QChar character : value) {
            if (!character.isDigit()
                && (character < QLatin1Char('a') || character > QLatin1Char('f'))) {
                return false;
            }
        }
        return true;
    }

    bool isHex(const QString& value, int size)
    {
        if (value.size() != size)
            return false;
        for (const QChar character : value) {
            const QChar lower = character.toLower();
            if (!character.isDigit()
                && (lower < QLatin1Char('a') || lower > QLatin1Char('f'))) {
                return false;
            }
        }
        return true;
    }

    QJsonObject issue(const QString& code, const QJsonArray& blockingFields = {})
    {
        return {
            { QStringLiteral("code"), code },
            { QStringLiteral("recoverable"), true },
            { QStringLiteral("blockingFields"), blockingFields },
            { QStringLiteral("details"), QJsonObject() },
        };
    }

    QJsonObject publicError(const QString& code,
                            const QJsonArray& blockingFields = {},
                            const QJsonObject& details = {})
    {
        QJsonObject error = issue(code, blockingFields);
        error.insert(QStringLiteral("details"), details);
        return {
            { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
            { QStringLiteral("status"), QStringLiteral("error") },
            { QStringLiteral("canSubmit"), false },
            { QStringLiteral("code"), code },
            { QStringLiteral("errors"), QJsonArray { error } },
            { QStringLiteral("warnings"), QJsonArray() },
            { QStringLiteral("accountPreview"), QJsonArray() },
        };
    }

    QJsonObject contextState(const QString& status,
                             const QString& networkId,
                             const QString& fingerprint = {})
    {
        return {
            { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
            { QStringLiteral("status"), status },
            { QStringLiteral("networkId"), networkId },
            { QStringLiteral("networkFingerprint"), fingerprint },
            { QStringLiteral("tokens"), QJsonArray() },
            { QStringLiteral("feeTiers"), QJsonArray {
                QJsonObject { { QStringLiteral("feeBps"), 1 }, { QStringLiteral("label"), QStringLiteral("0.01%") }, { QStringLiteral("enabled"), true } },
                QJsonObject { { QStringLiteral("feeBps"), 5 }, { QStringLiteral("label"), QStringLiteral("0.05%") }, { QStringLiteral("enabled"), true } },
                QJsonObject { { QStringLiteral("feeBps"), 30 }, { QStringLiteral("label"), QStringLiteral("0.30%") }, { QStringLiteral("enabled"), true } },
                QJsonObject { { QStringLiteral("feeBps"), 100 }, { QStringLiteral("label"), QStringLiteral("1.00%") }, { QStringLiteral("enabled"), true } },
            } },
            { QStringLiteral("warnings"), QJsonArray() },
        };
    }

    QJsonObject callClient(AmmClientOperation operation,
                           const QJsonObject& request,
                           bool* ok)
    {
        *ok = false;
        const QByteArray payload = QJsonDocument(request).toJson(QJsonDocument::Compact);
        char* raw = operation(payload.constData());
        if (!raw) {
            qWarning() << "AmmUiBackend: AMM client returned a null response";
            return {};
        }

        const QByteArray response(raw);
        amm_free(raw);
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(response, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
            qWarning() << "AmmUiBackend: invalid AMM client response";
            return {};
        }

        const QJsonObject envelope = document.object();
        if (!envelope.value(QStringLiteral("ok")).toBool(false)) {
            qWarning() << "AmmUiBackend: AMM client boundary failure:"
                       << envelope.value(QStringLiteral("error")).toString();
            return {};
        }
        if (!envelope.value(QStringLiteral("value")).isObject()) {
            qWarning() << "AmmUiBackend: AMM client value is not an object";
            return {};
        }
        *ok = true;
        return envelope.value(QStringLiteral("value")).toObject();
    }

    QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
    {
        return QJsonDocument(QJsonObject {
            { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
            { QStringLiteral("id"), 1 },
            { QStringLiteral("method"), method },
            { QStringLiteral("params"), params },
        }).toJson(QJsonDocument::Compact);
    }

    QString blockHashFromResponse(const QByteArray& payload)
    {
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(payload, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject())
            return {};
        const QByteArray block =
            QByteArray::fromBase64(document.object().value(QStringLiteral("result")).toString().toLatin1());
        if (block.size() < BLOCK_HASH_OFFSET + BLOCK_HASH_SIZE)
            return {};
        return QString::fromLatin1(block.mid(BLOCK_HASH_OFFSET, BLOCK_HASH_SIZE).toHex());
    }

    QString channelIdFromResponse(const QByteArray& payload)
    {
        QJsonParseError parseError;
        const QJsonDocument document = QJsonDocument::fromJson(payload, &parseError);
        if (parseError.error != QJsonParseError::NoError || !document.isObject())
            return {};
        const QString channel = document.object().value(QStringLiteral("result")).toString();
        return isLowerHex(channel, 64) ? channel : QString();
    }

    QJsonArray variantStringArray(const QVariant& value)
    {
        QJsonArray result;
        for (const QVariant& item : value.toList())
            result.append(item.toString());
        return result;
    }

    QStringList jsonStringList(const QJsonArray& values)
    {
        QStringList result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(value.toString());
        return result;
    }

    QVariantList jsonBoolList(const QJsonArray& values)
    {
        QVariantList result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(value.toBool());
        return result;
    }

    QVariantList jsonUIntList(const QJsonArray& values)
    {
        QVariantList result;
        result.reserve(values.size());
        for (const QJsonValue& value : values)
            result.append(QVariant::fromValue(static_cast<quint32>(value.toInteger())));
        return result;
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

namespace {
constexpr int CHECKPOINT_BLOCK_ID = 10;
constexpr int BLOCK_HASH_OFFSET = 40;
constexpr int BLOCK_HASH_SIZE = 32;
const QString DEFAULT_PROGRAM_OWNER(64, QLatin1Char('0'));

QByteArray resource(const QString& path)
{
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
}

QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
{
    return QJsonDocument(QJsonObject {
        { QStringLiteral("jsonrpc"), QStringLiteral("2.0") },
        { QStringLiteral("id"), 1 },
        { QStringLiteral("method"), method },
        { QStringLiteral("params"), params },
    }).toJson(QJsonDocument::Compact);
}

QString blockHashFromResponse(const QByteArray& payload)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(payload, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return {};
    const QByteArray block = QByteArray::fromBase64(
        document.object().value(QStringLiteral("result")).toString().toLatin1());
    if (block.size() < BLOCK_HASH_OFFSET + BLOCK_HASH_SIZE)
        return {};
    return QString::fromLatin1(block.mid(BLOCK_HASH_OFFSET, BLOCK_HASH_SIZE).toHex());
}

QString channelIdFromResponse(const QByteArray& payload)
{
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(payload, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return {};
    const QString channel = document.object().value(QStringLiteral("result")).toString();
    return ActiveNetwork::isValidIdentity(channel) ? channel : QString();
}

QString decimalAdd(const QString& left, const QString& right)
{
    if (left.isEmpty() || right.isEmpty())
        return {};
    if (!std::all_of(left.cbegin(), left.cend(), [](QChar value) { return value.isDigit(); })
        || !std::all_of(right.cbegin(), right.cend(), [](QChar value) { return value.isDigit(); })) {
        return {};
    }
    QString result;
    result.reserve(std::max(left.size(), right.size()) + 1);
    qsizetype leftIndex = left.size();
    qsizetype rightIndex = right.size();
    int carry = 0;
    while (leftIndex > 0 || rightIndex > 0 || carry > 0) {
        const int leftDigit = leftIndex > 0
            ? left.at(--leftIndex).digitValue() : 0;
        const int rightDigit = rightIndex > 0
            ? right.at(--rightIndex).digitValue() : 0;
        const int sum = leftDigit + rightDigit + carry;
        result.prepend(QChar(QLatin1Char('0').unicode() + sum % 10));
        carry = sum / 10;
    }
    while (result.size() > 1 && result.startsWith(QLatin1Char('0')))
        result.remove(0, 1);
    return result;
}

QJsonObject enumFields(const QJsonValue& value, const QString& variant)
{
    return value.toObject().value(variant).toObject();
}

WalletAccountRead accountRead(const WalletAccount& account)
{
    WalletAccountRead read;
    read.accountId = account.address;
    read.status = account.readStatus;
    read.programOwner = account.programOwner;
    read.dataHex = account.dataHex;
    return read;
}
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
    loadNetworkConfig();
    setNewPositionContext(contextState(
        m_networkStatus == QStringLiteral("network_unknown")
            ? QStringLiteral("loading")
            : m_networkStatus,
        m_networkId,
        m_networkFingerprint).toVariantMap());

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
    refreshNewPositionContext(QVariantMap());
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
    refreshNewPositionContext(QVariantMap());
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_logos->logos_execution_zone.get_balance(accountIdHex, isPublic);
}


QJsonObject AmmUiBackend::readAccount(const QString& accountId) const
{
    QJsonObject read {
        { QStringLiteral("id"), accountId },
        { QStringLiteral("status"), QStringLiteral("read_failed") },
    };
    if (!isLowerHex(accountId, 64))
        return read;

    const QString payload = m_logos->logos_execution_zone.get_account_public(accountId);
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(payload.toUtf8(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject())
        return read;

    const QJsonObject account = document.object();
    if (!isLowerHex(account.value(QStringLiteral("program_owner")).toString(), 64)
        || !isLowerHex(account.value(QStringLiteral("balance")).toString(), 32)
        || !isLowerHex(account.value(QStringLiteral("nonce")).toString(), 32)) {
        return read;
    }
    const QString data = account.value(QStringLiteral("data")).toString();
    if (data.size() % 2 != 0) {
        return read;
    }
    for (const QChar character : data) {
        if (!character.isDigit()
            && (character < QLatin1Char('a') || character > QLatin1Char('f'))) {
            return read;
        }
    }

    read.insert(QStringLiteral("status"), QStringLiteral("ok"));
    read.insert(QStringLiteral("account"), account);
    return read;
}

QJsonArray AmmUiBackend::walletAccountReads() const
{
    QJsonArray reads;
    if (!isWalletOpen())
        return reads;

    const QVariantList accounts = m_logos->logos_execution_zone.list_accounts();
    for (const QVariant& value : accounts) {
        const QVariantMap entry = value.toMap();
        if (!entry.value(QStringLiteral("is_public"), true).toBool())
            continue;
        const QString id = entry.value(QStringLiteral("account_id")).toString();
        if (isLowerHex(id, 64))
            reads.append(readAccount(id));
    }
    return reads;
}

QJsonObject AmmUiBackend::buildQuoteInput(const QVariantMap& request,
                                          QJsonObject* error) const
{
    if (m_networkStatus != QStringLiteral("ready")) {
        *error = publicError(m_networkStatus);
        return {};
    }
    bool ok = false;
    const QJsonObject configManifest = callClient(
        amm_config_id,
        QJsonObject { { QStringLiteral("ammProgramId"), m_ammProgramId } },
        &ok);
    if (!ok) {
        *error = publicError(QStringLiteral("backend_error"));
        return {};
    }
    const QJsonObject config = readAccount(configManifest.value(QStringLiteral("configId")).toString());
    const QJsonObject requestObject = QJsonObject::fromVariantMap(request);
    const QJsonObject pairManifest = callClient(
        amm_pair_ids,
        QJsonObject {
            { QStringLiteral("ammProgramId"), m_ammProgramId },
            { QStringLiteral("config"), config },
            { QStringLiteral("tokenAId"), requestObject.value(QStringLiteral("tokenAId")) },
            { QStringLiteral("tokenBId"), requestObject.value(QStringLiteral("tokenBId")) },
        },
        &ok);
    if (!ok) {
        *error = publicError(QStringLiteral("backend_error"));
        return {};
    }
    if (pairManifest.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        *error = publicError(pairManifest.value(QStringLiteral("code")).toString());
        return {};
    }

    const QJsonObject snapshot {
        { QStringLiteral("config"), config },
        { QStringLiteral("tokenA"), readAccount(pairManifest.value(QStringLiteral("tokenAId")).toString()) },
        { QStringLiteral("tokenB"), readAccount(pairManifest.value(QStringLiteral("tokenBId")).toString()) },
        { QStringLiteral("pool"), readAccount(pairManifest.value(QStringLiteral("poolId")).toString()) },
        { QStringLiteral("vaultA"), readAccount(pairManifest.value(QStringLiteral("vaultAId")).toString()) },
        { QStringLiteral("vaultB"), readAccount(pairManifest.value(QStringLiteral("vaultBId")).toString()) },
        { QStringLiteral("lpDefinition"), readAccount(pairManifest.value(QStringLiteral("lpDefinitionId")).toString()) },
        { QStringLiteral("lpLockHolding"), readAccount(pairManifest.value(QStringLiteral("lpLockHoldingId")).toString()) },
        { QStringLiteral("currentTick"), readAccount(pairManifest.value(QStringLiteral("currentTickId")).toString()) },
        { QStringLiteral("clock"), readAccount(pairManifest.value(QStringLiteral("clockId")).toString()) },
        { QStringLiteral("walletAvailable"), isWalletOpen() },
        { QStringLiteral("walletAccounts"), walletAccountReads() },
    };
    return {
        { QStringLiteral("networkId"), m_networkId },
        { QStringLiteral("networkFingerprint"), m_networkFingerprint },
        { QStringLiteral("ammProgramId"), m_ammProgramId },
        { QStringLiteral("request"), requestObject },
        { QStringLiteral("snapshot"), snapshot },
    };
}

QVariantMap AmmUiBackend::refreshNewPositionContext(QVariantMap request)
{
    if (m_networkStatus != QStringLiteral("ready")) {
        if (m_networkStatus == QStringLiteral("network_unknown"))
            probeNetworkIdentity();
        const QJsonObject context =
            contextState(m_networkStatus, m_networkId, m_networkFingerprint);
        setNewPositionContext(context.toVariantMap());
        return context.toVariantMap();
    }
    const QJsonObject hints = QJsonObject::fromVariantMap(request);
    bool ok = false;
    const QJsonObject configManifest = callClient(
        amm_config_id,
        QJsonObject { { QStringLiteral("ammProgramId"), m_ammProgramId } },
        &ok);
    if (!ok) {
        const QJsonObject context = contextState(
            QStringLiteral("error"), m_networkId, m_networkFingerprint);
        setNewPositionContext(context.toVariantMap());
        return context.toVariantMap();
    }
    const QJsonObject config = readAccount(configManifest.value(QStringLiteral("configId")).toString());
    const QJsonArray walletAccounts = walletAccountReads();
    QJsonArray configured;
    for (const QString& id : m_configuredTokenIds)
        configured.append(id);
    const QJsonArray recent = variantStringArray(hints.value(QStringLiteral("recentTokenIds")).toVariant());
    const QJsonArray resolved = variantStringArray(hints.value(QStringLiteral("resolvedTokenIds")).toVariant());

    const QJsonObject tokenManifest = callClient(
        amm_token_ids,
        QJsonObject {
            { QStringLiteral("ammProgramId"), m_ammProgramId },
            { QStringLiteral("config"), config },
            { QStringLiteral("walletAccounts"), walletAccounts },
            { QStringLiteral("configuredTokenIds"), configured },
            { QStringLiteral("recentTokenIds"), recent },
            { QStringLiteral("resolvedTokenIds"), resolved },
        },
        &ok);
    if (!ok || tokenManifest.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        const QString code = ok
            ? tokenManifest.value(QStringLiteral("code")).toString()
            : QStringLiteral("backend_error");
        const QJsonObject context =
            contextState(code.isEmpty() ? QStringLiteral("error") : code,
                         m_networkId,
                         m_networkFingerprint);
        setNewPositionContext(context.toVariantMap());
        return context.toVariantMap();
    }

    QJsonArray definitions;
    for (const QJsonValue& id : tokenManifest.value(QStringLiteral("tokenIds")).toArray())
        definitions.append(readAccount(id.toString()));

    const QJsonObject context = callClient(
        amm_context,
        QJsonObject {
            { QStringLiteral("networkId"), m_networkId },
            { QStringLiteral("networkFingerprint"), m_networkFingerprint },
            { QStringLiteral("ammProgramId"), m_ammProgramId },
            { QStringLiteral("walletAvailable"), isWalletOpen() },
            { QStringLiteral("config"), config },
            { QStringLiteral("walletAccounts"), walletAccounts },
            { QStringLiteral("tokenDefinitions"), definitions },
            { QStringLiteral("configuredTokenIds"), configured },
            { QStringLiteral("recentTokenIds"), recent },
            { QStringLiteral("resolvedTokenIds"), resolved },
        },
        &ok);
    const QJsonObject result = ok
        ? context
        : contextState(QStringLiteral("error"), m_networkId, m_networkFingerprint);
    setNewPositionContext(result.toVariantMap());
    return result.toVariantMap();
}

QVariantMap AmmUiBackend::quoteNewPosition(QVariantMap request)
{
    QJsonObject error;
    const QJsonObject input = buildQuoteInput(request, &error);
    if (!error.isEmpty())
        return error.toVariantMap();

    bool ok = false;
    const QJsonObject quote = callClient(amm_quote, input, &ok);
    return (ok ? quote : publicError(QStringLiteral("backend_error"))).toVariantMap();
}

QVariantMap AmmUiBackend::submitNewPosition(QVariantMap request, QString quoteHash)
{
    if (m_submitInFlight)
        return publicError(QStringLiteral("submit_in_progress")).toVariantMap();
    if (!isWalletOpen())
        return publicError(QStringLiteral("wallet_unavailable")).toVariantMap();
    QScopedValueRollback<bool> submitGuard(m_submitInFlight, true);

    QJsonObject error;
    const QJsonObject input = buildQuoteInput(request, &error);
    if (!error.isEmpty())
        return error.toVariantMap();

    bool ok = false;
    const QJsonObject quote = callClient(amm_quote, input, &ok);
    if (!ok)
        return publicError(QStringLiteral("backend_error")).toVariantMap();
    if (quote.value(QStringLiteral("quoteHash")).toString() != quoteHash) {
        QJsonObject result = publicError(QStringLiteral("quote_changed"));
        result.insert(QStringLiteral("quote"), quote);
        return result.toVariantMap();
    }
    if (!quote.value(QStringLiteral("canSubmit")).toBool(false)) {
        QJsonObject result = publicError(QStringLiteral("quote_not_submittable"));
        result.insert(QStringLiteral("quote"), quote);
        return result.toVariantMap();
    }

    QJsonValue freshLp;
    if (quote.value(QStringLiteral("requiresFreshLp")).toBool(false)) {
        const QString accountId = m_logos->logos_execution_zone.create_account_public();
        if (!isLowerHex(accountId, 64)
            || m_logos->logos_execution_zone.save() != WALLET_FFI_SUCCESS) {
            return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
        }
        const QJsonObject read = readAccount(accountId);
        if (read.value(QStringLiteral("status")).toString() != QStringLiteral("ok"))
            return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
        freshLp = read;
    }

    QJsonObject planInput = input;
    planInput.insert(QStringLiteral("quoteHash"), quoteHash);
    planInput.insert(QStringLiteral("nowMs"), QDateTime::currentMSecsSinceEpoch());
    if (!freshLp.isUndefined())
        planInput.insert(QStringLiteral("freshLp"), freshLp);

    const QJsonObject plan = callClient(amm_plan, planInput, &ok);
    if (!ok)
        return publicError(QStringLiteral("backend_error")).toVariantMap();
    if (plan.value(QStringLiteral("status")).toString() != QStringLiteral("ready")) {
        const QString code = plan.value(QStringLiteral("code")).toString();
        return publicError(code.isEmpty() ? QStringLiteral("wallet_submission_failed") : code)
            .toVariantMap();
    }

    const QStringList accountIds =
        jsonStringList(plan.value(QStringLiteral("accountIds")).toArray());
    const QVariantList signingRequirements =
        jsonBoolList(plan.value(QStringLiteral("signingRequirements")).toArray());
    const QVariantList instruction =
        jsonUIntList(plan.value(QStringLiteral("instruction")).toArray());
    const QString programId = plan.value(QStringLiteral("programId")).toString();
    bool deadlineValid = false;
    const qulonglong deadline =
        plan.value(QStringLiteral("deadlineMs")).toString().toULongLong(&deadlineValid);
    if (!deadlineValid
        || static_cast<qulonglong>(QDateTime::currentMSecsSinceEpoch()) >= deadline) {
        return publicError(QStringLiteral("transaction_deadline_expired")).toVariantMap();
    }
    const QString response = m_logos->logos_execution_zone.send_generic_public_transaction(
        accountIds,
        signingRequirements,
        QVariant::fromValue(instruction),
        programId);

    QJsonParseError parseError;
    const QJsonDocument responseDocument = QJsonDocument::fromJson(response.toUtf8(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !responseDocument.isObject())
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
    const QJsonObject walletResult = responseDocument.object();
    const QJsonValue success = walletResult.value(QStringLiteral("success"));
    const QJsonValue providerError = walletResult.value(QStringLiteral("error"));
    const QString transactionId = walletResult.value(QStringLiteral("tx_hash")).toString();
    const bool providerErrorClear = providerError.isUndefined()
        || providerError.isNull()
        || (providerError.isString() && providerError.toString().isEmpty());
    if (!success.isBool()
        || !success.toBool()
        || !providerErrorClear
        || !isHex(transactionId, 64)) {
        return publicError(QStringLiteral("wallet_submission_failed")).toVariantMap();
    }

    return QJsonObject {
        { QStringLiteral("schema"), QString::fromLatin1(SCHEMA) },
        { QStringLiteral("status"), QStringLiteral("submitted") },
        { QStringLiteral("transactionId"), transactionId.toLower() },
    }.toVariantMap();
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
    const QString address = m_logos->logos_execution_zone.get_sequencer_addr();
    if (sequencerAddr() != address) {
        setSequencerAddr(address);
        if (m_networkStatus != QStringLiteral("config_missing")) {
            m_networkStatus = QStringLiteral("network_unknown");
            m_networkFingerprint.clear();
        }
    }
    checkReachability();
}

bool AmmUiBackend::loadNetworkConfig()
{
    const QByteArray selected = qgetenv(NETWORK_ENV);
    m_networkId = selected.isEmpty()
        ? QStringLiteral("testnet")
        : QString::fromLocal8Bit(selected).trimmed();

    QJsonObject entry;
    if (m_networkId == QStringLiteral("devnet")) {
        const QString path = QString::fromLocal8Bit(qgetenv(DEVNET_FILE_ENV));
        QFile file(path);
        if (path.isEmpty() || !file.open(QIODevice::ReadOnly)) {
            m_networkStatus = QStringLiteral("config_missing");
            return false;
        }
        entry = QJsonDocument::fromJson(file.readAll()).object();
        m_expectedNetworkIdentity = entry.value(QStringLiteral("channelId")).toString();
    } else {
        QFile file(QStringLiteral(":/amm/config/networks.json"));
        if (!file.open(QIODevice::ReadOnly)) {
            m_networkStatus = QStringLiteral("config_missing");
            return false;
        }
        const QJsonObject networks = QJsonDocument::fromJson(file.readAll()).object();
        entry = networks.value(m_networkId).toObject();
        m_expectedNetworkIdentity = entry.value(QStringLiteral("checkpointHash")).toString();
    }

    m_ammProgramId = entry.value(QStringLiteral("ammProgramId")).toString();
    if (!isLowerHex(m_expectedNetworkIdentity, 64)
        || !isLowerHex(m_ammProgramId, 64)) {
        m_networkStatus = QStringLiteral("config_missing");
        return false;
    }

    m_configuredTokenIds.clear();
    for (const QJsonValue& value : entry.value(QStringLiteral("tokenDefinitionIds")).toArray()) {
        const QString id = value.toString();
        if (!isLowerHex(id, 64)) {
            m_networkStatus = QStringLiteral("config_missing");
            m_configuredTokenIds.clear();
            return false;
        }
        m_configuredTokenIds.append(id);
    }
    m_networkStatus = QStringLiteral("network_unknown");
    return true;
}

void AmmUiBackend::checkReachability()
{
    const QString address = sequencerAddr();
    if (address.isEmpty())
        return;

    QNetworkRequest request{QUrl(address)};
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_net->get(request);
    connect(reply, &QNetworkReply::finished, this, [this, reply, address]() {
        if (address != sequencerAddr()) {
            reply->deleteLater();
            return;
        }
        const bool gotHttpStatus =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        const bool reachable = gotHttpStatus || reply->error() == QNetworkReply::NoError;
        if (sequencerReachable() != reachable)
            setSequencerReachable(reachable);
        reply->deleteLater();

        if (reachable && m_networkStatus == QStringLiteral("network_unknown"))
            probeNetworkIdentity();
    });
}

void AmmUiBackend::probeNetworkIdentity()
{
    if (m_identityProbeInFlight
        || m_networkStatus == QStringLiteral("config_missing")
        || sequencerAddr().isEmpty()) {
        return;
    }
    m_identityProbeInFlight = true;
    const QString address = sequencerAddr();
    const bool devnet = m_networkId == QStringLiteral("devnet");
    const QString method = devnet
        ? QStringLiteral("getChannelId")
        : QStringLiteral("getBlock");
    const QJsonArray params = devnet
        ? QJsonArray()
        : QJsonArray { CHECKPOINT_BLOCK_ID };

    QNetworkRequest request{QUrl(address)};
    request.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));
    request.setTransferTimeout(4000);
    QNetworkReply* reply = m_net->post(request, jsonRpcBody(method, params));
    connect(reply, &QNetworkReply::finished, this, [this, reply, address, devnet]() {
        m_identityProbeInFlight = false;
        if (address != sequencerAddr()) {
            reply->deleteLater();
            return;
        }

        const QByteArray payload = reply->readAll();
        const QString actual = devnet
            ? channelIdFromResponse(payload)
            : blockHashFromResponse(payload);
        if (actual.isEmpty()) {
            m_networkStatus = QStringLiteral("network_unknown");
            m_networkFingerprint.clear();
        } else if (actual != m_expectedNetworkIdentity) {
            m_networkStatus = QStringLiteral("network_mismatch");
            m_networkFingerprint.clear();
        } else {
            m_networkStatus = QStringLiteral("ready");
            m_networkFingerprint =
                (devnet ? QStringLiteral("channel:") : QStringLiteral("block10:")) + actual;
        }
        reply->deleteLater();
        refreshNewPositionContext(QVariantMap());
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
    QString normalized = url.trimmed();
    if (normalized.isEmpty()) {
        qWarning() << "AmmUiBackend: refusing to set empty sequencer_addr";
        return false;
    }

    // The wallet config parses sequencer_addr as a strict URL — a missing
    // scheme makes the whole config fail to deserialize (and would leave the
    // wallet unopenable). Default to http:// so users can type just host:port,
    // then validate before writing anything.
    if (!normalized.contains(QStringLiteral("://")))
        normalized.prepend(QStringLiteral("http://"));

    const QUrl parsed(normalized, QUrl::StrictMode);
    if (!parsed.isValid() || parsed.host().isEmpty()
        || (parsed.scheme() != QStringLiteral("http")
            && parsed.scheme() != QStringLiteral("https"))) {
        qWarning() << "AmmUiBackend: invalid sequencer URL" << url;
        return false;
    }
    normalized = parsed.toString();

    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();

    // Preserve the other config fields (poll timeouts, retries) — only swap the
    // endpoint. The wallet reads this file on open via from_path_or_initialize_default.
    QJsonObject obj;
    QFile in(cfg);
    if (in.open(QIODevice::ReadOnly)) {
        obj = QJsonDocument::fromJson(in.readAll()).object();
        in.close();
    }
    obj.insert(QStringLiteral("sequencer_addr"), normalized);

    QFile out(cfg);
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        qWarning() << "AmmUiBackend: cannot write wallet config" << cfg;
        return false;
    }
    out.write(QJsonDocument(obj).toJson(QJsonDocument::Indented));
    out.close();

    // Config is now the source of truth — reflect the change in the UI.
    if (sequencerAddr() != normalized)
        setSequencerAddr(normalized);
    checkReachability();

    // The module can't re-open an already-open wallet, so the new endpoint only
    // takes effect on the next launch. The UI confirms a restart before calling
    // this and closes the app afterwards.
    return true;
}

void AmmUiBackend::copyToClipboard(QString text)
{
    if (QGuiApplication::clipboard())
        QGuiApplication::clipboard()->setText(text);
}
