#include "WalletIdlDecoder.h"

#include <QtTest>

class WalletIdlDecoderLinkTest final : public QObject {
    Q_OBJECT

private slots:
    void linksDefaultDecoder();
};

void WalletIdlDecoderLinkTest::linksDefaultDecoder()
{
    const WalletDecodeResult result = WalletIdlDecoder::decode(
        QByteArrayLiteral("not-json"), {});

    QCOMPARE(result.status, QStringLiteral("error"));
    QCOMPARE(result.error, QStringLiteral("invalid_idl"));
}

QTEST_GUILESS_MAIN(WalletIdlDecoderLinkTest)
#include "WalletIdlDecoderLinkTest.moc"
