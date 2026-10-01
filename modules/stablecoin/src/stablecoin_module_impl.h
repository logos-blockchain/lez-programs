#pragma once

#include <cstddef>
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

    /// Reads and exactly decodes the singleton StabilityFeeAccumulator account.
    /// Returns the stored snapshot without projecting it to the current time.
    LogosMap stabilityFeeAccumulator();

    /// Reads and exactly decodes the singleton RedemptionPriceState account.
    /// Returns stored controller state without projecting the current price.
    LogosMap redemptionPriceState();

    /// Reads all global state and projects the accumulator and redemption price
    /// at the canonical CLOCK_01 timestamp.
    LogosMap currentGlobalState();

    /// Quotes the next redemption-rate controller tick from live protocol,
    /// redemption-price, configured oracle, and CLOCK_01 state. Never submits
    /// a transaction; soft gates return `canSubmit: false` with blockers.
    LogosMap redemptionRateUpdateQuote();

    /// Advances the stability-fee accumulator. `caller_id` must identify a
    /// public account controlled by the connected wallet and is the sole signer.
    LogosMap accrueStabilityFee(const std::string& caller_id);

    /// Runs one strict redemption-rate controller tick. The live quote preflight
    /// blocks stale/zero oracle data and updates attempted before the interval.
    LogosMap updateRedemptionRate(const std::string& caller_id);

    /// Advances the fee accumulator and best-effort redemption-rate state. The
    /// redemption half may be skipped on soft gates; the transaction still runs.
    LogosMap refreshGlobals(const std::string& caller_id);

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
    using StablecoinOperation = char* (*)(const char*);

    std::vector<std::uint8_t> loadStablecoinBinary() const;
    nlohmann::json stablecoinProgramInfo(std::string& error);
    std::string normalizeAccountId(const std::string& id);
    bool requireWalletCaller(const std::string& caller_id, std::string& error);
    nlohmann::json readPublicAccount(const std::string& account_id);
    bool requireUninitialized(const std::string& account_id, std::string& error);
    LogosMap planAndSubmit(StablecoinOperation planner,
                           const nlohmann::json& request,
                           const std::string& expected_program_id,
                           std::size_t expected_account_count);
    LogosMap submitPlan(const nlohmann::json& plan, std::size_t expected_account_count);

    bool programInfoResolved_ = false;
    std::string programInfoJson_;
};
