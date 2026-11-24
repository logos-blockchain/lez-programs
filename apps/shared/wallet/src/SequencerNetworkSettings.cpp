#include "SequencerNetworkSettings.h"

#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QResource>

namespace {
std::optional<SequencerNetworkSettings> settingsForIdentity(
    const QString& id,
    const QString& identity,
    const QString& fingerprintPrefix,
    SequencerIdentityMethod method)
{
    if (!SequencerNetworkContext::isValidIdentity(identity))
        return std::nullopt;

    SequencerNetworkSettings settings;
    settings.context = { id, identity, fingerprintPrefix };
    settings.identityMethod = method;
    return settings;
}
}

std::optional<SequencerNetworkSettings> SequencerNetworkSettingsLoader::load(
    const QString& networkId,
    const QString& devnetConfigPath,
    const QString& resourcePath)
{
    Q_INIT_RESOURCE(logos_wallet_access_network_data);

    const QString id = networkId.trimmed().isEmpty()
        ? QStringLiteral("testnet") : networkId.trimmed();
    if (id == QStringLiteral("devnet")) {
        QFile file(devnetConfigPath);
        if (devnetConfigPath.isEmpty() || !file.open(QIODevice::ReadOnly))
            return std::nullopt;
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
        if (!document.isObject())
            return std::nullopt;
        return settingsForIdentity(
            id,
            document.object().value(QStringLiteral("channelId")).toString(),
            QStringLiteral("channel:"),
            SequencerIdentityMethod::ChannelId);
    }

    QFile file(resourcePath);
    if (!file.open(QIODevice::ReadOnly))
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
    if (!document.isObject())
        return std::nullopt;
    const QJsonObject entry = document.object().value(id).toObject();
    return settingsForIdentity(
        id,
        entry.value(QStringLiteral("checkpointHash")).toString(),
        QStringLiteral("block10:"),
        SequencerIdentityMethod::CheckpointBlock);
}
