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

    /// Withdraws recorded collateral after a native-compatible health preflight.
    /// Requires `ownerId`, `positionNonce`, `userCollateralHoldingId`, and
    /// `amount`. Only the Position owner must be a public wallet signer;
    /// the destination holding need not belong to the wallet. Frozen calls fail.
    LogosMap withdrawCollateral(const LogosMap& request);

    /// Borrows stablecoins after upward-rounded debt pricing, oracle freshness
    /// and post-mint health validation. Requires `ownerId`, `positionNonce`,
    /// `userStablecoinHoldingId`, and `amount`. Only the public wallet owner
    /// signs; the destination holding need not belong to the wallet.
    LogosMap generateDebt(const LogosMap& request);

    /// Clears a settled Position's data without releasing its PDA or vault.
    /// Requires `ownerId` and exact decimal-string `positionNonce`; only the
    /// public wallet owner signs. Debt, recorded collateral, and actual vault
    /// balance must all be zero. Frozen protocols may still close positions.
    LogosMap closePosition(const LogosMap& request);

    /// Sets `newRatio` (u128) in the inclusive native 1.1x..10x band.
    /// `adminId` must be the current public wallet admin. Allowed while frozen.
    LogosMap setMinimumCollateralizationRatio(const LogosMap& request);

    /// Sets signed i128 `newProportionalGain` and `newIntegralGain` atomically,
    /// without resetting redemption state. Requires the current `adminId`.
    LogosMap setControllerGains(const LogosMap& request);

    /// Sets u64 `newMinimumMillisecondsBetweenRateUpdates` and
    /// `newMaximumOraclePriceAgeMilliseconds` together, each 1..86400000.
    /// Requires the current `adminId`; globals are not automatically advanced.
    LogosMap setTimingParameters(const LogosMap& request);

    /// Immediately replaces the current `adminId` with `newAdminId`.
    /// Only the current admin signs; the replacement need not be in the wallet.
    LogosMap setAdmin(const LogosMap& request);

    /// Admin-authorized one-step rotation to `newFreezeAuthorityId`.
    /// The existing freeze authority is not sufficient to authorize rotation.
    LogosMap setFreezeAuthority(const LogosMap& request);

    /// Replaces the oracle with `newOracleId`, whose exact data must match the
    /// bound asset pair. No producer, price-freshness or nonzero-price gate.
    /// Only the current `adminId` signs; frozen state does not block the setter.
    LogosMap setMarketPriceOracle(const LogosMap& request);

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
    LogosMap adminPlanAndSubmit(StablecoinOperation planner,
                               const LogosMap& request,
                               const std::string& new_oracle_id = {});

    bool programInfoResolved_ = false;
    std::string programInfoJson_;
};
