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

struct WalletDecodedProgram {
    QString programId;
    QString programName;
    WalletDecodeResult result;
};

class WalletIdlRegistry final {
public:
    void registerProgram(const QString& programId,
                         const QString& programName,
                         const QByteArray& idlJson);
    QVector<WalletDecodedProgram> decode(
        const QVector<WalletAccountRead>& accounts) const;

private:
    struct Program {
        QString name;
        QByteArray idl;
    };

    QHash<QString, Program> m_programs;
};
