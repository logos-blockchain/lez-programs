//! Core data structures and utilities for the Stablecoin Program.

pub mod math;

use alloy_primitives::U256;
use borsh::{BorshDeserialize, BorshSerialize};
pub use math::{compound_rate, mul_div as mul_div_floor, mul_div_ceil, FIXED_POINT_ONE};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// Maximum elapsed time rolled into one compounding projection.
pub const MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS: u64 = 86_400_000;

/// Maximum accepted stability-fee multiplier per millisecond.
pub const MAX_STABILITY_FEE_PER_MILLISECOND: u128 =
    FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000_000_000;

/// Minimum accepted collateralization ratio: 110%.
pub const MINIMUM_COLLATERALIZATION_RATIO_LOWER_BOUND: u128 = FIXED_POINT_ONE / 10 * 11;

/// Maximum accepted collateralization ratio: 1000%.
pub const MINIMUM_COLLATERALIZATION_RATIO_UPPER_BOUND: u128 = FIXED_POINT_ONE * 10;

/// Maximum accepted proportional gain magnitude.
pub const MAX_CONTROLLER_PROPORTIONAL_GAIN: u128 = FIXED_POINT_ONE * 1_000;

/// Maximum accepted integral gain magnitude.
pub const MAX_CONTROLLER_INTEGRAL_GAIN: u128 = FIXED_POINT_ONE;

/// Maximum accepted timing parameter in milliseconds.
pub const MAX_TIMING_PARAMETER_MILLISECONDS: u64 = 86_400_000;

const PROTOCOL_PARAMETERS_PDA_DOMAIN: &[u8] = b"PROTOCOL_PARAMETERS";
const STABILITY_FEE_ACCUMULATOR_PDA_DOMAIN: &[u8] = b"STABILITY_FEE_ACCUMULATOR";
const REDEMPTION_PRICE_STATE_PDA_DOMAIN: &[u8] = b"REDEMPTION_PRICE_STATE";
const STABLECOIN_DEFINITION_PDA_DOMAIN: &[u8] = b"STABLECOIN_DEFINITION";
const STABLECOIN_MASTER_HOLDING_PDA_DOMAIN: &[u8] = b"STABLECOIN_MASTER_HOLDING";
const POSITION_PDA_DOMAIN: &[u8] = b"POSITION";
const POSITION_VAULT_PDA_DOMAIN: &[u8] = b"POSITION_VAULT";

/// Stablecoin Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Initialize protocol globals and stablecoin token definition.
    InitializeProgram {
        freeze_authority_account_id: AccountId,
        initial_stability_fee_per_millisecond: u128,
        initial_controller_proportional_gain: i128,
        initial_controller_integral_gain: i128,
        initial_minimum_collateralization_ratio: u128,
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
        initial_redemption_price: u128,
        stablecoin_name: String,
    },
    /// Permissionlessly roll the global stability-fee accumulator forward.
    AccrueStabilityFee,
    /// Update the stability-fee rate after accruing pending fees at the old rate.
    SetStabilityFeePerMillisecond { new_rate: u128 },
    /// Open a new collateral-only [`Position`] for the calling owner.
    OpenPosition {
        /// User-chosen nonce that disambiguates multiple positions.
        position_nonce: u64,
        /// Amount of collateral tokens to deposit into the position vault.
        initial_collateral_amount: u128,
    },
    /// Mint stablecoin debt against an existing position.
    GenerateDebt {
        /// Amount of stablecoin to mint.
        amount: u128,
    },
    /// Withdraw `amount` collateral tokens from a position back to a user-controlled holding.
    WithdrawCollateral {
        /// Amount of collateral tokens to move from the vault back to `destination`.
        amount: u128,
    },
    /// Repay `amount` of outstanding stablecoin debt against an existing position.
    RepayDebt {
        /// Amount of stablecoin debt to repay and burn from the user's holding.
        amount: u128,
    },
}

/// Protocol-level configuration account.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ProtocolParameters {
    pub admin_account_id: AccountId,
    pub freeze_authority_account_id: AccountId,
    pub stablecoin_definition_id: AccountId,
    pub collateral_definition_id: AccountId,
    pub market_price_oracle_id: AccountId,
    pub stability_fee_per_millisecond: u128,
    pub controller_proportional_gain: i128,
    pub controller_integral_gain: i128,
    pub minimum_collateralization_ratio: u128,
    pub minimum_milliseconds_between_rate_updates: u64,
    pub maximum_oracle_price_age_milliseconds: u64,
    pub is_frozen: bool,
}

/// Global stability-fee accumulator.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct StabilityFeeAccumulator {
    pub accumulated_rate_at_last_accrual: u128,
    pub last_accrued_at: u64,
}

/// Redemption-price state used by collateralization checks.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct RedemptionPriceState {
    pub redemption_price_at_last_update: u128,
    pub redemption_rate_per_millisecond: u128,
    pub controller_integral_term: i128,
    pub last_updated_at: u64,
}

/// Persistent state held by a Stablecoin [`Position`] account.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Position {
    /// Owner authorized for every position operation.
    pub owner_account_id: AccountId,
    /// User-chosen nonce. Together with owner, it derives the position PDA.
    pub position_nonce: u64,
    /// Token holding account that custodies collateral for this position.
    pub vault_account_id: AccountId,
    /// Amount of collateral tokens deposited.
    pub collateral_amount: u128,
    /// Debt shares. Nominal debt is derived from this and the current accumulator.
    pub normalized_debt_amount: u128,
    /// Unix timestamp in milliseconds when the position was opened.
    pub opened_at: u64,
}

impl TryFrom<&Data> for ProtocolParameters {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl TryFrom<&Data> for StabilityFeeAccumulator {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl TryFrom<&Data> for RedemptionPriceState {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl TryFrom<&Data> for Position {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&ProtocolParameters> for Data {
    fn from(params: &ProtocolParameters) -> Self {
        serialize_to_data(params)
    }
}

impl From<&StabilityFeeAccumulator> for Data {
    fn from(accumulator: &StabilityFeeAccumulator) -> Self {
        serialize_to_data(accumulator)
    }
}

impl From<&RedemptionPriceState> for Data {
    fn from(state: &RedemptionPriceState) -> Self {
        serialize_to_data(state)
    }
}

impl From<&Position> for Data {
    fn from(position: &Position) -> Self {
        serialize_to_data(position)
    }
}

fn serialize_to_data<T: BorshSerialize>(value: &T) -> Data {
    let bytes = borsh::to_vec(value).expect("Serialization to Vec should not fail");
    Data::try_from(bytes).expect("Encoded account data should fit into Data")
}

fn hash_seed(parts: &[&[u8]]) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let total_len = parts
        .iter()
        .try_fold(0usize, |acc, part| acc.checked_add(part.len()))
        .expect("PDA seed length should fit in usize");
    let mut bytes = Vec::with_capacity(total_len);
    for part in parts {
        bytes.extend_from_slice(part);
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// PDA seed for the protocol parameters singleton.
#[must_use]
pub fn compute_protocol_parameters_pda_seed() -> PdaSeed {
    hash_seed(&[PROTOCOL_PARAMETERS_PDA_DOMAIN])
}

/// Account id of the protocol parameters singleton.
#[must_use]
pub fn compute_protocol_parameters_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_protocol_parameters_pda_seed(),
    )
}

/// PDA seed for the stability-fee accumulator singleton.
#[must_use]
pub fn compute_stability_fee_accumulator_pda_seed() -> PdaSeed {
    hash_seed(&[STABILITY_FEE_ACCUMULATOR_PDA_DOMAIN])
}

/// Account id of the stability-fee accumulator singleton.
#[must_use]
pub fn compute_stability_fee_accumulator_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_stability_fee_accumulator_pda_seed(),
    )
}

/// PDA seed for the redemption-price state singleton.
#[must_use]
pub fn compute_redemption_price_state_pda_seed() -> PdaSeed {
    hash_seed(&[REDEMPTION_PRICE_STATE_PDA_DOMAIN])
}

/// Account id of the redemption-price state singleton.
#[must_use]
pub fn compute_redemption_price_state_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_redemption_price_state_pda_seed(),
    )
}

/// PDA seed for the stablecoin token definition.
#[must_use]
pub fn compute_stablecoin_definition_pda_seed() -> PdaSeed {
    hash_seed(&[STABLECOIN_DEFINITION_PDA_DOMAIN])
}

/// Account id of the stablecoin token definition.
#[must_use]
pub fn compute_stablecoin_definition_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_stablecoin_definition_pda_seed(),
    )
}

/// PDA seed for the stablecoin token definition's paired master holding.
#[must_use]
pub fn compute_stablecoin_master_holding_pda_seed() -> PdaSeed {
    hash_seed(&[STABLECOIN_MASTER_HOLDING_PDA_DOMAIN])
}

/// Account id of the stablecoin token definition's paired master holding.
#[must_use]
pub fn compute_stablecoin_master_holding_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_stablecoin_master_holding_pda_seed(),
    )
}

/// PDA seed for a [`Position`] account owned by `owner_id` with `position_nonce`.
#[must_use]
pub fn compute_position_pda_seed(owner_id: AccountId, position_nonce: u64) -> PdaSeed {
    hash_seed(&[
        &owner_id.to_bytes(),
        &position_nonce.to_le_bytes(),
        POSITION_PDA_DOMAIN,
    ])
}

/// Account id of the [`Position`] PDA owned by `owner_id` under `stablecoin_program_id`.
#[must_use]
pub fn compute_position_pda(
    stablecoin_program_id: ProgramId,
    owner_id: AccountId,
    position_nonce: u64,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_pda_seed(owner_id, position_nonce),
    )
}

/// PDA seed for the collateral vault token holding bound to a [`Position`].
#[must_use]
pub fn compute_position_vault_pda_seed(position_id: AccountId) -> PdaSeed {
    hash_seed(&[&position_id.to_bytes(), POSITION_VAULT_PDA_DOMAIN])
}

/// Account id of the collateral vault PDA for `position_id` under `stablecoin_program_id`.
#[must_use]
pub fn compute_position_vault_pda(
    stablecoin_program_id: ProgramId,
    position_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_vault_pda_seed(position_id),
    )
}

/// Verify a position account's address and return the PDA seed for post-state claims.
///
/// # Panics
/// If `position.account_id` does not match the expected PDA.
#[must_use]
pub fn verify_position_and_get_seed(
    position: &AccountWithMetadata,
    owner: &AccountWithMetadata,
    position_nonce: u64,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_pda_seed(owner.account_id, position_nonce);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        position.account_id, expected_id,
        "Position account ID does not match expected derivation"
    );
    seed
}

/// Verify a vault account's address and return the PDA seed for chained calls.
///
/// # Panics
/// If `vault.account_id` does not match the expected PDA.
#[must_use]
pub fn verify_position_vault_and_get_seed(
    vault: &AccountWithMetadata,
    position_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_vault_pda_seed(position_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        vault.account_id, expected_id,
        "Position vault account ID does not match expected derivation"
    );
    seed
}

/// Projects the current global stability-fee accumulator.
///
/// # Panics
/// Panics if fixed-point multiplication overflows.
#[must_use]
pub fn current_accumulated_rate(
    state: &StabilityFeeAccumulator,
    params: &ProtocolParameters,
    now: u64,
) -> u128 {
    let elapsed = now
        .saturating_sub(state.last_accrued_at)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let factor = compound_rate(params.stability_fee_per_millisecond, elapsed);
    mul_div_floor(
        state.accumulated_rate_at_last_accrual,
        factor,
        FIXED_POINT_ONE,
    )
}

/// Projects the current redemption price.
///
/// # Panics
/// Panics if fixed-point multiplication overflows.
#[must_use]
pub fn current_redemption_price(state: &RedemptionPriceState, now: u64) -> u128 {
    let elapsed = now
        .saturating_sub(state.last_updated_at)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let factor = compound_rate(state.redemption_rate_per_millisecond, elapsed);
    mul_div_floor(
        state.redemption_price_at_last_update,
        factor,
        FIXED_POINT_ONE,
    )
}

/// Computes nominal stablecoin debt from normalized position debt.
///
/// # Panics
/// Panics if fixed-point multiplication overflows.
#[must_use]
pub fn nominal_debt(normalized_debt_amount: u128, current_accumulator: u128) -> u128 {
    mul_div_floor(normalized_debt_amount, current_accumulator, FIXED_POINT_ONE)
}

/// Checks the README collateralization inequality using 256-bit intermediates.
///
/// # Panics
/// Panics if an intermediate exceeds 256 bits.
#[must_use]
pub fn is_collateralized(
    collateral_amount: u128,
    normalized_debt_amount: u128,
    current_accumulator: u128,
    redemption_price: u128,
    minimum_collateralization_ratio: u128,
) -> bool {
    let nominal_debt = nominal_debt(normalized_debt_amount, current_accumulator);
    if nominal_debt == 0 {
        return true;
    }

    let fixed_point = U256::from(FIXED_POINT_ONE);
    let left = U256::from(collateral_amount)
        .checked_mul(fixed_point)
        .and_then(|value| value.checked_mul(fixed_point))
        .expect("collateral side should fit in U256");
    let right = U256::from(nominal_debt)
        .checked_mul(U256::from(redemption_price))
        .and_then(|value| value.checked_mul(U256::from(minimum_collateralization_ratio)))
        .expect("debt side should fit in U256");

    left >= right
}

/// Validates protocol parameter bounds from the stablecoin README.
///
/// # Panics
/// Panics if any parameter is outside the accepted range.
pub fn assert_protocol_parameter_bounds(
    stability_fee_per_millisecond: u128,
    controller_proportional_gain: i128,
    controller_integral_gain: i128,
    minimum_collateralization_ratio: u128,
    minimum_milliseconds_between_rate_updates: u64,
    maximum_oracle_price_age_milliseconds: u64,
    initial_redemption_price: u128,
) {
    assert_valid_stability_fee_per_millisecond(stability_fee_per_millisecond);
    assert!(
        controller_proportional_gain.unsigned_abs() <= MAX_CONTROLLER_PROPORTIONAL_GAIN,
        "Controller proportional gain is out of bounds"
    );
    assert!(
        controller_integral_gain.unsigned_abs() <= MAX_CONTROLLER_INTEGRAL_GAIN,
        "Controller integral gain is out of bounds"
    );
    assert!(
        (MINIMUM_COLLATERALIZATION_RATIO_LOWER_BOUND..=MINIMUM_COLLATERALIZATION_RATIO_UPPER_BOUND)
            .contains(&minimum_collateralization_ratio),
        "Minimum collateralization ratio is out of bounds"
    );
    assert!(
        (1..=MAX_TIMING_PARAMETER_MILLISECONDS)
            .contains(&minimum_milliseconds_between_rate_updates),
        "Minimum milliseconds between rate updates is out of bounds"
    );
    assert!(
        (1..=MAX_TIMING_PARAMETER_MILLISECONDS).contains(&maximum_oracle_price_age_milliseconds),
        "Maximum oracle price age is out of bounds"
    );
    assert!(
        initial_redemption_price != 0,
        "Initial redemption price must be non-zero"
    );
}

/// Validates the stability-fee multiplier bound.
///
/// # Panics
/// Panics if `rate` is outside the accepted range.
pub fn assert_valid_stability_fee_per_millisecond(rate: u128) {
    assert!(
        (FIXED_POINT_ONE..=MAX_STABILITY_FEE_PER_MILLISECOND).contains(&rate),
        "Stability fee per millisecond is out of bounds"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM_ID: ProgramId = [1u32; 8];

    fn params(rate: u128) -> ProtocolParameters {
        ProtocolParameters {
            admin_account_id: AccountId::new([1u8; 32]),
            freeze_authority_account_id: AccountId::new([2u8; 32]),
            stablecoin_definition_id: AccountId::new([3u8; 32]),
            collateral_definition_id: AccountId::new([4u8; 32]),
            market_price_oracle_id: AccountId::new([5u8; 32]),
            stability_fee_per_millisecond: rate,
            controller_proportional_gain: 0,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: MINIMUM_COLLATERALIZATION_RATIO_LOWER_BOUND,
            minimum_milliseconds_between_rate_updates: 1,
            maximum_oracle_price_age_milliseconds: 1,
            is_frozen: false,
        }
    }

    #[test]
    fn compound_rate_returns_identity_for_zero_elapsed_time() {
        assert_eq!(
            compound_rate(FIXED_POINT_ONE + FIXED_POINT_ONE / 10, 0),
            FIXED_POINT_ONE
        );
    }

    #[test]
    fn compound_rate_keeps_one_rate_at_identity() {
        assert_eq!(compound_rate(FIXED_POINT_ONE, 123), FIXED_POINT_ONE);
    }

    #[test]
    fn compound_rate_compounds_small_growth() {
        let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 10;

        assert_eq!(
            compound_rate(rate, 2),
            FIXED_POINT_ONE + FIXED_POINT_ONE / 5 + FIXED_POINT_ONE / 100
        );
    }

    #[test]
    fn mul_div_floor_and_ceil_split_remainders() {
        assert_eq!(mul_div_floor(10, 10, 6), 16);
        assert_eq!(mul_div_ceil(10, 10, 6), 17);
    }

    #[test]
    fn current_accumulated_rate_clamps_elapsed_time() {
        let rate = FIXED_POINT_ONE + 1;
        let state = StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: 0,
        };

        assert_eq!(
            current_accumulated_rate(
                &state,
                &params(rate),
                MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1
            ),
            compound_rate(rate, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS)
        );
    }

    #[test]
    fn max_stability_fee_compounds_within_the_clamped_window() {
        let factor = compound_rate(
            MAX_STABILITY_FEE_PER_MILLISECOND,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );

        assert!(factor >= FIXED_POINT_ONE);
    }

    #[test]
    #[should_panic(expected = "Stability fee per millisecond is out of bounds")]
    fn stability_fee_bound_rejects_rate_above_safe_maximum() {
        assert_valid_stability_fee_per_millisecond(MAX_STABILITY_FEE_PER_MILLISECOND + 1);
    }

    #[test]
    fn nominal_debt_grows_with_accumulator() {
        assert_eq!(
            nominal_debt(100, FIXED_POINT_ONE + FIXED_POINT_ONE / 10),
            110
        );
    }

    #[test]
    fn pda_helpers_keep_position_and_vault_derivations_distinct() {
        let owner = AccountId::new([9u8; 32]);
        let position = compute_position_pda(PROGRAM_ID, owner, 7);
        let vault = compute_position_vault_pda(PROGRAM_ID, position);

        assert_ne!(position, vault);
    }
}
