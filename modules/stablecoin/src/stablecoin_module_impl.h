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

    /// Quotes stored Position health at CLOCK_01, using wide fractional-debt
    /// arithmetic. Requires `ownerId` and exact decimal-string `positionNonce`.
    /// No signer, wallet ownership, oracle, or transaction is required.
    LogosMap positionHealth(const LogosMap& request);

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

    /// Opens a collateral-only position. `ownerId`, `userCollateralHoldingId`,
    /// `positionNonce`, and `initialCollateralAmount` are required; amounts
    /// are exact decimal strings. Owner and source holding must be public
    /// accounts controlled by the connected wallet and both sign.
    LogosMap openPosition(const LogosMap& request);

    /// Deposits collateral into an existing position and reconciles donations
    /// from the live vault balance. `ownerId`, `positionNonce`,
    /// `userCollateralHoldingId`, and `amount` are required. Use decimal
    /// strings for portable exact integers; JSON floats are rejected. Owner
    /// and source holding must be public accounts controlled by the wallet and
    /// both sign, including for zero-amount reconciliation.
    LogosMap depositCollateral(const LogosMap& request);

    /// Burns the requested stablecoins and reduces normalized debt using the
    /// current fee accumulator, rounding down. Requires `ownerId`,
    /// `positionNonce`, `userStablecoinHoldingId`, and `amount`; use decimal
    /// strings for exact integers. Owner and source holding both sign.
    /// Repayment remains available while frozen, including zero amounts.
    LogosMap repayDebt(const LogosMap& request);

private:
    using StablecoinOperation = char* (*)(const char*);

    std::vector<std::uint8_t> loadStablecoinBinary() const;
    nlohmann::json stablecoinProgramInfo(std::string& error);
    std::string normalizeAccountId(const std::string& id);
    bool requireWalletCaller(const std::string& caller_id, std::string& error);
    bool requireWalletSigners(const std::vector<std::string>& signer_ids,
                              std::string& error);
    nlohmann::json readPublicAccount(const std::string& account_id);
    bool requireUninitialized(const std::string& account_id, std::string& error);
    LogosMap planAndSubmit(StablecoinOperation planner,
                           const nlohmann::json& request,
                           const std::string& expected_program_id,
                           std::size_t expected_account_count,
                           std::vector<std::size_t> expected_signer_indices = {0});
    LogosMap submitPlan(const nlohmann::json& plan,
                        std::size_t expected_account_count,
                        const std::vector<std::size_t>& expected_signer_indices = {0});

    bool programInfoResolved_ = false;
    std::string programInfoJson_;
};
