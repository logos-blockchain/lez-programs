#pragma once

#include <QByteArray>
#include <QHash>
#include <QJsonValue>
#include <QString>
#include <QVector>

#include "WalletProvider.h"

struct WalletDecodedAccount {
    QString id;
    QString status;
    QString typeName;
    QJsonValue value;
    QHash<QString, QString> accountIds;
};

struct WalletDecodeResult {
    QString status;
    QString error;
    QVector<WalletDecodedAccount> accounts;

    bool ok() const { return status == QStringLiteral("ok"); }
};

class WalletIdlDecoder final {
public:
    static WalletDecodeResult decode(const QByteArray& idlJson,
                                     const QVector<WalletAccountRead>& accounts);
};
