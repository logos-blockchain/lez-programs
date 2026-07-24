#pragma once

#include <QString>
#include <QStringList>

// Network context handed to the new-position flow. The AMM deployment identity
// (ammProgramId, from $AMM_PROGRAM_BIN) and the configured token set (tokenIds,
// from $TOKENS_CONFIG) are the same sources the Swap view uses; there is no
// separate network config file or channel-identity probe. `fingerprint` binds a
// quote to the deployment so a quote can't be replayed against a different one.
struct ActiveNetworkSnapshot {
    QString id;
    QString status;
    QString fingerprint;
    QString ammProgramId;
    QStringList tokenIds;
};
