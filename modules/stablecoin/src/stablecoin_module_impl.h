#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <logos_json.h>
#include <logos_module_context.h>

// Universal Logos core module for the LEZ Stablecoin Program. Rust
// stablecoin_ffi owns typed codecs, PDA derivation, validation, and instruction
// serialization. This Qt-free adapter owns live reads and wallet submission.
class StablecoinModuleImpl : public LogosModuleContext {
public:
    StablecoinModuleImpl() = default;
    ~StablecoinModuleImpl() = default;

    /// Returns stablecoin program IDs and all derived singleton account IDs.
    /// Configure STABLECOIN_PROGRAM_ID or STABLECOIN_PROGRAM_BIN. When both are
    /// configured, they must identify the same program.
    LogosMap programInfo();

    /// Reads and exactly decodes the singleton ProtocolParameters account.
    /// Success adds `protocolParameters`; failures use stable error codes.
    LogosMap protocolParameters();

    /// Initializes the stablecoin protocol. Request fields are `adminId`,
    /// `freezeAuthorityId`, `collateralDefinitionId`, `marketPriceOracleId`,
    /// `initialStabilityFeePerMillisecond`,
    /// `initialControllerProportionalGain`, `initialControllerIntegralGain`,
    /// `initialMinimumCollateralizationRatio`,
    /// `minimumMillisecondsBetweenRateUpdates`,
    /// `maximumOraclePriceAgeMilliseconds`, `initialRedemptionPrice`, and
    /// `stablecoinName`. Integer values accept exact decimal strings or JSON
    /// integers; JSON floats are rejected. Only `adminId` signs.
    LogosMap initializeProgram(const LogosMap& request);

private:
    std::vector<std::uint8_t> loadStablecoinBinary() const;
    nlohmann::json stablecoinProgramInfo(std::string& error);
    std::string normalizeAccountId(const std::string& id);
    nlohmann::json readPublicAccount(const std::string& account_id);
    bool requireUninitialized(const std::string& account_id, std::string& error);
    LogosMap submitPlan(const nlohmann::json& plan);

    bool programInfoResolved_ = false;
    std::string programInfoJson_;
};
