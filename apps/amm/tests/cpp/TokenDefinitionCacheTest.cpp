#include "FakeWalletProvider.h"
#include "TokenDefinitionCache.h"

#include <QtTest>

#include <utility>

namespace {
TokenDefinitionCacheKey cacheKey(const QString& fingerprint = QStringLiteral("channel:one"))
{
    return {
        QStringLiteral("devnet"),
        fingerprint,
        QStringLiteral("http://127.0.0.1:8080"),
        {
            QString(64, QLatin1Char('a')),
            QString(64, QLatin1Char('b')),
        },
    };
}

void makeReadsReady(FakeWalletProvider& provider)
{
    provider.readResult.status = QStringLiteral("ok");
    provider.readResult.programOwner = QString(64, QLatin1Char('c'));
    provider.readResult.dataHex = QStringLiteral("00");
}
}

class TokenDefinitionCacheTest : public QObject {
    Q_OBJECT

private slots:
    void reusesCompleteReads();
    void retriesIncompleteReads();
    void separatesNetworkKeys();
    void coalescesInFlightReads();
    void restartsCancelledRead();
    void dropsPendingCallbackOnDestruction();
};

void TokenDefinitionCacheTest::reusesCompleteReads()
{
    FakeWalletProvider provider;
    makeReadsReady(provider);
    TokenDefinitionCache cache(provider);
    const TokenDefinitionCacheKey key = cacheKey();

    QVector<WalletAccountRead> first;
    cache.read(key, [&first](QVector<WalletAccountRead> reads) {
        first = std::move(reads);
    });

    QCOMPARE(provider.publicAccountReadCalls, 1);
    QCOMPARE(provider.lastPublicAccountIds, key.tokenIds);
    QCOMPARE(first.size(), key.tokenIds.size());
    QVERIFY(cache.contains(key));

    QVector<WalletAccountRead> second;
    cache.read(key, [&second](QVector<WalletAccountRead> reads) {
        second = std::move(reads);
    });

    QCOMPARE(provider.publicAccountReadCalls, 1);
    QCOMPARE(second.size(), first.size());
    for (qsizetype index = 0; index < second.size(); ++index) {
        QCOMPARE(second.at(index).accountId, first.at(index).accountId);
        QCOMPARE(second.at(index).status, first.at(index).status);
    }
}

void TokenDefinitionCacheTest::retriesIncompleteReads()
{
    FakeWalletProvider provider;
    TokenDefinitionCache cache(provider);
    const TokenDefinitionCacheKey key = cacheKey();

    cache.read(key, [](QVector<WalletAccountRead>) {});
    cache.read(key, [](QVector<WalletAccountRead>) {});

    QCOMPARE(provider.publicAccountReadCalls, 2);
    QVERIFY(!cache.contains(key));
}

void TokenDefinitionCacheTest::separatesNetworkKeys()
{
    FakeWalletProvider provider;
    makeReadsReady(provider);
    TokenDefinitionCache cache(provider);
    const TokenDefinitionCacheKey baseKey = cacheKey();
    TokenDefinitionCacheKey fingerprintKey = baseKey;
    fingerprintKey.networkFingerprint = QStringLiteral("channel:two");
    TokenDefinitionCacheKey endpointKey = baseKey;
    endpointKey.sequencerAddress = QStringLiteral("http://127.0.0.1:8081");
    TokenDefinitionCacheKey definitionsKey = baseKey;
    definitionsKey.tokenIds = {
        baseKey.tokenIds.at(1),
        baseKey.tokenIds.at(0),
    };
    TokenDefinitionCacheKey networkKey = baseKey;
    networkKey.networkId = QStringLiteral("testnet");

    cache.read(baseKey, [](QVector<WalletAccountRead>) {});
    cache.read(fingerprintKey, [](QVector<WalletAccountRead>) {});
    cache.read(endpointKey, [](QVector<WalletAccountRead>) {});
    cache.read(definitionsKey, [](QVector<WalletAccountRead>) {});
    cache.read(networkKey, [](QVector<WalletAccountRead>) {});

    QCOMPARE(provider.publicAccountReadCalls, 5);
    QVERIFY(!cache.contains(baseKey));
    QVERIFY(cache.contains(networkKey));
}

void TokenDefinitionCacheTest::coalescesInFlightReads()
{
    FakeWalletProvider provider;
    makeReadsReady(provider);
    provider.deferPublicAccountReads = true;
    TokenDefinitionCache cache(provider);
    const TokenDefinitionCacheKey key = cacheKey();
    bool firstCalled = false;
    bool secondCalled = false;

    cache.read(key, [&firstCalled](QVector<WalletAccountRead>) {
        firstCalled = true;
    });
    cache.read(key, [&secondCalled](QVector<WalletAccountRead>) {
        secondCalled = true;
    });

    QCOMPARE(provider.publicAccountReadCalls, 1);
    provider.completePendingPublicAccountReads();

    QVERIFY(firstCalled);
    QVERIFY(secondCalled);
    QVERIFY(cache.contains(key));
}

void TokenDefinitionCacheTest::restartsCancelledRead()
{
    FakeWalletProvider provider;
    makeReadsReady(provider);
    provider.deferPublicAccountReads = true;
    TokenDefinitionCache cache(provider);
    const TokenDefinitionCacheKey key = cacheKey();
    bool cancelledCallback = false;
    bool retryCallback = false;

    cache.read(key, [&cancelledCallback](QVector<WalletAccountRead>) {
        cancelledCallback = true;
    });
    cache.cancelPending();
    cache.read(key, [&retryCallback](QVector<WalletAccountRead>) {
        retryCallback = true;
    });

    QCOMPARE(provider.publicAccountReadCalls, 2);
    provider.completePendingPublicAccountReads();

    QVERIFY(!cancelledCallback);
    QVERIFY(retryCallback);
    QVERIFY(cache.contains(key));
}

void TokenDefinitionCacheTest::dropsPendingCallbackOnDestruction()
{
    FakeWalletProvider provider;
    makeReadsReady(provider);
    provider.deferPublicAccountReads = true;
    const TokenDefinitionCacheKey key = cacheKey();
    bool callbackCalled = false;

    {
        TokenDefinitionCache cache(provider);
        cache.read(key, [&callbackCalled](QVector<WalletAccountRead>) {
            callbackCalled = true;
        });
    }
    provider.completePendingPublicAccountReads();

    QVERIFY(!callbackCalled);
}

QTEST_GUILESS_MAIN(TokenDefinitionCacheTest)
#include "TokenDefinitionCacheTest.moc"
