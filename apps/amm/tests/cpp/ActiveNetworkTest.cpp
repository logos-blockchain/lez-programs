#include "ActiveNetwork.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryFile>

namespace {
    bool expect(bool condition, const char* message)
    {
        if (!condition)
            qCritical("%s", message);
        return condition;
    }
}

int main()
{
    const QString identity(64, QLatin1Char('a'));
    const QString programId(64, QLatin1Char('b'));
    const QString tokenId(64, QLatin1Char('c'));

    QTemporaryFile config;
    if (!config.open())
        return 1;
    config.write(QJsonDocument(QJsonObject {
        { QStringLiteral("channelId"), identity },
        { QStringLiteral("ammProgramId"), programId },
        { QStringLiteral("tokenDefinitionIds"), QJsonArray { tokenId } },
    }).toJson(QJsonDocument::Compact));
    config.flush();

    qputenv("AMM_UI_NETWORK", "devnet");
    qputenv("AMM_UI_DEVNET_FILE", config.fileName().toLocal8Bit());

    ActiveNetwork network;
    if (!expect(network.load(), "devnet config should load"))
        return 1;
    if (!expect(network.snapshot().status == QStringLiteral("network_unknown"),
                "loaded network should await identity"))
        return 1;

    network.sequencerChanged(true);
    network.finishIdentityProbe(QString(64, QLatin1Char('d')));
    if (!expect(network.status() == QStringLiteral("network_mismatch"),
                "wrong identity should block the network"))
        return 1;

    network.reachabilityChanged(false, true);
    network.reachabilityChanged(true, false);
    network.finishIdentityProbe(identity);
    const ActiveNetworkSnapshot snapshot = network.snapshot();
    if (!expect(snapshot.status == QStringLiteral("ready"),
                "matching identity should ready the network"))
        return 1;
    if (!expect(snapshot.fingerprint == QStringLiteral("channel:") + identity,
                "devnet fingerprint should use channel identity"))
        return 1;
    if (!expect(snapshot.ammProgramId == programId
                    && snapshot.tokenIds == QStringList { tokenId },
                "network snapshot should preserve configured program and tokens"))
        return 1;

    return 0;
}
