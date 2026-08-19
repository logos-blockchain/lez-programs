#include "WalletPortfolioService.h"

#include <QtTest>

#include <utility>

// The service normally links this symbol from wallet-idl-decoder. Every test
// injects a decoder, so keep this target independent from the Rust FFI library.
WalletDecodeResult WalletIdlDecoder::decode(const QByteArray&,
                                            const QVector<WalletAccountRead>&)
{
    return {
        QStringLiteral("error"),
        QStringLiteral("unexpected_default_decoder"),
        {},
    };
}

namespace {
const QString DEFINITION_ID(64, QLatin1Char('a'));
const QString DEFINITION_BASE58 = QStringLiteral("base58-definition");
const QString HOLDING_ID(64, QLatin1Char('b'));
const QString TOKEN_PROGRAM_ID(64, QLatin1Char('c'));
const QString AMM_ACCOUNT_ID(64, QLatin1Char('d'));
const QString AMM_PROGRAM_ID(64, QLatin1Char('e'));

WalletAccountRead read(const QString& accountId,
                       const QString& programOwner,
                       const QString& data)
{
    WalletAccountRead result;
    result.accountId = accountId;
    result.status = QStringLiteral("ok");
    result.programOwner = programOwner;
    result.dataHex = data;
    return result;
}

WalletPortfolioRequest request(const QString& name = QStringLiteral("Test token"))
{
    WalletSnapshot snapshot;
    snapshot.publicAccountReads = {
        read(DEFINITION_ID, TOKEN_PROGRAM_ID, QStringLiteral("definition")),
        read(HOLDING_ID, TOKEN_PROGRAM_ID, QStringLiteral("holding")),
        read(AMM_ACCOUNT_ID, AMM_PROGRAM_ID, QStringLiteral("amm")),
    };
    WalletPortfolioRequest result(snapshot);
    result.tokenDefinitionIds = { DEFINITION_BASE58 };
    result.tokens = { QVariantMap {
        { QStringLiteral("definitionId"), DEFINITION_BASE58 },
        { QStringLiteral("definitionIdHex"), DEFINITION_ID },
        { QStringLiteral("name"), name },
    } };
    result.tokenProgramId = TOKEN_PROGRAM_ID;
    result.tokenIdl = QByteArrayLiteral("token-idl");
    return result;
}

WalletDecodedAccount tokenDefinition()
{
    WalletDecodedAccount account;
    account.id = DEFINITION_ID;
    account.status = QStringLiteral("decoded");
    account.typeName = QStringLiteral("TokenDefinition");
    account.value = QJsonObject {
        { QStringLiteral("Fungible"), QJsonObject {
            { QStringLiteral("name"), QStringLiteral("Test token") },
        } },
    };
    return account;
}

WalletDecodedAccount tokenHolding(bool decoded = true)
{
    WalletDecodedAccount account;
    account.id = HOLDING_ID;
    account.status = decoded ? QStringLiteral("decoded") : QStringLiteral("error");
    account.typeName = QStringLiteral("TokenHolding");
    account.value = QJsonObject {
        { QStringLiteral("Fungible"), QJsonObject {
            { QStringLiteral("definition_id"), QStringLiteral("definition") },
            { QStringLiteral("balance"), QStringLiteral("25") },
        } },
    };
    account.accountIds.insert(QStringLiteral("definition"), DEFINITION_ID);
    return account;
}

WalletDecodedAccount ammAccount()
{
    WalletDecodedAccount account;
    account.id = AMM_ACCOUNT_ID;
    account.status = QStringLiteral("decoded");
    account.typeName = QStringLiteral("Pool");
    account.value = QJsonObject {
        { QStringLiteral("Pool"), QJsonObject {} },
    };
    return account;
}

WalletPortfolioService::Decoder decoder(int* calls, bool failHolding = false)
{
    return [calls, failHolding](const QByteArray&, const QVector<WalletAccountRead>& reads) {
        ++*calls;
        WalletDecodeResult result;
        result.status = QStringLiteral("ok");
        for (const WalletAccountRead& item : reads) {
            if (item.accountId == DEFINITION_ID)
                result.accounts.append(tokenDefinition());
            else if (item.accountId == HOLDING_ID)
                result.accounts.append(tokenHolding(!failHolding));
            else if (item.accountId == AMM_ACCOUNT_ID)
                result.accounts.append(ammAccount());
        }
        return result;
    };
}
}

class WalletPortfolioServiceTest : public QObject {
    Q_OBJECT

private slots:
    void acceptsBase58DefinitionsAndReusesUnchangedDecodes();
    void exposesHoldingDecodeFailureWithoutZeroBalance();
    void exposesUnreadPublicAccountWithoutZeroBalance();
    void exposesUnresolvedDefinitions();
};

void WalletPortfolioServiceTest::acceptsBase58DefinitionsAndReusesUnchangedDecodes()
{
    int decodeCalls = 0;
    WalletPortfolioService service(decoder(&decodeCalls));
    service.registerProgram(AMM_PROGRAM_ID, QStringLiteral("AMM"), QByteArrayLiteral("amm-idl"));

    const WalletPortfolioResult first = service.refresh(request());

    QCOMPARE(first.status, QStringLiteral("ready"));
    QCOMPARE(first.assets.size(), 1);
    const QVariantMap asset = first.assets.first().toMap();
    QCOMPARE(asset.value(QStringLiteral("definitionId")).toString(), DEFINITION_ID);
    QCOMPARE(asset.value(QStringLiteral("displayDefinitionId")).toString(), DEFINITION_BASE58);
    QCOMPARE(asset.value(QStringLiteral("balance")).toString(), QStringLiteral("25"));
    QCOMPARE(first.presentations.size(), 3);
    const int firstDecodeCalls = decodeCalls;
    QVERIFY(firstDecodeCalls > 0);

    const WalletPortfolioResult renamed = service.refresh(request(QStringLiteral("Renamed")));

    QCOMPARE(decodeCalls, firstDecodeCalls);
    QCOMPARE(renamed.status, QStringLiteral("ready"));
    QCOMPARE(renamed.assets.first().toMap().value(QStringLiteral("name")).toString(),
             QStringLiteral("Renamed"));
}

void WalletPortfolioServiceTest::exposesHoldingDecodeFailureWithoutZeroBalance()
{
    int decodeCalls = 0;
    WalletPortfolioService service(decoder(&decodeCalls, true));

    const WalletPortfolioResult result = service.refresh(request());

    QCOMPARE(result.status, QStringLiteral("partial"));
    QCOMPARE(result.error, QStringLiteral("holding_decode_failed"));
    const QVariantMap asset = result.assets.first().toMap();
    QCOMPARE(asset.value(QStringLiteral("status")).toString(), QStringLiteral("unavailable"));
    QVERIFY(asset.value(QStringLiteral("balance")).toString().isEmpty());
}

void WalletPortfolioServiceTest::exposesUnreadPublicAccountWithoutZeroBalance()
{
    int decodeCalls = 0;
    WalletPortfolioService service(decoder(&decodeCalls));
    WalletPortfolioRequest input = request();
    for (WalletAccountRead& account : input.publicAccountReads) {
        if (account.accountId == HOLDING_ID)
            account = WalletAccountRead { HOLDING_ID };
    }

    const WalletPortfolioResult result = service.refresh(input);

    QCOMPARE(result.status, QStringLiteral("partial"));
    QCOMPARE(result.error, QStringLiteral("public_account_read_failed"));
    const QVariantMap asset = result.assets.first().toMap();
    QCOMPARE(asset.value(QStringLiteral("status")).toString(), QStringLiteral("unavailable"));
    QVERIFY(asset.value(QStringLiteral("balance")).toString().isEmpty());
}

void WalletPortfolioServiceTest::exposesUnresolvedDefinitions()
{
    int decodeCalls = 0;
    WalletPortfolioService service(decoder(&decodeCalls));
    WalletPortfolioRequest input = request();
    input.tokens.clear();

    const WalletPortfolioResult result = service.refresh(input);

    QCOMPARE(result.status, QStringLiteral("error"));
    QCOMPARE(result.error, QStringLiteral("definitions_unavailable"));
    QCOMPARE(result.assets.size(), 1);
    QCOMPARE(result.assets.first().toMap().value(QStringLiteral("status")).toString(),
             QStringLiteral("unavailable"));
}

QTEST_GUILESS_MAIN(WalletPortfolioServiceTest)
#include "WalletPortfolioServiceTest.moc"
