#include "WalletIdlDecoder.h"

#include <utility>

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>

#include <wallet_idl_decoder.h>

WalletDecodeResult WalletIdlDecoder::decode(
    const QByteArray& idlJson,
    const QVector<WalletAccountRead>& accounts)
{
    WalletDecodeResult result;
    QJsonParseError idlError;
    const QJsonDocument idl = QJsonDocument::fromJson(idlJson, &idlError);
    if (idlError.error != QJsonParseError::NoError || !idl.isObject()) {
        result.status = QStringLiteral("error");
        result.error = QStringLiteral("invalid_idl");
        return result;
    }

    QJsonArray inputs;
    for (const WalletAccountRead& account : accounts) {
        inputs.append(QJsonObject {
            { QStringLiteral("id"), account.accountId },
            { QStringLiteral("dataHex"), account.dataHex },
        });
    }
    const QByteArray request = QJsonDocument(QJsonObject {
        { QStringLiteral("idl"), idl.object() },
        { QStringLiteral("accounts"), inputs },
    }).toJson(QJsonDocument::Compact);

    char* responsePointer = wallet_idl_decode_accounts(request.constData());
    if (!responsePointer) {
        result.status = QStringLiteral("error");
        result.error = QStringLiteral("decoder_unavailable");
        return result;
    }
    const QByteArray response(responsePointer);
    wallet_idl_decoder_free(responsePointer);

    QJsonParseError responseError;
    const QJsonDocument document = QJsonDocument::fromJson(response, &responseError);
    if (responseError.error != QJsonParseError::NoError || !document.isObject()) {
        result.status = QStringLiteral("error");
        result.error = QStringLiteral("invalid_decoder_response");
        return result;
    }

    const QJsonObject root = document.object();
    result.status = root.value(QStringLiteral("status")).toString();
    result.error = root.value(QStringLiteral("error")).toString();
    for (const QJsonValue& value : root.value(QStringLiteral("accounts")).toArray()) {
        const QJsonObject decoded = value.toObject();
        WalletDecodedAccount account;
        account.id = decoded.value(QStringLiteral("id")).toString();
        account.status = decoded.value(QStringLiteral("status")).toString();
        account.typeName = decoded.value(QStringLiteral("typeName")).toString();
        account.value = decoded.value(QStringLiteral("value"));
        const QJsonObject ids = decoded.value(QStringLiteral("accountIds")).toObject();
        for (auto iterator = ids.begin(); iterator != ids.end(); ++iterator)
            account.accountIds.insert(iterator.key(), iterator.value().toString());
        result.accounts.append(std::move(account));
    }
    return result;
}
