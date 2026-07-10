#include "AmmUiBackend.h"

#include <QClipboard>
#include <QCoreApplication>
#include <QCryptographicHash>
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

#include <algorithm>
#include <cmath>

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

    QString stableId(const QString& key)
    {
        const QByteArray digest =
            QCryptographicHash::hash(key.toUtf8(), QCryptographicHash::Sha256).toHex();
        return QString::fromLatin1(digest);
    }

    QString shortId(const QString& id)
    {
        if (id.length() <= 14)
            return id;
        return id.left(6) + QStringLiteral("...") + id.right(4);
    }

    double parsePositiveAmount(QString value)
    {
        value.remove(QLatin1Char(','));
        bool ok = false;
        const double parsed = value.toDouble(&ok);
        return ok && std::isfinite(parsed) && parsed > 0.0 ? parsed : 0.0;
    }

    QString formatAmount(double amount)
    {
        amount = std::max(0.0, amount);
        const int decimals = amount >= 1000.0 ? 2 : amount >= 1.0 ? 4 : 6;
        QString text = QString::number(amount, 'f', decimals);
        while (text.contains(QLatin1Char('.')) && text.endsWith(QLatin1Char('0')))
            text.chop(1);
        if (text.endsWith(QLatin1Char('.')))
            text.chop(1);
        return text;
    }

    QString formatTokenAmount(double amount, const QString& symbol)
    {
        return QStringLiteral("%1 %2").arg(formatAmount(amount), symbol);
    }

    QVariantMap amountValue(double amount, const QString& symbol)
    {
        QVariantMap value;
        value.insert(QStringLiteral("value"), amount);
        value.insert(QStringLiteral("input"), formatAmount(amount));
        value.insert(QStringLiteral("display"), formatTokenAmount(amount, symbol));
        value.insert(QStringLiteral("symbol"), symbol);
        return value;
    }

    QString feeLabel(int bps)
    {
        if (bps == 1)
            return QStringLiteral("0.01%");
        if (bps == 5)
            return QStringLiteral("0.05%");
        if (bps == 30)
            return QStringLiteral("0.30%");
        if (bps == 100)
            return QStringLiteral("1.00%");
        return QStringLiteral("%1 bps").arg(bps);
    }

    bool isSupportedFeeTier(int bps)
    {
        return bps == 1 || bps == 5 || bps == 30 || bps == 100;
    }

    QVariantList feeTiers()
    {
        QVariantList tiers;
        for (const int bps : {1, 5, 25, 30, 100}) {
            QVariantMap tier;
            tier.insert(QStringLiteral("bps"), bps);
            tier.insert(QStringLiteral("label"), feeLabel(bps));
            tier.insert(QStringLiteral("supported"), isSupportedFeeTier(bps));
            tiers.append(tier);
        }
        return tiers;
    }

    double devnetBalance(const QString& symbol)
    {
        if (symbol == QStringLiteral("USDC"))
            return 12450.0;
        if (symbol == QStringLiteral("LOGOS"))
            return 850000.0;
        if (symbol == QStringLiteral("WETH"))
            return 3.25;
        return 0.0;
    }

    QString accentForSymbol(const QString& symbol)
    {
        if (symbol == QStringLiteral("USDC"))
            return QStringLiteral("#2E7CF6");
        if (symbol == QStringLiteral("LOGOS"))
            return QStringLiteral("#F26A21");
        if (symbol == QStringLiteral("WETH"))
            return QStringLiteral("#B7C2D8");
        return QStringLiteral("#343434");
    }

    QVariantMap devnetHolding(const QString& owner, const QString& symbol, const QString& name)
    {
        const double balance = devnetBalance(symbol);
        const QString definitionId = stableId(QStringLiteral("devnet:token-definition:%1").arg(symbol));
        QVariantMap holding;
        holding.insert(QStringLiteral("symbol"), symbol);
        holding.insert(QStringLiteral("name"), name);
        holding.insert(QStringLiteral("definitionId"), definitionId);
        holding.insert(QStringLiteral("holdingId"),
                       stableId(QStringLiteral("devnet:token-holding:%1:%2").arg(owner, symbol)));
        holding.insert(QStringLiteral("balance"), balance);
        holding.insert(QStringLiteral("balanceText"), formatTokenAmount(balance, symbol));
        holding.insert(QStringLiteral("accent"), accentForSymbol(symbol));
        return holding;
    }

    QVariantList devnetHoldings(const QString& owner)
    {
        if (owner.isEmpty())
            return {};

        QVariantList holdings;
        holdings.append(devnetHolding(owner, QStringLiteral("USDC"), QStringLiteral("USD Coin")));
        holdings.append(devnetHolding(owner, QStringLiteral("LOGOS"), QStringLiteral("Logos")));
        holdings.append(devnetHolding(owner, QStringLiteral("WETH"), QStringLiteral("Wrapped Ether")));
        return holdings;
    }

    QVariantMap holdingBySymbol(const QVariantList& holdings, const QString& symbol)
    {
        for (const QVariant& item : holdings) {
            const QVariantMap holding = item.toMap();
            if (holding.value(QStringLiteral("symbol")).toString() == symbol)
                return holding;
        }
        return {};
    }

    QString unorderedPairKey(const QString& symbolA, const QString& symbolB)
    {
        return symbolA < symbolB
            ? QStringLiteral("%1/%2").arg(symbolA, symbolB)
            : QStringLiteral("%1/%2").arg(symbolB, symbolA);
    }

    double activeRatio(const QString& symbolA, const QString& symbolB)
    {
        if (symbolA == QStringLiteral("USDC") && symbolB == QStringLiteral("LOGOS"))
            return 8.0;
        if (symbolA == QStringLiteral("LOGOS") && symbolB == QStringLiteral("USDC"))
            return 0.125;
        return 1.0;
    }

    double defaultInitialPrice(const QString& symbolA, const QString& symbolB)
    {
        if (symbolA == QStringLiteral("USDC") && symbolB == QStringLiteral("WETH"))
            return 2500.0;
        if (symbolA == QStringLiteral("WETH") && symbolB == QStringLiteral("USDC"))
            return 0.0004;
        return 1.0;
    }

    QVariantMap poolContext(const QString& symbolA, const QString& symbolB)
    {
        QVariantMap context;
        context.insert(QStringLiteral("poolStatus"), QStringLiteral("unavailable_pool"));
        context.insert(QStringLiteral("statusLabel"), QStringLiteral("Unavailable"));
        context.insert(QStringLiteral("detail"), QStringLiteral("Choose two different assets from the active account."));
        context.insert(QStringLiteral("instruction"), QString());
        context.insert(QStringLiteral("storedFeeBps"), 0);
        context.insert(QStringLiteral("poolId"), QString());
        context.insert(QStringLiteral("priceText"), QString());
        context.insert(QStringLiteral("reserveText"), QString());

        if (symbolA.isEmpty() || symbolB.isEmpty() || symbolA == symbolB)
            return context;

        const QString pairKey = unorderedPairKey(symbolA, symbolB);
        const QString poolId = stableId(QStringLiteral("devnet:amm-pool:%1").arg(pairKey));
        context.insert(QStringLiteral("poolId"), poolId);

        if (pairKey == QStringLiteral("LOGOS/USDC")) {
            context.insert(QStringLiteral("poolStatus"), QStringLiteral("active_pool"));
            context.insert(QStringLiteral("statusLabel"), QStringLiteral("Active pool"));
            context.insert(QStringLiteral("detail"), QStringLiteral("Deposits are quoted against the existing pool ratio. Nonmatching fee tiers are locked."));
            context.insert(QStringLiteral("instruction"), QStringLiteral("add_liquidity"));
            context.insert(QStringLiteral("storedFeeBps"), 30);
            context.insert(QStringLiteral("priceText"),
                           symbolA == QStringLiteral("USDC")
                               ? QStringLiteral("1 USDC = 8 LOGOS")
                               : QStringLiteral("1 LOGOS = 0.125 USDC"));
            context.insert(QStringLiteral("reserveText"),
                           symbolA == QStringLiteral("USDC")
                               ? QStringLiteral("1,250,000 USDC / 10,000,000 LOGOS")
                               : QStringLiteral("10,000,000 LOGOS / 1,250,000 USDC"));
            return context;
        }

        if (pairKey == QStringLiteral("USDC/WETH")) {
            context.insert(QStringLiteral("poolStatus"), QStringLiteral("missing_pool"));
            context.insert(QStringLiteral("statusLabel"), QStringLiteral("Missing pool"));
            context.insert(QStringLiteral("detail"), QStringLiteral("Set the initial price first, then scale both deposits together."));
            context.insert(QStringLiteral("instruction"), QStringLiteral("new_definition"));
            context.insert(QStringLiteral("priceText"), QStringLiteral("No reserves yet"));
            context.insert(QStringLiteral("reserveText"), QStringLiteral("Pool account is empty"));
            return context;
        }

        context.insert(QStringLiteral("detail"), QStringLiteral("A pool account exists, but it cannot be quoted safely for this devnet state."));
        context.insert(QStringLiteral("priceText"), QStringLiteral("Quote disabled"));
        context.insert(QStringLiteral("reserveText"), QStringLiteral("Unsupported stored pool state"));
        return context;
    }

    QString quoteHash(const QVariantMap& request)
    {
        const QStringList parts = {
            request.value(QStringLiteral("tokenA")).toString(),
            request.value(QStringLiteral("tokenB")).toString(),
            QString::number(request.value(QStringLiteral("feeBps")).toInt()),
            request.value(QStringLiteral("editedSide")).toString(),
            request.value(QStringLiteral("amountA")).toString(),
            request.value(QStringLiteral("amountB")).toString(),
            request.value(QStringLiteral("initialPrice")).toString(),
            QString::number(request.value(QStringLiteral("depositScale")).toInt()),
            QString::number(request.value(QStringLiteral("slippageBps")).toInt()),
        };
        const QByteArray digest =
            QCryptographicHash::hash(parts.join(QLatin1Char('|')).toUtf8(),
                                     QCryptographicHash::Sha256).toHex();
        return QStringLiteral("sha256-%1").arg(QString::fromLatin1(digest));
    }

    QVariantMap accountChange(const QString& role, const QString& id, const QString& action)
    {
        QVariantMap change;
        change.insert(QStringLiteral("role"), role);
        change.insert(QStringLiteral("id"), id);
        change.insert(QStringLiteral("action"), action);
        return change;
    }

    QVariantList accountChanges(const QVariantMap& request, const QVariantMap& context)
    {
        const QString tokenA = request.value(QStringLiteral("tokenA")).toString();
        const QString tokenB = request.value(QStringLiteral("tokenB")).toString();
        const QString poolId = context.value(QStringLiteral("poolId")).toString();
        const bool missingPool =
            context.value(QStringLiteral("poolStatus")).toString() == QStringLiteral("missing_pool");

        QVariantList changes;
        changes.append(accountChange(QStringLiteral("Config"),
                                     stableId(QStringLiteral("devnet:amm-config")),
                                     QStringLiteral("Read")));
        changes.append(accountChange(QStringLiteral("Pool"),
                                     poolId,
                                     missingPool ? QStringLiteral("Create") : QStringLiteral("Update")));
        changes.append(accountChange(QStringLiteral("Vault A"),
                                     stableId(QStringLiteral("devnet:vault:%1:%2").arg(poolId, tokenA)),
                                     missingPool ? QStringLiteral("Update or create") : QStringLiteral("Update")));
        changes.append(accountChange(QStringLiteral("Vault B"),
                                     stableId(QStringLiteral("devnet:vault:%1:%2").arg(poolId, tokenB)),
                                     missingPool ? QStringLiteral("Update or create") : QStringLiteral("Update")));
        if (missingPool) {
            changes.append(accountChange(QStringLiteral("LP definition"),
                                         stableId(QStringLiteral("devnet:lp-definition:%1").arg(poolId)),
                                         QStringLiteral("Create")));
            changes.append(accountChange(QStringLiteral("LP lock holding"),
                                         stableId(QStringLiteral("devnet:lp-lock:%1").arg(poolId)),
                                         QStringLiteral("Create")));
        }
        changes.append(accountChange(QStringLiteral("User LP holding"),
                                     stableId(QStringLiteral("devnet:user-lp:%1").arg(poolId)),
                                     QStringLiteral("Update or create")));
        changes.append(accountChange(QStringLiteral("Current tick"),
                                     stableId(QStringLiteral("devnet:current-tick:%1").arg(poolId)),
                                     missingPool ? QStringLiteral("Create") : QStringLiteral("Update")));
        changes.append(accountChange(QStringLiteral("Clock"),
                                     stableId(QStringLiteral("devnet:clock:canonical")),
                                     QStringLiteral("Read")));
        return changes;
    }

    QVariantMap baseQuote(const QVariantMap& context, const QVariantMap& request)
    {
        QVariantMap quote;
        quote.insert(QStringLiteral("poolStatus"), context.value(QStringLiteral("poolStatus")));
        quote.insert(QStringLiteral("statusLabel"), context.value(QStringLiteral("statusLabel")));
        quote.insert(QStringLiteral("statusDetail"), context.value(QStringLiteral("detail")));
        quote.insert(QStringLiteral("instruction"), context.value(QStringLiteral("instruction")));
        quote.insert(QStringLiteral("storedFeeBps"), context.value(QStringLiteral("storedFeeBps")));
        quote.insert(QStringLiteral("feeBps"), request.value(QStringLiteral("feeBps")).toInt());
        quote.insert(QStringLiteral("feeLabel"), feeLabel(request.value(QStringLiteral("feeBps")).toInt()));
        quote.insert(QStringLiteral("quoteHash"), quoteHash(request));

        QVariantMap pool;
        pool.insert(QStringLiteral("id"), context.value(QStringLiteral("poolId")));
        pool.insert(QStringLiteral("priceText"), context.value(QStringLiteral("priceText")));
        pool.insert(QStringLiteral("reserveText"), context.value(QStringLiteral("reserveText")));
        quote.insert(QStringLiteral("pool"), pool);
        return quote;
    }

    QVariantMap quoteOk(const QVariantMap& context,
                        const QVariantMap& request,
                        double maxA,
                        double maxB,
                        double actualA,
                        double actualB,
                        double expectedLp,
                        double minimumLp,
                        double lockedLp,
                        const QVariantMap& position)
    {
        QVariantMap quote = baseQuote(context, request);
        const QString tokenA = request.value(QStringLiteral("tokenA")).toString();
        const QString tokenB = request.value(QStringLiteral("tokenB")).toString();
        quote.insert(QStringLiteral("status"), QStringLiteral("ok"));
        quote.insert(QStringLiteral("error"), QString());

        QVariantMap deposit;
        deposit.insert(QStringLiteral("maxA"), amountValue(maxA, tokenA));
        deposit.insert(QStringLiteral("maxB"), amountValue(maxB, tokenB));
        deposit.insert(QStringLiteral("actualA"), amountValue(actualA, tokenA));
        deposit.insert(QStringLiteral("actualB"), amountValue(actualB, tokenB));
        quote.insert(QStringLiteral("deposit"), deposit);

        QVariantMap lp;
        lp.insert(QStringLiteral("expected"), amountValue(expectedLp, QStringLiteral("LP")));
        lp.insert(QStringLiteral("minimum"), amountValue(minimumLp, QStringLiteral("LP")));
        lp.insert(QStringLiteral("locked"), amountValue(lockedLp, QStringLiteral("LP")));
        quote.insert(QStringLiteral("lp"), lp);

        quote.insert(QStringLiteral("position"), position);
        quote.insert(QStringLiteral("accountChanges"), accountChanges(request, context));

        QVariantMap transaction;
        transaction.insert(QStringLiteral("instruction"), context.value(QStringLiteral("instruction")));
        transaction.insert(QStringLiteral("ready"), false);
        transaction.insert(QStringLiteral("reason"), QStringLiteral("Submission requires real token holding account ids from wallet discovery."));
        quote.insert(QStringLiteral("transaction"), transaction);
        return quote;
    }

    QVariantMap quoteError(const QVariantMap& context, const QVariantMap& request, const QString& errorText)
    {
        QVariantMap quote = baseQuote(context, request);
        const QString tokenA = request.value(QStringLiteral("tokenA")).toString();
        const QString tokenB = request.value(QStringLiteral("tokenB")).toString();
        quote.insert(QStringLiteral("status"), QStringLiteral("error"));
        quote.insert(QStringLiteral("error"), errorText);

        QVariantMap deposit;
        deposit.insert(QStringLiteral("maxA"), amountValue(0, tokenA));
        deposit.insert(QStringLiteral("maxB"), amountValue(0, tokenB));
        deposit.insert(QStringLiteral("actualA"), amountValue(0, tokenA));
        deposit.insert(QStringLiteral("actualB"), amountValue(0, tokenB));
        quote.insert(QStringLiteral("deposit"), deposit);

        QVariantMap lp;
        lp.insert(QStringLiteral("expected"), amountValue(0, QStringLiteral("LP")));
        lp.insert(QStringLiteral("minimum"), amountValue(0, QStringLiteral("LP")));
        const bool missingPool =
            context.value(QStringLiteral("poolStatus")).toString() == QStringLiteral("missing_pool");
        lp.insert(QStringLiteral("locked"), amountValue(missingPool ? 1000.0 : 0.0, QStringLiteral("LP")));
        quote.insert(QStringLiteral("lp"), lp);

        QVariantMap position;
        position.insert(QStringLiteral("userLp"), QStringLiteral("0 LP"));
        position.insert(QStringLiteral("share"), QStringLiteral("-"));
        position.insert(QStringLiteral("ownedA"), formatTokenAmount(0, tokenA));
        position.insert(QStringLiteral("ownedB"), formatTokenAmount(0, tokenB));
        quote.insert(QStringLiteral("position"), position);
        quote.insert(QStringLiteral("accountChanges"), QVariantList());
        return quote;
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
    setNewPositionContext(buildNewPositionContext());

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
        refreshNewPositionContext();
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
        refreshNewPositionContext();
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
    refreshNewPositionContext();
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
        refreshNewPositionContext();
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
    refreshNewPositionContext();
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
    refreshNewPositionContext();
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
    refreshNewPositionContext();
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
    refreshNewPositionContext();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_logos->logos_execution_zone.get_balance(accountIdHex, isPublic);
}

QString AmmUiBackend::activeAccountAddress() const
{
    if (m_accountModel->count() <= 0)
        return {};
    const QModelIndex idx = m_accountModel->index(0, 0);
    return m_accountModel->data(idx, AccountModel::AddressRole).toString();
}

QVariantMap AmmUiBackend::buildNewPositionContext() const
{
    QVariantMap context;
    context.insert(QStringLiteral("minimumLiquidity"), 1000.0);
    context.insert(QStringLiteral("feeTiers"), feeTiers());

    QVariantMap network;
    network.insert(QStringLiteral("id"), QStringLiteral("devnet"));
    network.insert(QStringLiteral("name"), QStringLiteral("Local devnet"));
    network.insert(QStringLiteral("selector"), QStringLiteral("temporary-file"));
    context.insert(QStringLiteral("network"), network);

    QVariantMap programIds;
    programIds.insert(QStringLiteral("amm"), stableId(QStringLiteral("devnet:program:amm")));
    programIds.insert(QStringLiteral("token"), stableId(QStringLiteral("devnet:program:token")));
    programIds.insert(QStringLiteral("twapOracle"), stableId(QStringLiteral("devnet:program:twap-oracle")));
    context.insert(QStringLiteral("programIds"), programIds);

    QVariantMap activeAccount;
    const bool hasAccount = isWalletOpen() && m_accountModel->count() > 0;
    if (hasAccount) {
        const QModelIndex idx = m_accountModel->index(0, 0);
        const QString address = m_accountModel->data(idx, AccountModel::AddressRole).toString();
        activeAccount.insert(QStringLiteral("name"), m_accountModel->data(idx, AccountModel::NameRole).toString());
        activeAccount.insert(QStringLiteral("address"), address);
        activeAccount.insert(QStringLiteral("display"), shortId(address));
        activeAccount.insert(QStringLiteral("isPublic"), m_accountModel->data(idx, AccountModel::IsPublicRole).toBool());
        activeAccount.insert(QStringLiteral("balance"), m_accountModel->data(idx, AccountModel::BalanceRole).toString());
        context.insert(QStringLiteral("holdings"), devnetHoldings(address));
        context.insert(QStringLiteral("status"), QStringLiteral("ready"));
        context.insert(QStringLiteral("statusDetail"), QStringLiteral("Devnet token holdings resolved for the active wallet account."));
    } else {
        activeAccount.insert(QStringLiteral("name"), QString());
        activeAccount.insert(QStringLiteral("address"), QString());
        activeAccount.insert(QStringLiteral("display"), QStringLiteral("Not connected"));
        activeAccount.insert(QStringLiteral("isPublic"), true);
        activeAccount.insert(QStringLiteral("balance"), QString());
        context.insert(QStringLiteral("holdings"), QVariantList());
        context.insert(QStringLiteral("status"), isWalletOpen() ? QStringLiteral("no_account") : QStringLiteral("no_wallet"));
        context.insert(QStringLiteral("statusDetail"),
                       isWalletOpen()
                           ? QStringLiteral("Create an account before opening a liquidity position.")
                           : QStringLiteral("Connect a wallet before opening a liquidity position."));
    }
    context.insert(QStringLiteral("activeAccount"), activeAccount);
    context.insert(QStringLiteral("activeAccountDisplay"), activeAccount.value(QStringLiteral("display")));
    return context;
}

QVariant AmmUiBackend::refreshNewPositionContext()
{
    const QVariantMap context = buildNewPositionContext();
    setNewPositionContext(context);
    return context;
}

QVariantMap AmmUiBackend::quoteNewPositionMap(const QVariantMap& request) const
{
    const QString tokenA = request.value(QStringLiteral("tokenA")).toString();
    const QString tokenB = request.value(QStringLiteral("tokenB")).toString();
    const QVariantMap context = poolContext(tokenA, tokenB);

    if (!isWalletOpen())
        return quoteError(context, request, tr("Connect a wallet to preview this position."));

    const QString owner = activeAccountAddress();
    if (owner.isEmpty())
        return quoteError(context, request, tr("Create an account before previewing this position."));

    const QVariantList holdings = devnetHoldings(owner);
    const QVariantMap holdingA = holdingBySymbol(holdings, tokenA);
    const QVariantMap holdingB = holdingBySymbol(holdings, tokenB);
    if (holdingA.isEmpty() || holdingB.isEmpty())
        return quoteError(context, request, tr("Choose two token holdings from the active account."));

    const int feeBps = request.value(QStringLiteral("feeBps")).toInt();
    if (!isSupportedFeeTier(feeBps))
        return quoteError(context, request, tr("Fee tier is not supported by the AMM program."));

    const QString poolStatus = context.value(QStringLiteral("poolStatus")).toString();
    const int storedFeeBps = context.value(QStringLiteral("storedFeeBps")).toInt();
    if (poolStatus == QStringLiteral("active_pool") && feeBps != storedFeeBps)
        return quoteError(context, request,
                          tr("Existing pool uses %1.").arg(feeLabel(storedFeeBps)));

    const int slippageBps =
        std::max(1, std::min(5000, request.value(QStringLiteral("slippageBps")).toInt()));

    if (poolStatus == QStringLiteral("active_pool")) {
        const double inputA = parsePositiveAmount(request.value(QStringLiteral("amountA")).toString());
        const double inputB = parsePositiveAmount(request.value(QStringLiteral("amountB")).toString());
        const bool editA = request.value(QStringLiteral("editedSide")).toString() != QStringLiteral("B");
        const double ratio = activeRatio(tokenA, tokenB);
        const double amountA = editA ? inputA : inputB / ratio;
        const double amountB = editA ? inputA * ratio : inputB;
        const double expectedLp = std::floor(std::min(amountA * 5.5, amountB * 0.69));
        const double minimumLp = std::floor(expectedLp * (10000 - slippageBps) / 10000.0);

        if (inputA <= 0.0 && inputB <= 0.0)
            return quoteError(context, request, tr("Enter a deposit amount to preview LP output."));
        if (amountA > holdingA.value(QStringLiteral("balance")).toDouble())
            return quoteError(context, request, tr("Insufficient %1 balance.").arg(tokenA));
        if (amountB > holdingB.value(QStringLiteral("balance")).toDouble())
            return quoteError(context, request, tr("Insufficient %1 balance.").arg(tokenB));
        if (minimumLp <= 0.0)
            return quoteError(context, request, tr("LP minimum rounds to zero. Increase deposit amount."));

        QVariantMap position;
        position.insert(QStringLiteral("userLp"), QStringLiteral("148320 LP"));
        position.insert(QStringLiteral("share"), QStringLiteral("1.18%"));
        position.insert(QStringLiteral("ownedA"), formatTokenAmount(tokenA == QStringLiteral("USDC") ? 14750.0 : 118000.0, tokenA));
        position.insert(QStringLiteral("ownedB"), formatTokenAmount(tokenB == QStringLiteral("LOGOS") ? 118000.0 : 14750.0, tokenB));
        return quoteOk(context, request, amountA, amountB, amountA, amountB, expectedLp, minimumLp, 0.0, position);
    }

    if (poolStatus == QStringLiteral("missing_pool")) {
        const double price =
            parsePositiveAmount(request.value(QStringLiteral("initialPrice")).toString()) > 0.0
                ? parsePositiveAmount(request.value(QStringLiteral("initialPrice")).toString())
                : defaultInitialPrice(tokenA, tokenB);
        const double scale = std::max(1, request.value(QStringLiteral("depositScale")).toInt());
        const double amountA = price >= 1.0 ? price * scale : scale;
        const double amountB = price >= 1.0 ? scale : scale / price;
        const double expectedLp = std::floor(std::sqrt(amountA * amountB) * 48.0);
        const double userLp = std::max(0.0, expectedLp - 1000.0);
        const double minimumLp = std::floor(userLp * (10000 - slippageBps) / 10000.0);

        if (amountA > holdingA.value(QStringLiteral("balance")).toDouble())
            return quoteError(context, request, tr("Initial deposit exceeds %1 balance.").arg(tokenA));
        if (amountB > holdingB.value(QStringLiteral("balance")).toDouble())
            return quoteError(context, request, tr("Initial deposit exceeds %1 balance.").arg(tokenB));
        if (userLp <= 0.0)
            return quoteError(context, request, tr("Deposit must mint more than the locked minimum liquidity."));

        QVariantMap position;
        position.insert(QStringLiteral("userLp"), QStringLiteral("0 LP"));
        position.insert(QStringLiteral("share"), QStringLiteral("New pool"));
        position.insert(QStringLiteral("ownedA"), formatTokenAmount(0, tokenA));
        position.insert(QStringLiteral("ownedB"), formatTokenAmount(0, tokenB));

        QVariantMap quote =
            quoteOk(context, request, amountA, amountB, amountA, amountB, userLp, minimumLp, 1000.0, position);
        QVariantMap pool = quote.value(QStringLiteral("pool")).toMap();
        pool.insert(QStringLiteral("priceText"),
                    QStringLiteral("1 %1 = %2 %3")
                        .arg(tokenB, formatAmount(price), tokenA));
        quote.insert(QStringLiteral("pool"), pool);
        return quote;
    }

    return quoteError(context, request, context.value(QStringLiteral("detail")).toString());
}

QVariant AmmUiBackend::quoteNewPosition(QVariant request)
{
    return quoteNewPositionMap(request.toMap());
}

QVariant AmmUiBackend::submitNewPosition(QVariant request, QString quoteHash)
{
    const QVariantMap quote = quoteNewPositionMap(request.toMap());
    if (quote.value(QStringLiteral("status")).toString() != QStringLiteral("ok")) {
        QVariantMap result;
        result.insert(QStringLiteral("status"), QStringLiteral("error"));
        result.insert(QStringLiteral("code"), QStringLiteral("quote_error"));
        result.insert(QStringLiteral("error"), quote.value(QStringLiteral("error")));
        result.insert(QStringLiteral("quote"), quote);
        return result;
    }

    if (quote.value(QStringLiteral("quoteHash")).toString() != quoteHash) {
        QVariantMap result;
        result.insert(QStringLiteral("status"), QStringLiteral("error"));
        result.insert(QStringLiteral("code"), QStringLiteral("quote_changed"));
        result.insert(QStringLiteral("error"), tr("Quote changed. Refresh preview before submitting."));
        result.insert(QStringLiteral("quote"), quote);
        return result;
    }

    const QVariantMap transaction = quote.value(QStringLiteral("transaction")).toMap();
    if (!transaction.value(QStringLiteral("ready")).toBool()) {
        QVariantMap result;
        result.insert(QStringLiteral("status"), QStringLiteral("error"));
        result.insert(QStringLiteral("code"), QStringLiteral("submit_unavailable"));
        result.insert(QStringLiteral("error"), transaction.value(QStringLiteral("reason")).toString());
        result.insert(QStringLiteral("quote"), quote);
        return result;
    }

    QVariantMap result;
    result.insert(QStringLiteral("status"), QStringLiteral("ok"));
    result.insert(QStringLiteral("message"),
                  quote.value(QStringLiteral("instruction")).toString() == QStringLiteral("new_definition")
                      ? tr("Pool creation submitted")
                      : tr("Liquidity deposit submitted"));
    result.insert(QStringLiteral("detail"),
                  QStringLiteral("%1 / %2")
                      .arg(request.toMap().value(QStringLiteral("tokenA")).toString(),
                           request.toMap().value(QStringLiteral("tokenB")).toString()));
    return result;
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
