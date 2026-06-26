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
#include <QJsonParseError>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "AmmClient.h"
#include "LogosWalletProvider.h"
#include "NewPositionRuntime.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

extern "C" {
#include "amm_client_ffi.h"
}

// Debug tracing for the swap path (resolvePool / swapExactInput), gated behind
// the AMM_DEBUG env var so normal runs stay quiet. When disabled, the streamed
// arguments — including the account_id_to_base58 round-trips used to render
// accounts — are NOT evaluated, so there's no per-call overhead.
//
// NOTE: `send_generic_public_transaction(account_ids, signing_requirements,
// instruction, program_id_hex)`. `instruction` is a byte string (`bstr`):
// declaring it as Vec<u32> makes the module's Qt/QtRO glue downgrade it to an
// opaque `any` the separate module process can't deserialize, silently dropping
// every argument. We send `instruction` as the little-endian bytes of the u32
// words, and the deployed program by its id hex (not the raw ELF). Requires the
// wallet module built with the byte-string param; see
// docs/amm-swap-qtro-serialization-bug.md.
static bool ammDebugEnabled()
{
    static const bool on = qEnvironmentVariableIsSet("AMM_DEBUG");
    return on;
}
// AMM_DBG() << ... behaves like qWarning().noquote() but only when AMM_DEBUG is
// set; otherwise the whole statement (and its arguments) is skipped.
#define AMM_DBG() \
    if (ammDebugEnabled()) qWarning().noquote()

namespace {
    // Absolute path to the deployed AMM program's RISC Zero program binary
    // (amm.bin — the `ProgramBinary` `.bin` from the docker guest build, decoded
    // on the Rust side via `ProgramBinary::decode`; NOT a raw ELF — pointing at
    // the raw guest ELF yields a different/failed program id). The app can't
    // safely embed/derive this itself: the wallet module's bundled AMM program
    // may differ from whatever is actually deployed on the target sequencer, and
    // the binary's bytes are what determine its program id (and therefore every
    // PDA derived from it). See apps/amm/README.md.
    const char AMM_PROGRAM_BIN_ENV[] = "AMM_PROGRAM_BIN";

    // Absolute path to the JSON token-list config consumed by tokenList()
    // (see apps/amm/README.md). Config-driven so the Swap view's token picker
    // doesn't need a hardcoded/dummy token list.
    const char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";

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

        const unsigned __int128 kMax = ~static_cast<unsigned __int128>(0);
        unsigned __int128 value = 0;
        for (const QChar ch : trimmed) {
            if (!ch.isDigit())
                return false;
            const unsigned int d = static_cast<unsigned int>(ch.digitValue());
            if (value > (kMax - d) / 10)
                return false; // would overflow u128
            value = value * 10 + d;
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

namespace {
    const int CHECKPOINT_BLOCK_ID = 10;
    const int BLOCK_HASH_OFFSET = 40;
    const int BLOCK_HASH_SIZE = 32;

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
        return ActiveNetwork::isValidIdentity(channel) ? channel : QString();
    }

}

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_logos(std::make_unique<LogosModules>(m_logosAPI)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("AmmUI"))),
      m_ammClient(std::make_unique<BundledAmmClient>()),
      m_newPosition(std::make_unique<NewPositionRuntime>(m_wallet.get(), m_ammClient.get())),
      m_net(new QNetworkAccessManager(this))
{
    setWalletStateReady(false);
    m_network.load();
    setNewPositionContext(m_newPosition->context(
        QVariantMap(), m_network.snapshot(), false, false));

    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    syncWalletState();
    m_walletController->start();
    QTimer::singleShot(0, this, [this]() {
        setWalletStateReady(true);
        syncWalletState();
    });
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString AmmUiBackend::createNewDefault(QString password)
{
    setWalletStateReady(false);
    const QString mnemonic = m_walletController->createDefaultWallet(password);
    setWalletStateReady(true);
    syncWalletState();
    return mnemonic;
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    setWalletStateReady(false);
    const QString mnemonic =
        m_walletController->createWallet(configPath, storagePath, password);
    setWalletStateReady(true);
    syncWalletState();
    return mnemonic;
}

bool AmmUiBackend::openExisting()
{
    setWalletStateReady(false);
    const bool opened = m_walletController->open();
    setWalletStateReady(true);
    syncWalletState();
    return opened;
}

void AmmUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
    setWalletStateReady(true);
    m_newPosition->clearWalletAccounts();
    refreshNewPositionContext(QVariantMap());
}

QString AmmUiBackend::createAccountPublic()
{
    return m_walletController->createAccount(true);
}

QString AmmUiBackend::createAccountPrivate()
{
    return m_walletController->createAccount(false);
}

void AmmUiBackend::refreshAccounts()
{
    m_walletController->refresh();
}

void AmmUiBackend::refreshBalances()
{
    m_walletController->refresh();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_walletController->balance(accountIdHex, isPublic);
}

void AmmUiBackend::refreshNewPositionContext(QVariantMap request)
{
    const bool refreshWalletAccounts =
        request.take(QStringLiteral("refreshWalletAccounts")).toBool();
    if (request.contains(QStringLiteral("recentTokenIds"))
        || request.contains(QStringLiteral("resolvedTokenIds"))) {
        m_newPositionHints = request;
    }
    else {
        request = m_newPositionHints;
    }
    if (m_network.status() == QStringLiteral("network_unknown"))
        probeNetworkIdentity();
    setNewPositionContext(m_newPosition->context(
        request, m_network.snapshot(), isWalletOpen(), refreshWalletAccounts));
}

QVariantMap AmmUiBackend::quoteNewPosition(QVariantMap request)
{
    return m_newPosition->quote(request, m_network.snapshot(), isWalletOpen());
}

QVariantMap AmmUiBackend::submitNewPosition(QVariantMap request, QString quoteHash)
{
    return m_newPosition->submit(
        request, quoteHash, m_network.snapshot(), isWalletOpen());
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    const bool walletWasOpen = isWalletOpen();
    const bool wasReachable = sequencerReachable();
    const QString previousAddress = sequencerAddr();

    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);

    const bool addressChanged = previousAddress != state.sequencerAddress;
    if (addressChanged)
        m_network.sequencerChanged(!state.sequencerAddress.isEmpty());
    if (addressChanged || wasReachable != state.sequencerReachable) {
        m_network.reachabilityChanged(state.sequencerReachable, wasReachable);
    }
    if (walletWasOpen && !state.isWalletOpen)
        m_newPosition->clearWalletAccounts();

    publishNetworkContext();
    if (state.sequencerReachable && m_network.needsIdentityProbe())
        probeNetworkIdentity();
}

void AmmUiBackend::probeNetworkIdentity()
{
    if (m_identityProbeInFlight
        || !m_network.isConfigured()
        || sequencerAddr().isEmpty()) {
        return;
    }
    m_identityProbeInFlight = true;
    m_network.beginIdentityProbe();
    publishNetworkContext();
    const QString address = sequencerAddr();
    const bool devnet = m_network.isDevnet();
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
            probeNetworkIdentity();
            return;
        }
        if (!sequencerReachable()) {
            reply->deleteLater();
            return;
        }

        const QByteArray payload = reply->readAll();
        const QString actual = devnet
            ? channelIdFromResponse(payload)
            : blockHashFromResponse(payload);
        m_network.finishIdentityProbe(actual);
        reply->deleteLater();
        publishNetworkContext();
    });
}

void AmmUiBackend::publishNetworkContext()
{
    setNewPositionContext(m_newPosition->context(
        m_newPositionHints, m_network.snapshot(), isWalletOpen(), false));
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

QByteArray AmmUiBackend::loadAmmElf()
{
    const QByteArray binPath = qgetenv(AMM_PROGRAM_BIN_ENV);
    if (binPath.isEmpty()) {
        qWarning() << "AmmUiBackend::loadAmmElf: AMM_PROGRAM_BIN not set";
        return QByteArray();
    }
    QFile elfFile(QString::fromLocal8Bit(binPath));
    if (!elfFile.open(QIODevice::ReadOnly)) {
        qWarning() << "AmmUiBackend::loadAmmElf: cannot read AMM_PROGRAM_BIN at" << elfFile.fileName();
        return QByteArray();
    }
    const QByteArray elf = elfFile.readAll();
    elfFile.close();
    if (elf.isEmpty()) {
        qWarning() << "AmmUiBackend::loadAmmElf: AMM_PROGRAM_BIN is empty";
        return QByteArray();
    }
    return elf;
}

QVariantMap AmmUiBackend::resolvePool(QString defAHex, QString defBHex)
{
    QVariantMap out;
    out[QStringLiteral("exists")] = false;

    // 1. Load the deployed AMM program's ELF. We can't derive this ourselves —
    // it must match whatever is actually deployed on the target sequencer.
    const QByteArray elf = loadAmmElf();
    if (elf.isEmpty()) {
        out[QStringLiteral("error")] = QStringLiteral("no_program_bin");
        return out;
    }

    // 2. Derive the AMM program id from the ELF bytes.
    ProgramId ammId{};
    if (!amm_client_program_id_from_elf(reinterpret_cast<const uint8_t*>(elf.constData()),
                                        static_cast<uintptr_t>(elf.size()), &ammId)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_program_id_from_elf failed";
        out[QStringLiteral("error")] = QStringLiteral("bad_program_bin");
        return out;
    }

    // 3. Read + decode the AMM config account to discover the TWAP oracle
    // program id (needed for the current-tick PDA below). No data means the
    // AMM hasn't been initialized on this sequencer yet.
    uint8_t configPda[32];
    amm_client_config_pda(&ammId, &configPda);
    const QString configHex = bytes32ToHex(configPda);

    // Debug: log every derived account (base58 + hex) and every on-chain read so
    // a failed resolve can be traced directly against `spel inspect`.
    // account_id_to_base58 is a LOCAL encode (safe even when on-chain reads
    // time out).
    auto b58 = [this](const QString& hex) {
        const QString s = m_logos->logos_execution_zone.account_id_to_base58(hex);
        return s.isEmpty() ? hex : QStringLiteral("%1 (hex %2)").arg(s, hex);
    };
    AMM_DBG() << "[amm-debug] resolvePool: defA=" << b58(defAHex)
                         << "defB=" << b58(defBHex);
    AMM_DBG() << "[amm-debug] resolvePool: reading config account" << b58(configHex);

    const QString configJson = m_logos->logos_execution_zone.get_account_public(configHex);
    AMM_DBG() << "[amm-debug] resolvePool: config get_account_public ->"
                         << configJson.size() << "chars; raw:" << configJson.left(400);
    const QString configDataHex = accountDataHex(configJson);
    if (configDataHex.isEmpty()) {
        qWarning() << "AmmUiBackend::resolvePool: AMM config read returned no data"
                   << "(a failed/timed-out RPC or an uninitialized account)";
        out[QStringLiteral("error")] = QStringLiteral("amm_not_initialized");
        return out;
    }

    const QByteArray configBytes = QByteArray::fromHex(configDataHex.toUtf8());
    FfiConfigView configView{};
    if (!amm_client_decode_config(reinterpret_cast<const uint8_t*>(configBytes.constData()),
                                  static_cast<uintptr_t>(configBytes.size()), &configView)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_decode_config failed";
        out[QStringLiteral("error")] = QStringLiteral("bad_config");
        return out;
    }

    // 4. Derive the pool PDA and read + decode its account. No data means
    // there's no pool (or no liquidity) for this token pair yet.
    uint8_t defA[32];
    uint8_t defB[32];
    if (!hexToBytes32(defAHex, defA) || !hexToBytes32(defBHex, defB)) {
        qWarning() << "AmmUiBackend::resolvePool: invalid defAHex/defBHex";
        out[QStringLiteral("error")] = QStringLiteral("bad_config");
        return out;
    }

    uint8_t poolPda[32];
    amm_client_pool_pda(&ammId, &defA, &defB, &poolPda);
    const QString poolHex = bytes32ToHex(poolPda);

    AMM_DBG() << "[amm-debug] resolvePool: reading pool account" << b58(poolHex);
    const QString poolJson = m_logos->logos_execution_zone.get_account_public(poolHex);
    AMM_DBG() << "[amm-debug] resolvePool: pool get_account_public ->"
                         << poolJson.size() << "chars; raw:" << poolJson.left(400);
    const QString poolDataHex = accountDataHex(poolJson);
    if (poolDataHex.isEmpty()) {
        // Not a warning: this is the normal "no liquidity yet" state.
        out[QStringLiteral("error")] = QStringLiteral("no_pool");
        return out;
    }

    const QByteArray poolBytes = QByteArray::fromHex(poolDataHex.toUtf8());
    FfiPoolView poolView{};
    if (!amm_client_decode_pool(reinterpret_cast<const uint8_t*>(poolBytes.constData()),
                                static_cast<uintptr_t>(poolBytes.size()), &poolView)) {
        qWarning() << "AmmUiBackend::resolvePool: amm_client_decode_pool failed";
        out[QStringLiteral("error")] = QStringLiteral("bad_config");
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

    AMM_DBG() << "[amm-debug] resolvePool: RESOLVED"
                         << "pool=" << b58(poolHex)
                         << "poolDefA=" << b58(out[QStringLiteral("defAHex")].toString())
                         << "poolDefB=" << b58(out[QStringLiteral("defBHex")].toString())
                         << "vaultA=" << b58(out[QStringLiteral("vaultAHex")].toString())
                         << "vaultB=" << b58(out[QStringLiteral("vaultBHex")].toString())
                         << "currentTick=" << b58(out[QStringLiteral("currentTickHex")].toString())
                         << "reserveA=" << out[QStringLiteral("reserveA")].toString()
                         << "reserveB=" << out[QStringLiteral("reserveB")].toString()
                         << "feeBps=" << out[QStringLiteral("feeBps")].toInt();
    return out;
}

QString AmmUiBackend::swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                      QString userOutputHoldingHex, QString amountInDecimal,
                                      QString minOutDecimal, QString deadlineDecimal)
{
    AMM_DBG() << "[amm-debug] swapExactInput: ARGS"
                         << "defA=" << defAHex << "defB=" << defBHex
                         << "userInputHolding=" << userInputHoldingHex
                         << "userOutputHolding=" << userOutputHoldingHex
                         << "amountIn=" << amountInDecimal << "minOut=" << minOutDecimal
                         << "deadline=" << deadlineDecimal;

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
    const QByteArray elf = loadAmmElf();
    if (elf.isEmpty()) {
        qWarning() << "AmmUiBackend::swapExactInput: failed to load AMM_PROGRAM_BIN";
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

    // Debug: dump the exact accounts/signers we submit (base58 for `spel`
    // comparison), plus the instruction/elf sizes.
    auto b58s = [this](const QString& hex) {
        const QString s = m_logos->logos_execution_zone.account_id_to_base58(hex);
        return s.isEmpty() ? hex : QStringLiteral("%1 (hex %2)").arg(s, hex);
    };
    static const char* const kSlot[] = { "config",       "pool",
                                         "vault_a",      "vault_b",
                                         "user_input_holding", "user_output_holding",
                                         "current_tick", "clock" };
    AMM_DBG() << "[amm-debug] swapExactInput: SUBMIT"
                         << "instruction_words=" << static_cast<int>(instruction.size())
                         << "elf_bytes=" << elf.size();
    for (int i = 0; i < accounts.size(); ++i) {
        AMM_DBG() << "[amm-debug]   account[" << i << "]"
                             << (i < 8 ? kSlot[i] : "?") << "=" << b58s(accounts[i])
                             << "signer=" << (i < signers.size() && signers[i].toBool());
    }

    // 6. Submit through the wallet module's generic public-transaction entry
    // point. The module API takes `instruction` (as bytes — see the note atop
    // this file) plus the program's id as hex, not the raw ELF: the program is
    // already deployed and referenced by id. There is no program_dependencies
    // arg — the sequencer resolves the AMM's chained token/twap calls on-chain.
    // Serialize the u32 instruction words to bytes explicitly as little-endian,
    // matching the protocol's LE-u32 wire format. A reinterpret_cast of the
    // in-memory vector would be host-endian-dependent and byte-swap on a
    // big-endian host, making the guest decode a different instruction.
    QByteArray instructionBytes;
    instructionBytes.reserve(static_cast<int>(instruction.size() * sizeof(uint32_t)));
    for (const uint32_t w : instruction) {
        instructionBytes.append(static_cast<char>(w & 0xFF));
        instructionBytes.append(static_cast<char>((w >> 8) & 0xFF));
        instructionBytes.append(static_cast<char>((w >> 16) & 0xFF));
        instructionBytes.append(static_cast<char>((w >> 24) & 0xFF));
    }

    // Derive the deployed AMM program id from the ELF and hex-encode it as the
    // canonical 32-byte hex (little-endian per u32 word — matches `spel
    // program-id` and the on-chain config's *_program_id fields).
    ProgramId ammId;
    if (!amm_client_program_id_from_elf(reinterpret_cast<const uint8_t*>(elf.constData()),
                                        static_cast<uintptr_t>(elf.size()), &ammId)) {
        qWarning() << "AmmUiBackend::swapExactInput: amm_client_program_id_from_elf failed";
        return QString();
    }
    QByteArray programIdBytes;
    programIdBytes.reserve(32);
    for (int i = 0; i < 8; ++i) {
        const uint32_t w = ammId[i];
        programIdBytes.append(static_cast<char>(w & 0xff));
        programIdBytes.append(static_cast<char>((w >> 8) & 0xff));
        programIdBytes.append(static_cast<char>((w >> 16) & 0xff));
        programIdBytes.append(static_cast<char>((w >> 24) & 0xff));
    }
    const QString programIdHex = QString::fromLatin1(programIdBytes.toHex());
    AMM_DBG() << "[amm-debug] swapExactInput: program_id_hex=" << programIdHex;

    // `instruction` is a QVariant (bstr); `program_id_hex` is a plain QString
    // (tstr) in the generated proxy — pass it directly, not QVariant-wrapped.
    const QString resultJson = m_logos->logos_execution_zone.send_generic_public_transaction(
        accounts, signers, QVariant::fromValue(instructionBytes), programIdHex);
    AMM_DBG() << "[amm-debug] swapExactInput: send_generic_public_transaction ->"
                         << resultJson.size() << "chars; raw:" << resultJson.left(600);

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
