#include "WalletAccountId.h"

#include <QByteArray>
#include <QVector>

namespace {
constexpr char BASE58_ALPHABET[] =
    "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
constexpr qsizetype ACCOUNT_ID_BYTES = 32;
constexpr qsizetype MIN_BASE58_ACCOUNT_ID_SIZE = 32;
constexpr qsizetype MAX_BASE58_ACCOUNT_ID_SIZE = 44;

bool isHexCharacter(QChar character)
{
    const ushort value = character.unicode();
    return (value >= '0' && value <= '9')
        || (value >= 'a' && value <= 'f')
        || (value >= 'A' && value <= 'F');
}

int base58Digit(QChar character)
{
    const ushort value = character.unicode();
    if (value > 0x7f)
        return -1;
    for (int digit = 0; BASE58_ALPHABET[digit] != '\0'; ++digit) {
        if (BASE58_ALPHABET[digit] == static_cast<char>(value))
            return digit;
    }
    return -1;
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
    if (bytes.size() != ACCOUNT_ID_BYTES)
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

QString walletAccountIdFromBase58(const QString& accountId)
{
    if (accountId.size() < MIN_BASE58_ACCOUNT_ID_SIZE
        || accountId.size() > MAX_BASE58_ACCOUNT_ID_SIZE) {
        return {};
    }

    qsizetype leadingZeroes = 0;
    while (leadingZeroes < accountId.size()
           && accountId.at(leadingZeroes) == QLatin1Char('1')) {
        ++leadingZeroes;
    }

    QVector<unsigned char> bytes;
    bytes.reserve(ACCOUNT_ID_BYTES);
    for (const QChar character : accountId) {
        int carry = base58Digit(character);
        if (carry < 0)
            return {};
        for (unsigned char& byte : bytes) {
            carry += static_cast<int>(byte) * 58;
            byte = static_cast<unsigned char>(carry % 256);
            carry /= 256;
        }
        while (carry > 0) {
            bytes.append(static_cast<unsigned char>(carry % 256));
            carry /= 256;
        }
    }

    if (leadingZeroes > ACCOUNT_ID_BYTES
        || bytes.size() != ACCOUNT_ID_BYTES - leadingZeroes) {
        return {};
    }

    QByteArray decoded(ACCOUNT_ID_BYTES, '\0');
    for (qsizetype index = 0; index < bytes.size(); ++index)
        decoded[ACCOUNT_ID_BYTES - index - 1] = static_cast<char>(bytes.at(index));
    return QString::fromLatin1(decoded.toHex());
}
