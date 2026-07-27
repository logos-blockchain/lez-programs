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
#include <QSettings>
#include <QTimer>
#include <QUrl>

#include "AmmClient.h"
#include "LogosWalletProvider.h"
#include "NewPositionRuntime.h"
#include "SwapRuntime.h"
#include "WalletController.h"
#include "logos_api.h"
#include "logos_sdk.h"

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
      m_swap(std::make_unique<SwapRuntime>(m_wallet.get(), m_ammClient.get()))
{
    setWalletStateReady(false);
    setNewPositionContext(m_newPosition->context(
        QVariantMap(), networkSnapshot(), false, false));

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
    setNewPositionContext(m_newPosition->context(
        request, networkSnapshot(), isWalletOpen(), refreshWalletAccounts));
}

QVariantMap AmmUiBackend::quoteNewPosition(QVariantMap request)
{
    return m_newPosition->quote(request, networkSnapshot(), isWalletOpen());
}

QVariantMap AmmUiBackend::submitNewPosition(QVariantMap request, QString quoteHash)
{
    return m_newPosition->submit(
        request, quoteHash, networkSnapshot(), isWalletOpen());
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    const bool walletWasOpen = isWalletOpen();

    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);

    if (walletWasOpen && !state.isWalletOpen)
        m_newPosition->clearWalletAccounts();

    publishNetworkContext();
}

void AmmUiBackend::publishNetworkContext()
{
    setNewPositionContext(m_newPosition->context(
        m_newPositionHints, networkSnapshot(), isWalletOpen(), false));
}

QString AmmUiBackend::ammProgramIdHex()
{
    const QByteArray elf = loadAmmElf();
    if (elf.isEmpty())
        return QString();
    // Hand the deployed program binary to the amm_client program_id op, which
    // decodes it and computes the Image ID — 64-char lowercase hex, little-endian
    // per u32 word (matches `spel program-id` and the on-chain *_program_id fields).
    const AmmClientResult result = m_ammClient->programId(
        QJsonObject { { QStringLiteral("elf"), QString::fromLatin1(elf.toHex()) } });
    if (!result.ok) {
        qWarning() << "AmmUiBackend::ammProgramIdHex: amm_program_id failed";
        return QString();
    }
    return result.value.value(QStringLiteral("programId")).toString();
}

ActiveNetworkSnapshot AmmUiBackend::networkSnapshot()
{
    ActiveNetworkSnapshot snapshot;
    snapshot.id = QStringLiteral("lez");
    // Defer program/token resolution (which reaches the module) until wallet
    // state is resolved; the constructor publishes an initial context before
    // the module is up, and syncWalletState() republishes once it is.
    if (!walletStateReady()) {
        snapshot.status = QStringLiteral("loading");
        return snapshot;
    }
    // Resolve the AMM deployment id ($AMM_PROGRAM_BIN) and configured token set
    // ($TOKENS_CONFIG) ONCE and cache — they're fixed for the process lifetime.
    // networkSnapshot() runs on the quote hot path and from inside runtime reply
    // callbacks, and tokenList() makes remote base58 conversions; recomputing each
    // call reenters the module connection and hangs the reply.
    if (!m_networkResolved) {
        m_ammProgramIdCache = ammProgramIdHex();
        m_tokenIdsCache.clear();
        // Configured token set = the TOKENS_CONFIG definition ids, the same source
        // the Swap view's token picker uses (tokenList normalizes them to hex).
        const QVariantList tokens = tokenList();
        for (const QVariant& entry : tokens) {
            const QString id = entry.toMap().value(QStringLiteral("definitionId")).toString();
            if (!id.isEmpty())
                m_tokenIdsCache.append(id);
        }
        m_networkResolved = true;
    }
    snapshot.ammProgramId = m_ammProgramIdCache;
    // Bind a quote to this AMM deployment: the program id changes per deployment,
    // so it doubles as the network fingerprint (a quote can't be replayed against
    // a different program). Empty when AMM_PROGRAM_BIN is unset — status gates it.
    snapshot.fingerprint = m_ammProgramIdCache;
    snapshot.tokenIds = m_tokenIdsCache;
    snapshot.status = m_ammProgramIdCache.isEmpty()
        ? QStringLiteral("config_missing")
        : QStringLiteral("ready");
    return snapshot;
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
    return m_swap->resolvePool(defAHex, defBHex, networkSnapshot());
}

QString AmmUiBackend::swapExactInput(QString defAHex, QString defBHex, QString userInputHoldingHex,
                                      QString userOutputHoldingHex, QString amountInDecimal,
                                      QString minOutDecimal, QString deadlineDecimal)
{
    const QString txHash = m_swap->swap(defAHex, defBHex, userInputHoldingHex, userOutputHoldingHex,
                                        amountInDecimal, minOutDecimal, deadlineDecimal,
                                        networkSnapshot(), isWalletOpen());
    if (!txHash.isEmpty())
        refreshBalances();
    return txHash;
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
