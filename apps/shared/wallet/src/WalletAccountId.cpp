#include "WalletAccountId.h"

#include <QByteArray>
#include <QVector>

namespace {
constexpr char BASE58_ALPHABET[] =
    "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

bool isHexCharacter(QChar character)
{
    const ushort value = character.unicode();
    return (value >= '0' && value <= '9')
        || (value >= 'a' && value <= 'f')
        || (value >= 'A' && value <= 'F');
}
}

QString walletAccountIdToBase58(const QString& accountId)
{
    if (accountId.size() != 64)
        return {};
    for (const QChar character : accountId) {
        if (!isHexCharacter(character))
            return {};
    }

    const QByteArray bytes = QByteArray::fromHex(accountId.toLatin1());
    if (bytes.size() != 32)
        return {};

    qsizetype leadingZeroes = 0;
    while (leadingZeroes < bytes.size() && bytes.at(leadingZeroes) == 0)
        ++leadingZeroes;

    QVector<unsigned char> digits;
    digits.reserve(45);
    for (const char byte : bytes) {
        int carry = static_cast<unsigned char>(byte);
        for (unsigned char& digit : digits) {
            carry += static_cast<int>(digit) * 256;
            digit = static_cast<unsigned char>(carry % 58);
            carry /= 58;
        }
        while (carry > 0) {
            digits.append(static_cast<unsigned char>(carry % 58));
            carry /= 58;
        }
    }

    QString encoded(leadingZeroes, QLatin1Char('1'));
    encoded.reserve(leadingZeroes + digits.size());
    for (auto digit = digits.crbegin(); digit != digits.crend(); ++digit)
        encoded.append(QLatin1Char(BASE58_ALPHABET[*digit]));
    return encoded;
}
