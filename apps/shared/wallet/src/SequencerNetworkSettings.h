#pragma once

#include <optional>

#include <QString>

#include "SequencerNetworkContext.h"

enum class SequencerIdentityMethod {
    CheckpointBlock,
    ChannelId,
};

struct SequencerNetworkSettings {
    SequencerNetworkContext::Configuration context;
    SequencerIdentityMethod identityMethod = SequencerIdentityMethod::CheckpointBlock;
};

// Loads the identity contract for a wallet network. Program deployments and
// application-specific assets deliberately stay outside this loader.
class SequencerNetworkSettingsLoader final {
public:
    static std::optional<SequencerNetworkSettings> load(
        const QString& networkId,
        const QString& devnetConfigPath,
        const QString& resourcePath = QStringLiteral(":/wallet/config/networks.json"));
};
