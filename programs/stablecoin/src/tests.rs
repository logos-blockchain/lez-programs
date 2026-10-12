#![cfg(test)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    clippy::wildcard_enum_match_arm,
    reason = "fixtures build ratios from constants to lock the §6.2 and §8 boundaries, index fixed-length call lists, assert panics deliberately, and match one chained-call variant against a borsh-only instruction enum"
)]

//! Unit tests for the Stablecoin Program's two phases.
//!
//! Each instruction is now two testable pieces rather than one: a `plan_*`
//! function over `AccountMeta`s, and one or more apply halves over a single
//! shard's bytes. The split is visible in the test names — `plan_*` tests drive
//! the first, `check_*` / `position_after_*` / `accrue_fee` / `advance_redemption`
//! / `update_parameters` the second.
//!
//! Three families of v0.2.5 test have no successor, each because the thing they
//! asserted no longer exists:
//!
//! * `*_echoes_*`, `*_returns_N_post_states*`, `*_claims_no_pda*`,
//!   `*_post_state_preserves_owner_and_nonce` — a handler no longer returns a diff per declared
//!   account. "Unchanged" is the absence of an effect, so there is nothing to assert on; a guard's
//!   apply returning `None` is the surviving form of the claim, and the runtime rejects a guard
//!   that writes.
//! * `rejects_foreign_owned_*`, `*_owned_by_other_program_panics` — a shard is keyed by the program
//!   that wrote it, so provenance is structural. Emitting an effect on `shards[self]` *is* the old
//!   ownership assertion.
//! * `rejects_uninitialized_*` on this program's own globals — those survive, but as apply tests:
//!   an empty shard does not decode, so every guard is also the initialization gate.
//!
//! What replaces them is a new family: every value that used to be read is now
//! declared, so each one has a test that it is *pinned* — `check_parameters`,
//! `check_accumulator`, `check_redemption`, `check_oracle`, `check_clock`,
//! `check_holding_balance`, and the identity comparison inside every
//! `position_after_*`.

use alloy_primitives::U512;
use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda,
    math::{mul_div, mul_div_ceil, FIXED_POINT_ONE},
    AdvanceRedemption, CheckHoldingBalance, CheckOraclePair, CollateralizationInputs,
    DepositCollateral, FeeProjection, GenerateDebt, Position, PositionIdentity, ProtocolParameters,
    RedemptionPriceState, RepayDebt, StabilityFeeAccumulator, UpdateParameters, WithdrawCollateral,
};
use token_core::{TokenDefinition, TokenDescriptor, TokenHolding, TokenKind};
use twap_oracle_core::OraclePriceAccount;

use crate::{
    accrue_stability_fee::{accrue_fee, accrue_fee_effect, plan_accrue_stability_fee},
    admin::{
        plan_set_admin, plan_set_controller_gains, plan_set_freeze_authority,
        plan_set_market_price_oracle, plan_set_minimum_collateralization_ratio,
        plan_set_stability_fee_per_millisecond, plan_set_timing_parameters, update_parameters,
    },
    checks::{
        assert_collateralization_ratio_in_band, assert_controller_gains_in_band,
        assert_position_is_collateralized, assert_stability_fee_in_band,
        assert_timing_milliseconds_in_band, MAX_COLLATERALIZATION_RATIO,
        MAX_INTEGRAL_GAIN_MAGNITUDE, MAX_PROPORTIONAL_GAIN_MAGNITUDE,
        MAX_STABILITY_FEE_PER_MILLISECOND, MAX_TIMING_MILLISECONDS, MIN_COLLATERALIZATION_RATIO,
    },
    close_position::{plan_close_position, position_after_close},
    deposit_collateral::{plan_deposit_collateral, position_after_deposit},
    freeze::{plan_set_frozen, set_frozen},
    generate_debt::{plan_generate_debt, position_after_generate},
    initialize_program::{plan_initialize_program, InitializeProgramParams},
    open_position::plan_open_position,
    refresh_globals::plan_refresh_globals,
    repay_debt::{plan_repay_debt, position_after_repay},
    shared::{
        assert_canonical_clock, check_accumulator, check_clock, check_fungible_definition,
        check_holding_absent, check_holding_balance, check_oracle, check_oracle_pair,
        check_parameters, check_redemption, create, position_of,
    },
    update_redemption_rate::{
        advance_redemption, advance_redemption_effect, plan_update_redemption_rate,
        try_advance_redemption,
    },
    withdraw_collateral::{plan_withdraw_collateral, position_after_withdraw},
};

// ─── fixtures ───────────────────────────────────────────────────────────────

const STABLECOIN_PROGRAM_ID: AccountId = AccountId::new([3u8; 32]);
const TOKEN_PROGRAM_ID: AccountId = AccountId::new([2u8; 32]);
const ORACLE_PROGRAM_ID: AccountId = AccountId::new([4u8; 32]);
/// Stands in for any program that is not the one a slot is supposed to name.
const FOREIGN_PROGRAM_ID: AccountId = AccountId::new([99u8; 32]);

/// Timestamps are Unix milliseconds, matching the `CLOCK_01` account.
const T0: u64 = 1_700_000_000_000;
/// Ten minutes after `T0` — past the 300_000 ms rate-update interval below.
const NOW: u64 = T0 + 600_000;

/// ~5% annual in per-millisecond fixed-point.
const FEE_RATE: u128 = FIXED_POINT_ONE + 1_500_000_000_000_000;
const RATE_INTERVAL: u64 = 300_000;
const ORACLE_MAX_AGE: u64 = 900_000;
/// Starting accumulator anchor: `1.0`.
const ACCUMULATOR_ANCHOR: u128 = FIXED_POINT_ONE;
/// Starting redemption price: `0.5` collateral per stablecoin.
const REDEMPTION_PRICE_ANCHOR: u128 = FIXED_POINT_ONE / 2;
/// A market price below the redemption anchor, so `error > 0` and a positive
/// `Kp` drives the rate above `FIXED_POINT_ONE`.
const MARKET_PRICE_BELOW_ANCHOR: u128 = FIXED_POINT_ONE / 4;
const MIN_RATIO: u128 = FIXED_POINT_ONE * 3 / 2;
/// `FIXED_POINT_ONE` as a controller gain. The gains are signed; this is the
/// unit gain every fixture starts from.
const UNIT_GAIN: i128 = FIXED_POINT_ONE.cast_signed();
const NONCE: u64 = 7;

fn caller_id() -> AccountId {
    AccountId::new([0xCA; 32])
}
fn admin_id() -> AccountId {
    AccountId::new([0xA0; 32])
}
fn freeze_authority_id() -> AccountId {
    AccountId::new([0xFE; 32])
}
fn owner_id() -> AccountId {
    AccountId::new([1u8; 32])
}
fn other_id() -> AccountId {
    AccountId::new([0xBB; 32])
}
fn stablecoin_definition_id() -> AccountId {
    AccountId::new([0x10; 32])
}
fn collateral_definition_id() -> AccountId {
    AccountId::new([0x20; 32])
}
fn other_definition_id() -> AccountId {
    AccountId::new([0x21; 32])
}
fn oracle_id() -> AccountId {
    AccountId::new([0x30; 32])
}
fn oracle_source_id() -> AccountId {
    AccountId::new([0x31; 32])
}
fn new_oracle_id() -> AccountId {
    AccountId::new([0x32; 32])
}
fn user_collateral_holding_id() -> AccountId {
    AccountId::new([0x40; 32])
}
fn user_stablecoin_holding_id() -> AccountId {
    AccountId::new([0x41; 32])
}

fn parameters_id() -> AccountId {
    compute_protocol_parameters_pda(STABLECOIN_PROGRAM_ID)
}
fn accumulator_id() -> AccountId {
    compute_stability_fee_accumulator_pda(STABLECOIN_PROGRAM_ID)
}
fn redemption_id() -> AccountId {
    compute_redemption_price_state_pda(STABLECOIN_PROGRAM_ID)
}
fn position_id() -> AccountId {
    compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), NONCE)
}
fn vault_id() -> AccountId {
    compute_position_vault_pda(STABLECOIN_PROGRAM_ID, position_id())
}

/// The canonical declared parameters: not frozen, 1.5x ratio, bound to the test
/// Token Program and oracle producer.
fn parameters() -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: admin_id(),
        freeze_authority_account_id: freeze_authority_id(),
        stablecoin_definition_id: stablecoin_definition_id(),
        collateral_definition_id: collateral_definition_id(),
        market_price_oracle_id: oracle_id(),
        token_program_id: TOKEN_PROGRAM_ID,
        market_price_oracle_program_id: ORACLE_PROGRAM_ID,
        stability_fee_per_millisecond: FEE_RATE,
        controller_proportional_gain: UNIT_GAIN,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: MIN_RATIO,
        minimum_milliseconds_between_rate_updates: RATE_INTERVAL,
        maximum_oracle_price_age_milliseconds: ORACLE_MAX_AGE,
        is_frozen: false,
    }
}

fn frozen_parameters() -> ProtocolParameters {
    ProtocolParameters {
        is_frozen: true,
        ..parameters()
    }
}

fn accumulator() -> StabilityFeeAccumulator {
    StabilityFeeAccumulator {
        accumulated_rate_at_last_accrual: ACCUMULATOR_ANCHOR,
        last_accrued_at: T0,
    }
}

fn redemption() -> RedemptionPriceState {
    RedemptionPriceState {
        redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR,
        redemption_rate_per_millisecond: FIXED_POINT_ONE,
        controller_integral_term: 0,
        last_updated_at: T0,
    }
}

fn oracle(price: u128, timestamp: u64) -> OraclePriceAccount {
    OraclePriceAccount {
        base_asset: stablecoin_definition_id(),
        quote_asset: collateral_definition_id(),
        price,
        timestamp,
        source_id: oracle_source_id(),
        confidence_interval: 0,
    }
}

/// An observation inside the staleness window at [`NOW`].
fn fresh_oracle() -> OraclePriceAccount {
    oracle(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000)
}

fn position(collateral_amount: u128, normalized_debt_amount: u128) -> Position {
    Position {
        owner_account_id: owner_id(),
        position_nonce: NONCE,
        vault_account_id: vault_id(),
        collateral_amount,
        normalized_debt_amount,
        opened_at: T0,
    }
}

fn identity() -> PositionIdentity {
    PositionIdentity {
        owner_account_id: owner_id(),
        position_nonce: NONCE,
        vault_account_id: vault_id(),
    }
}

fn fungible_definition() -> TokenDefinition {
    TokenDefinition::Fungible {
        name: "COLL".to_owned(),
        total_supply: 1_000_000,
        metadata_id: None,
        authority: None,
    }
}

fn holding(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible {
        definition_id,
        balance,
    })
}

fn descriptor(definition_id: AccountId) -> TokenDescriptor {
    TokenDescriptor {
        definition_id,
        kind: TokenKind::Fungible,
    }
}

fn clock_data(timestamp: u64) -> ShardData {
    ShardData::try_from(
        ClockAccountData {
            block_id: 1,
            timestamp,
        }
        .to_bytes(),
    )
    .expect("clock data fits")
}

fn empty() -> ShardData {
    ShardData::default()
}

/// A slot selecting one of this program's own shards.
fn ours(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, STABLECOIN_PROGRAM_ID)
}
/// A signed slot selecting one of this program's own shards. Signers carry no
/// data in these tests, so the program they name is irrelevant beyond being set.
fn signer(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, true, STABLECOIN_PROGRAM_ID)
}
/// A slot selecting a Token Program shard.
fn token_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, TOKEN_PROGRAM_ID)
}
/// A signed slot selecting a Token Program shard.
fn signed_token_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, true, TOKEN_PROGRAM_ID)
}
/// A slot selecting an oracle producer's shard.
fn oracle_slot(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, ORACLE_PROGRAM_ID)
}
fn clock_slot() -> AccountMeta {
    AccountMeta::new(CLOCK_01_PROGRAM_ACCOUNT_ID, false, clock_account_id())
}

// ─── PDA derivation ─────────────────────────────────────────────────────────

#[test]
fn position_pda_is_deterministic_and_owner_and_nonce_specific() {
    assert_eq!(position_id(), position_id());
    assert_ne!(
        position_id(),
        compute_position_pda(STABLECOIN_PROGRAM_ID, other_id(), NONCE)
    );
    assert_ne!(
        position_id(),
        compute_position_pda(STABLECOIN_PROGRAM_ID, owner_id(), NONCE + 1)
    );
    assert_ne!(
        position_id(),
        compute_position_pda(FOREIGN_PROGRAM_ID, owner_id(), NONCE)
    );
}

#[test]
fn position_pda_and_vault_pda_do_not_collide() {
    // Both derive from the same program with the position's address in the
    // pre-image; only the domain-separation tag differs.
    assert_ne!(position_id(), vault_id());
}

#[test]
fn the_five_global_pdas_are_all_distinct() {
    let ids = [
        parameters_id(),
        accumulator_id(),
        redemption_id(),
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID),
        compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID),
    ];
    for (i, a) in ids.iter().enumerate() {
        for b in ids.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
}

// ─── §6.2 collateralization invariant ───────────────────────────────────────

/// The projections are `U512` in production; the fixtures below are all
/// `u128`-sized, so widen them at the call boundary.
fn assert_collateralized(
    position: &Position,
    current_accumulator: u128,
    current_redemption_price: u128,
    minimum_collateralization_ratio: u128,
) {
    assert_position_is_collateralized(
        position,
        U512::from(current_accumulator),
        U512::from(current_redemption_price),
        minimum_collateralization_ratio,
    );
}

fn position_with(collateral_amount: u128, normalized_debt_amount: u128) -> Position {
    position(collateral_amount, normalized_debt_amount)
}

/// Regression for the `U256` overflow found in review: `collateral * F^2`
/// exceeds `U256` once collateral passes 115792089237316195423570, which is
/// only ~115_792 whole tokens at 18 decimals. The position below is
/// comfortably collateralized, so it must compare, not panic.
#[test]
fn very_large_collateral_does_not_overflow_the_cross_product() {
    let collateral = 115_792_089_237_316_195_423_571u128;
    assert_collateralized(
        &position_with(collateral, 1),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 11 / 10,
    );
}

#[test]
fn maximum_collateral_does_not_overflow_the_cross_product() {
    assert_collateralized(
        &position_with(u128::MAX, 1),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 11 / 10,
    );
}

/// The right-hand side has the same exposure: nominal debt is scaled by both
/// the redemption price and the ratio.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn very_large_debt_still_compares_rather_than_overflowing() {
    assert_collateralized(
        &position_with(1, 115_792_089_237_316_195_423_571),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 11 / 10,
    );
}

/// Every input at `u128::MAX` — the widest the domain allows. Pins the claim
/// that `U512` has headroom for the whole cross-product, not just for the
/// boundary case above.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn all_inputs_at_u128_max_compare_without_overflowing() {
    assert_collateralized(
        &position_with(u128::MAX, u128::MAX),
        u128::MAX,
        u128::MAX,
        u128::MAX,
    );
}

/// Flooring `normalized × accumulator / FIXED_POINT_ONE` before comparing
/// understates the debt and tilts the check toward the borrower. Here the true
/// nominal debt is 1.9, which needs 2.85 collateral at 1.5x — so 2 must fail,
/// even though a floored nominal debt of 1 would only ask for 1.5.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn fractional_nominal_debt_is_not_rounded_down() {
    assert_collateralized(
        &position_with(2, 1),
        FIXED_POINT_ONE * 19 / 10,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
fn fractional_nominal_debt_passes_once_fully_covered() {
    assert_collateralized(
        &position_with(3, 1),
        FIXED_POINT_ONE * 19 / 10,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
fn zero_debt_passes_even_with_zero_collateral() {
    assert_collateralized(
        &position_with(0, 0),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
fn zero_debt_passes_with_collateral() {
    assert_collateralized(
        &position_with(1_000_000, 0),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
fn comfortable_surplus_passes() {
    // 1 unit of debt at a 1.0 redemption price needs 1.5 collateral at a 1.5x
    // ratio; 10 is far above that.
    assert_collateralized(
        &position_with(10, 1),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
fn exact_boundary_passes() {
    // accumulator, redemption price, and ratio all 1.0, so the requirement is
    // exactly `collateral >= normalized_debt`.
    assert_collateralized(
        &position_with(100, 100),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
    );
}

#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn one_unit_below_the_boundary_fails() {
    assert_collateralized(
        &position_with(99, 100),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE,
    );
}

#[test]
fn exactly_one_and_a_half_times_collateral_passes() {
    // nominal debt 100 at a 0.5 redemption price is worth 50 in collateral
    // units; 1.5x of that is 75.
    assert_collateralized(
        &position_with(75, 100),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE / 2,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn one_unit_below_one_and_a_half_times_collateral_fails() {
    assert_collateralized(
        &position_with(74, 100),
        FIXED_POINT_ONE,
        FIXED_POINT_ONE / 2,
        FIXED_POINT_ONE * 3 / 2,
    );
}

#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn accumulator_growth_turns_a_passing_position_into_a_failing_one() {
    // 80 collateral against normalized debt 100 at a 0.5 redemption price and a
    // 1.5x ratio: needs 75 while the accumulator is 1.0, but 90 once the
    // accumulator reaches 1.2.
    let position = position_with(80, 100);

    assert_collateralized(
        &position,
        FIXED_POINT_ONE,
        FIXED_POINT_ONE / 2,
        FIXED_POINT_ONE * 3 / 2,
    );

    assert_collateralized(
        &position,
        FIXED_POINT_ONE * 12 / 10,
        FIXED_POINT_ONE / 2,
        FIXED_POINT_ONE * 3 / 2,
    );
}

/// A projection that saturated `U512` must never wave a position through.
/// The requirement saturates too, and the left-hand side tops out around
/// `10^119` — well below the ceiling — so the comparison always loses.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn a_saturated_projected_price_can_never_pass() {
    assert_position_is_collateralized(
        &position_with(u128::MAX, 1),
        U512::from(FIXED_POINT_ONE),
        U512::MAX,
        FIXED_POINT_ONE,
    );
}

/// Same for the accumulator side.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn a_saturated_projected_accumulator_can_never_pass() {
    assert_position_is_collateralized(
        &position_with(u128::MAX, 1),
        U512::MAX,
        U512::from(FIXED_POINT_ONE),
        FIXED_POINT_ONE,
    );
}

/// Saturation on the right-hand side must not turn into a panic.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn every_input_at_the_u512_ceiling_compares_rather_than_panicking() {
    assert_position_is_collateralized(
        &position_with(u128::MAX, u128::MAX),
        U512::MAX,
        U512::MAX,
        u128::MAX,
    );
}

// ─── §8 parameter bands ─────────────────────────────────────────────────────

#[test]
fn stability_fee_band_accepts_both_endpoints() {
    assert_stability_fee_in_band(FIXED_POINT_ONE, "rate");
    assert_stability_fee_in_band(MAX_STABILITY_FEE_PER_MILLISECOND, "rate");
}

#[test]
#[should_panic(expected = "rate below FIXED_POINT_ONE")]
fn stability_fee_band_rejects_below_one() {
    assert_stability_fee_in_band(FIXED_POINT_ONE - 1, "rate");
}

#[test]
#[should_panic(expected = "rate above sane upper bound")]
fn stability_fee_band_rejects_above_two() {
    assert_stability_fee_in_band(MAX_STABILITY_FEE_PER_MILLISECOND + 1, "rate");
}

#[test]
fn collateralization_ratio_band_accepts_both_endpoints() {
    assert_collateralization_ratio_in_band(MIN_COLLATERALIZATION_RATIO, "ratio");
    assert_collateralization_ratio_in_band(MAX_COLLATERALIZATION_RATIO, "ratio");
}

#[test]
#[should_panic(expected = "ratio below 1.1x")]
fn collateralization_ratio_band_rejects_below_one_point_one() {
    assert_collateralization_ratio_in_band(MIN_COLLATERALIZATION_RATIO - 1, "ratio");
}

#[test]
#[should_panic(expected = "ratio above 10x")]
fn collateralization_ratio_band_rejects_above_ten() {
    assert_collateralization_ratio_in_band(MAX_COLLATERALIZATION_RATIO + 1, "ratio");
}

#[test]
fn controller_gain_band_accepts_the_magnitude_limits_in_both_signs() {
    let kp = i128::try_from(MAX_PROPORTIONAL_GAIN_MAGNITUDE).expect("fits i128");
    let ki = i128::try_from(MAX_INTEGRAL_GAIN_MAGNITUDE).expect("fits i128");
    assert_controller_gains_in_band(kp, ki);
    assert_controller_gains_in_band(-kp, -ki);
}

#[test]
#[should_panic(expected = "controller_proportional_gain out of band")]
fn controller_gain_band_rejects_an_oversized_proportional_gain() {
    let kp = i128::try_from(MAX_PROPORTIONAL_GAIN_MAGNITUDE).expect("fits i128") + 1;
    assert_controller_gains_in_band(kp, 0);
}

#[test]
#[should_panic(expected = "controller_integral_gain out of band")]
fn controller_gain_band_rejects_an_oversized_integral_gain() {
    let ki = i128::try_from(MAX_INTEGRAL_GAIN_MAGNITUDE).expect("fits i128") + 1;
    assert_controller_gains_in_band(0, ki);
}

#[test]
fn timing_band_accepts_both_endpoints() {
    assert_timing_milliseconds_in_band(1, "interval");
    assert_timing_milliseconds_in_band(MAX_TIMING_MILLISECONDS, "interval");
}

#[test]
#[should_panic(expected = "interval below minimum 1ms")]
fn timing_band_rejects_zero() {
    assert_timing_milliseconds_in_band(0, "interval");
}

#[test]
#[should_panic(expected = "interval above maximum 86_400_000ms")]
fn timing_band_rejects_above_one_day() {
    assert_timing_milliseconds_in_band(MAX_TIMING_MILLISECONDS + 1, "interval");
}

// ─── guards: the pins that replaced the reads ───────────────────────────────

#[test]
fn canonical_clock_accepts_the_system_clock_slot() {
    assert_canonical_clock(&clock_slot());
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn canonical_clock_rejects_another_address() {
    assert_canonical_clock(&AccountMeta::new(other_id(), false, clock_account_id()));
}

/// The address alone is not enough: a shard is keyed by the program that wrote
/// it, so a slot naming `CLOCK_01` under some other program would hand
/// [`check_clock`] bytes that program authored.
#[test]
#[should_panic(expected = "Clock account must select the clock program's shard")]
fn canonical_clock_rejects_a_foreign_program_at_the_clock_address() {
    assert_canonical_clock(&AccountMeta::new(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        false,
        FOREIGN_PROGRAM_ID,
    ));
}

#[test]
fn check_clock_accepts_the_stored_timestamp() {
    check_clock(&clock_data(NOW), NOW);
}

#[test]
#[should_panic(expected = "Declared timestamp does not match the system clock")]
fn check_clock_rejects_a_declared_timestamp_that_is_not_stored() {
    check_clock(&clock_data(NOW), NOW + 1);
}

#[test]
#[should_panic(expected = "Clock account must be initialized")]
fn check_clock_rejects_an_empty_shard() {
    check_clock(&empty(), NOW);
}

#[test]
fn check_parameters_accepts_the_stored_record() {
    check_parameters(&ShardData::from(&parameters()), &parameters());
}

/// Carrying the whole record rather than a per-instruction subset is what stops
/// a caller pairing a true ratio with a forged `is_frozen`.
#[test]
#[should_panic(expected = "Declared protocol parameters do not match")]
fn check_parameters_rejects_a_forged_freeze_flag() {
    check_parameters(&ShardData::from(&frozen_parameters()), &parameters());
}

#[test]
#[should_panic(expected = "Declared protocol parameters do not match")]
fn check_parameters_rejects_a_forged_token_program_binding() {
    let forged = ProtocolParameters {
        token_program_id: FOREIGN_PROGRAM_ID,
        ..parameters()
    };
    check_parameters(&ShardData::from(&parameters()), &forged);
}

/// An empty shard means the protocol was never bootstrapped, so every guard is
/// also the initialization gate.
#[test]
#[should_panic(expected = "ProtocolParameters account must be initialized")]
fn check_parameters_rejects_an_empty_shard() {
    check_parameters(&empty(), &parameters());
}

#[test]
fn check_accumulator_accepts_the_stored_anchor() {
    check_accumulator(&ShardData::from(&accumulator()), &accumulator());
}

#[test]
#[should_panic(expected = "Declared fee accumulator does not match")]
fn check_accumulator_rejects_a_declared_anchor_that_drifted() {
    let stale = StabilityFeeAccumulator {
        last_accrued_at: T0 - 1,
        ..accumulator()
    };
    check_accumulator(&ShardData::from(&accumulator()), &stale);
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account must be initialized")]
fn check_accumulator_rejects_an_empty_shard() {
    check_accumulator(&empty(), &accumulator());
}

#[test]
fn check_redemption_accepts_the_stored_anchor() {
    check_redemption(&ShardData::from(&redemption()), &redemption());
}

#[test]
#[should_panic(expected = "Declared redemption state does not match")]
fn check_redemption_rejects_a_declared_anchor_that_drifted() {
    let stale = RedemptionPriceState {
        redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR + 1,
        ..redemption()
    };
    check_redemption(&ShardData::from(&redemption()), &stale);
}

#[test]
#[should_panic(expected = "RedemptionPriceState account must be initialized")]
fn check_redemption_rejects_an_empty_shard() {
    check_redemption(&empty(), &redemption());
}

#[test]
fn check_oracle_accepts_the_stored_observation() {
    let stored = fresh_oracle();
    check_oracle(&ShardData::from(&stored), stored.price, stored.timestamp);
}

#[test]
#[should_panic(expected = "Declared oracle observation does not match")]
fn check_oracle_rejects_a_forged_price() {
    let stored = fresh_oracle();
    check_oracle(
        &ShardData::from(&stored),
        stored.price + 1,
        stored.timestamp,
    );
}

/// The timestamp is the half `generate_debt` actually consumes, and forging it
/// upward is what would make a stale observation read as fresh.
#[test]
#[should_panic(expected = "Declared oracle observation does not match")]
fn check_oracle_rejects_a_forged_timestamp() {
    let stored = oracle(MARKET_PRICE_BELOW_ANCHOR, NOW - ORACLE_MAX_AGE - 1);
    check_oracle(&ShardData::from(&stored), stored.price, NOW);
}

#[test]
#[should_panic(expected = "Market price oracle must be initialized")]
fn check_oracle_rejects_an_empty_shard() {
    check_oracle(&empty(), MARKET_PRICE_BELOW_ANCHOR, NOW);
}

#[test]
fn check_oracle_pair_accepts_the_bound_pair() {
    check_oracle_pair(
        &ShardData::from(&fresh_oracle()),
        &CheckOraclePair {
            base_asset: stablecoin_definition_id(),
            quote_asset: collateral_definition_id(),
        },
    );
}

#[test]
#[should_panic(expected = "Oracle base_asset must equal the stablecoin definition")]
fn check_oracle_pair_rejects_a_different_base_asset() {
    check_oracle_pair(
        &ShardData::from(&fresh_oracle()),
        &CheckOraclePair {
            base_asset: other_definition_id(),
            quote_asset: collateral_definition_id(),
        },
    );
}

#[test]
#[should_panic(expected = "Oracle quote_asset must equal the collateral definition")]
fn check_oracle_pair_rejects_a_different_quote_asset() {
    check_oracle_pair(
        &ShardData::from(&fresh_oracle()),
        &CheckOraclePair {
            base_asset: stablecoin_definition_id(),
            quote_asset: other_definition_id(),
        },
    );
}

#[test]
fn check_fungible_definition_accepts_a_fungible_definition() {
    check_fungible_definition(&ShardData::from(&fungible_definition()));
}

#[test]
#[should_panic(expected = "Collateral definition must be Fungible")]
fn check_fungible_definition_rejects_a_non_fungible_definition() {
    check_fungible_definition(&ShardData::from(&TokenDefinition::NonFungible {
        name: "NFT".to_owned(),
        printable_supply: 1,
        metadata_id: other_id(),
    }));
}

#[test]
#[should_panic(expected = "Collateral definition must be initialized")]
fn check_fungible_definition_rejects_an_empty_shard() {
    check_fungible_definition(&empty());
}

#[test]
fn check_holding_balance_accepts_the_stored_balance() {
    check_holding_balance(
        &holding(collateral_definition_id(), 500),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 500,
        },
    );
}

#[test]
#[should_panic(expected = "Declared vault balance does not match")]
fn check_holding_balance_rejects_a_forged_balance() {
    check_holding_balance(
        &holding(collateral_definition_id(), 500),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 499,
        },
    );
}

#[test]
#[should_panic(expected = "Vault holding does not match the protocol's collateral definition")]
fn check_holding_balance_rejects_a_holding_for_another_definition() {
    check_holding_balance(
        &holding(other_definition_id(), 500),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 500,
        },
    );
}

#[test]
#[should_panic(expected = "Vault must hold a fungible collateral balance")]
fn check_holding_balance_rejects_a_non_fungible_holding() {
    check_holding_balance(
        &ShardData::from(&TokenHolding::NftMaster {
            definition_id: collateral_definition_id(),
            print_balance: 1,
        }),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 1,
        },
    );
}

#[test]
#[should_panic(expected = "Vault account must be initialized")]
fn check_holding_balance_rejects_an_empty_shard() {
    check_holding_balance(
        &empty(),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 0,
        },
    );
}

#[test]
fn check_holding_absent_accepts_an_empty_shard() {
    check_holding_absent(&empty());
}

#[test]
#[should_panic(expected = "Position vault account must be uninitialized")]
fn check_holding_absent_rejects_an_existing_holding() {
    check_holding_absent(&holding(collateral_definition_id(), 0));
}

// ─── shared writes ──────────────────────────────────────────────────────────

#[test]
fn create_writes_into_an_empty_shard() {
    let data = ShardData::from(&parameters());
    assert_eq!(create(&empty(), data.clone()), data);
}

/// `create` is deliberately strict rather than `write_once`: the five globals
/// and every position are one-shot, so a second write is a caller error, not an
/// idempotent retry.
#[test]
#[should_panic(expected = "Target account must not already hold data")]
fn create_refuses_to_write_a_populated_shard() {
    let _ = create(
        &ShardData::from(&parameters()),
        ShardData::from(&redemption()),
    );
}

#[test]
fn position_of_accepts_the_stored_identity() {
    let stored = position(1_000, 10);
    assert_eq!(position_of(&ShardData::from(&stored), &identity()), stored);
}

/// `position_nonce` is declared rather than read now. The plan's address
/// assertion is what checks it; this guard catches a stored `owner_account_id`
/// or `vault_account_id` that has drifted out of step with it.
#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_of_rejects_a_stale_owner_field() {
    let stored = Position {
        owner_account_id: other_id(),
        ..position(1_000, 10)
    };
    position_of(&ShardData::from(&stored), &identity());
}

#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_of_rejects_a_stale_vault_field() {
    let stored = Position {
        vault_account_id: other_id(),
        ..position(1_000, 10)
    };
    position_of(&ShardData::from(&stored), &identity());
}

#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_of_rejects_a_declared_nonce_the_record_disagrees_with() {
    let stored = Position {
        position_nonce: NONCE + 1,
        ..position(1_000, 10)
    };
    position_of(&ShardData::from(&stored), &identity());
}

#[test]
#[should_panic(expected = "Position account must be initialized")]
fn position_of_rejects_an_empty_shard() {
    position_of(&empty(), &identity());
}

// ─── InitializeProgram ──────────────────────────────────────────────────────

fn ok_params() -> InitializeProgramParams<'static> {
    InitializeProgramParams {
        freeze_authority_account_id: freeze_authority_id(),
        initial_stability_fee_per_millisecond: FEE_RATE,
        initial_controller_proportional_gain: UNIT_GAIN,
        initial_controller_integral_gain: 0,
        initial_minimum_collateralization_ratio: MIN_RATIO,
        minimum_milliseconds_between_rate_updates: RATE_INTERVAL,
        maximum_oracle_price_age_milliseconds: ORACLE_MAX_AGE,
        initial_redemption_price: REDEMPTION_PRICE_ANCHOR,
        stablecoin_name: "LSD",
    }
}

type InitPlan = (
    crate::initialize_program::Globals,
    CheckOraclePair,
    lee_core::program::ChainedCall,
);

fn plan_init(params: &InitializeProgramParams<'_>) -> InitPlan {
    plan_initialize_program(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        params,
        STABLECOIN_PROGRAM_ID,
    )
}

#[test]
fn initialize_program_records_every_binding_the_later_instructions_check() {
    let (globals, pair, _) = plan_init(&ok_params());
    let p = globals.parameters;
    assert_eq!(p.admin_account_id, admin_id());
    assert_eq!(p.freeze_authority_account_id, freeze_authority_id());
    assert_eq!(
        p.stablecoin_definition_id,
        compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)
    );
    assert_eq!(p.collateral_definition_id, collateral_definition_id());
    assert_eq!(p.market_price_oracle_id, oracle_id());
    assert!(!p.is_frozen);
    assert_eq!(
        pair,
        CheckOraclePair {
            base_asset: p.stablecoin_definition_id,
            quote_asset: collateral_definition_id(),
        }
    );
}

/// The two bindings v0.2.5 did not keep. Without them a caller chooses, per
/// call, whose shard a token or oracle slot selects — and so chooses whose bytes
/// a burn or an observation is checked against.
#[test]
fn initialize_program_binds_the_token_program_and_oracle_producer_from_their_slots() {
    let (globals, _, _) = plan_init(&ok_params());
    assert_eq!(globals.parameters.token_program_id, TOKEN_PROGRAM_ID);
    assert_eq!(
        globals.parameters.market_price_oracle_program_id,
        ORACLE_PROGRAM_ID
    );
}

#[test]
fn initialize_program_anchors_both_globals_at_the_clock() {
    let (globals, _, _) = plan_init(&ok_params());
    assert_eq!(
        globals.accumulator,
        StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: NOW,
        }
    );
    assert_eq!(
        globals.redemption,
        RedemptionPriceState {
            redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: NOW,
        }
    );
}

#[test]
fn initialize_program_chains_a_zero_supply_definition_with_two_seeds() {
    let (_, _, call) = plan_init(&ok_params());
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    assert_eq!(call.pda_seeds.len(), 2);
    let instruction: token_core::Instruction =
        borsh::from_slice(&call.instruction_data).expect("decodes as a Token instruction");
    match instruction {
        token_core::Instruction::NewFungibleDefinition {
            name,
            total_supply,
            mint_authority,
        } => {
            assert_eq!(name, "LSD");
            assert_eq!(total_supply, 0);
            // Self/PDA authority: the definition is its own mint authority, so
            // `generate_debt` can mint by presenting the definition's seed.
            assert_eq!(
                mint_authority,
                Some(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID))
            );
        }
        _ => panic!("expected a NewFungibleDefinition instruction"),
    }
}

/// The three globals are written through `create`, which is where "must be
/// uninitialized" now lives — v0.2.5's five `rejects_initialized_*` tests
/// collapse into this one plus `create_refuses_to_write_a_populated_shard`.
#[test]
fn initialize_program_globals_are_written_through_create() {
    let (globals, _, _) = plan_init(&ok_params());
    let [p, a, r] = globals.shard_data();
    assert_eq!(create(&empty(), p), ShardData::from(&globals.parameters));
    assert_eq!(create(&empty(), a), ShardData::from(&globals.accumulator));
    assert_eq!(create(&empty(), r), ShardData::from(&globals.redemption));
}

#[test]
#[should_panic(expected = "Admin authorization is missing")]
fn initialize_program_requires_admin_authorization() {
    let _ = plan_initialize_program(
        &ours(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn initialize_program_rejects_a_wrong_parameters_pda() {
    let _ = plan_initialize_program(
        &signer(admin_id()),
        &ours(other_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn initialize_program_rejects_a_wrong_accumulator_pda() {
    let _ = plan_initialize_program(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(other_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "StablecoinDefinition account ID does not match")]
fn initialize_program_rejects_a_wrong_definition_pda() {
    let _ = plan_initialize_program(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(other_id()),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "StablecoinMasterHolding account ID does not match")]
fn initialize_program_rejects_a_wrong_master_holding_pda() {
    let _ = plan_initialize_program(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(other_id()),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn initialize_program_rejects_a_wrong_clock_account() {
    let _ = plan_initialize_program(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(compute_stablecoin_definition_pda(STABLECOIN_PROGRAM_ID)),
        &ours(compute_stablecoin_master_holding_pda(STABLECOIN_PROGRAM_ID)),
        &token_slot(collateral_definition_id()),
        &oracle_slot(oracle_id()),
        &AccountMeta::new(other_id(), false, clock_account_id()),
        NOW,
        &ok_params(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "initial_stability_fee_per_millisecond below FIXED_POINT_ONE")]
fn initialize_program_rejects_a_stability_fee_below_the_band() {
    plan_init(&InitializeProgramParams {
        initial_stability_fee_per_millisecond: FIXED_POINT_ONE - 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "initial_stability_fee_per_millisecond above sane upper bound")]
fn initialize_program_rejects_a_stability_fee_above_the_band() {
    plan_init(&InitializeProgramParams {
        initial_stability_fee_per_millisecond: MAX_STABILITY_FEE_PER_MILLISECOND + 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "initial_minimum_collateralization_ratio below 1.1x")]
fn initialize_program_rejects_a_ratio_below_the_band() {
    plan_init(&InitializeProgramParams {
        initial_minimum_collateralization_ratio: MIN_COLLATERALIZATION_RATIO - 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "initial_minimum_collateralization_ratio above 10x")]
fn initialize_program_rejects_a_ratio_above_the_band() {
    plan_init(&InitializeProgramParams {
        initial_minimum_collateralization_ratio: MAX_COLLATERALIZATION_RATIO + 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "controller_proportional_gain out of band")]
fn initialize_program_rejects_an_oversized_proportional_gain() {
    plan_init(&InitializeProgramParams {
        initial_controller_proportional_gain: i128::try_from(MAX_PROPORTIONAL_GAIN_MAGNITUDE)
            .expect("fits i128")
            + 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "minimum_milliseconds_between_rate_updates below minimum 1ms")]
fn initialize_program_rejects_a_zero_rate_update_interval() {
    plan_init(&InitializeProgramParams {
        minimum_milliseconds_between_rate_updates: 0,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "maximum_oracle_price_age_milliseconds above maximum 86_400_000ms")]
fn initialize_program_rejects_an_oracle_max_age_above_a_day() {
    plan_init(&InitializeProgramParams {
        maximum_oracle_price_age_milliseconds: MAX_TIMING_MILLISECONDS + 1,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "initial_redemption_price must be positive")]
fn initialize_program_rejects_a_zero_initial_redemption_price() {
    plan_init(&InitializeProgramParams {
        initial_redemption_price: 0,
        ..ok_params()
    });
}

#[test]
#[should_panic(expected = "stablecoin_name must be non-empty")]
fn initialize_program_rejects_an_empty_stablecoin_name() {
    plan_init(&InitializeProgramParams {
        stablecoin_name: "",
        ..ok_params()
    });
}

// ─── AccrueStabilityFee ─────────────────────────────────────────────────────

fn plan_accrue(caller: &AccountMeta, accumulator_slot: &AccountMeta, clock: &AccountMeta) {
    plan_accrue_stability_fee(
        caller,
        &ours(parameters_id()),
        accumulator_slot,
        clock,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn plan_accrue_stability_fee_accepts_the_canonical_accounts() {
    plan_accrue(&signer(caller_id()), &ours(accumulator_id()), &clock_slot());
}

#[test]
#[should_panic(expected = "Caller authorization is missing")]
fn plan_accrue_stability_fee_requires_caller_authorization() {
    plan_accrue(&ours(caller_id()), &ours(accumulator_id()), &clock_slot());
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn plan_accrue_stability_fee_rejects_a_wrong_accumulator_pda() {
    plan_accrue(&signer(caller_id()), &ours(other_id()), &clock_slot());
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn plan_accrue_stability_fee_rejects_a_wrong_clock_account() {
    plan_accrue(
        &signer(caller_id()),
        &ours(accumulator_id()),
        &AccountMeta::new(other_id(), false, clock_account_id()),
    );
}

#[test]
fn accrue_fee_advances_the_anchor_by_the_projection() {
    let effect = accrue_fee_effect(FEE_RATE, NOW);
    let after =
        StabilityFeeAccumulator::try_from(&accrue_fee(&ShardData::from(&accumulator()), &effect))
            .unwrap();
    assert_eq!(after.last_accrued_at, NOW);
    assert!(after.accumulated_rate_at_last_accrual > ACCUMULATOR_ANCHOR);
    assert_eq!(
        after,
        FeeProjection {
            accumulator: accumulator(),
            stability_fee_per_millisecond: FEE_RATE,
            now: NOW,
        }
        .advanced()
    );
}

/// Permissionless and unthrottled: a redundant poke is a restamp, not an error.
#[test]
fn accrue_fee_with_zero_elapsed_time_is_a_noop_restamp() {
    let after = accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FEE_RATE, T0),
    );
    assert_eq!(after, ShardData::from(&accumulator()));
}

#[test]
fn accrue_fee_at_a_unit_rate_leaves_the_anchor_untouched() {
    let after = StabilityFeeAccumulator::try_from(&accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FIXED_POINT_ONE, NOW),
    ))
    .unwrap();
    assert_eq!(after.accumulated_rate_at_last_accrual, ACCUMULATOR_ANCHOR);
    assert_eq!(after.last_accrued_at, NOW);
}

/// `SetStabilityFeePerMillisecond` is why the rate travels in the effect rather
/// than being read: by the time the accumulator's apply runs, the stored rate is
/// already the new one, and the accrual must use the old.
#[test]
fn accrue_fee_uses_the_declared_rate_not_the_stored_parameters() {
    let at_old = accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FEE_RATE, NOW),
    );
    let at_new = accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FEE_RATE * 2 - FIXED_POINT_ONE, NOW),
    );
    assert_ne!(at_old, at_new);
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account must be initialized")]
fn accrue_fee_rejects_an_empty_shard() {
    let _ = accrue_fee(&empty(), &accrue_fee_effect(FEE_RATE, NOW));
}

// ─── UpdateRedemptionRate ───────────────────────────────────────────────────

fn plan_update_rate(
    caller: &AccountMeta,
    redemption_slot: &AccountMeta,
    oracle: &AccountMeta,
    clock: &AccountMeta,
) {
    plan_update_redemption_rate(
        caller,
        &ours(parameters_id()),
        redemption_slot,
        oracle,
        clock,
        &parameters(),
        STABLECOIN_PROGRAM_ID,
    );
}

fn advance_effect(price: u128, timestamp: u64, now: u64) -> AdvanceRedemption {
    advance_redemption_effect(&parameters(), price, timestamp, now)
}

#[test]
fn plan_update_redemption_rate_accepts_the_canonical_accounts() {
    plan_update_rate(
        &signer(caller_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
    );
}

#[test]
#[should_panic(expected = "Caller authorization is missing")]
fn plan_update_redemption_rate_requires_caller_authorization() {
    plan_update_rate(
        &ours(caller_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
    );
}

#[test]
#[should_panic(expected = "RedemptionPriceState account ID does not match")]
fn plan_update_redemption_rate_rejects_a_wrong_redemption_pda() {
    plan_update_rate(
        &signer(caller_id()),
        &ours(other_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
    );
}

#[test]
#[should_panic(expected = "Market price oracle account_id does not match")]
fn plan_update_redemption_rate_rejects_an_unbound_oracle_address() {
    plan_update_rate(
        &signer(caller_id()),
        &ours(redemption_id()),
        &oracle_slot(new_oracle_id()),
        &clock_slot(),
    );
}

/// The address alone is not enough any more: a caller who may name the program a
/// slot selects can point the bound address at a shard they authored.
#[test]
#[should_panic(expected = "Market price oracle must select the bound producer's shard")]
fn plan_update_redemption_rate_rejects_a_foreign_program_at_the_bound_address() {
    plan_update_rate(
        &signer(caller_id()),
        &ours(redemption_id()),
        &AccountMeta::new(oracle_id(), false, FOREIGN_PROGRAM_ID),
        &clock_slot(),
    );
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn plan_update_redemption_rate_rejects_a_wrong_clock_account() {
    plan_update_rate(
        &signer(caller_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &AccountMeta::new(other_id(), false, clock_account_id()),
    );
}

#[test]
fn advance_redemption_re_anchors_at_the_projected_price() {
    let after = RedemptionPriceState::try_from(&advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW),
    ))
    .unwrap();
    // The stored rate is exactly 1.0, so the projection re-anchors at the same
    // price; the controller tick is what moves the rate.
    assert_eq!(
        after.redemption_price_at_last_update,
        REDEMPTION_PRICE_ANCHOR
    );
    assert_eq!(after.last_updated_at, NOW);
}

#[test]
fn a_market_price_below_the_redemption_price_drives_the_rate_above_one() {
    let after = RedemptionPriceState::try_from(&advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW),
    ))
    .unwrap();
    assert!(after.redemption_rate_per_millisecond > FIXED_POINT_ONE);
}

#[test]
fn a_market_price_above_the_redemption_price_drives_the_rate_below_one() {
    let after = RedemptionPriceState::try_from(&advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(REDEMPTION_PRICE_ANCHOR * 2, NOW - 1_000, NOW),
    ))
    .unwrap();
    assert!(after.redemption_rate_per_millisecond < FIXED_POINT_ONE);
}

#[test]
fn the_controller_carries_its_prior_integral_term_forward() {
    let seeded = RedemptionPriceState {
        controller_integral_term: 12_345,
        ..redemption()
    };
    let parameters_with_ki = ProtocolParameters {
        controller_integral_gain: UNIT_GAIN,
        ..parameters()
    };
    let effect = advance_redemption_effect(
        &parameters_with_ki,
        MARKET_PRICE_BELOW_ANCHOR,
        NOW - 1_000,
        NOW,
    );
    let after =
        RedemptionPriceState::try_from(&advance_redemption(&ShardData::from(&seeded), &effect))
            .unwrap();
    assert_ne!(after.controller_integral_term, 0);
    assert_ne!(after.controller_integral_term, 12_345);
}

#[test]
#[should_panic(expected = "Market price oracle observation is stale")]
fn advance_redemption_rejects_a_stale_observation() {
    let _ = advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - ORACLE_MAX_AGE - 1, NOW),
    );
}

#[test]
#[should_panic(expected = "Market price oracle reports zero price")]
fn advance_redemption_rejects_a_zero_price() {
    let _ = advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(0, NOW - 1_000, NOW),
    );
}

/// The one gate no plan could ever check: it needs `last_updated_at`, which
/// lives in the shard being rewritten.
#[test]
#[should_panic(expected = "called too soon since last update")]
fn advance_redemption_rejects_a_call_before_the_minimum_interval() {
    let _ = advance_redemption(
        &ShardData::from(&redemption()),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, T0, T0 + RATE_INTERVAL - 1),
    );
}

#[test]
#[should_panic(expected = "RedemptionPriceState account must be initialized")]
fn advance_redemption_rejects_an_empty_shard() {
    let _ = advance_redemption(
        &empty(),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW),
    );
}

// ─── RefreshGlobals ─────────────────────────────────────────────────────────

#[test]
fn plan_refresh_globals_accepts_the_canonical_accounts() {
    plan_refresh_globals(
        &signer(caller_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        &parameters(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Caller authorization is missing")]
fn plan_refresh_globals_requires_caller_authorization() {
    plan_refresh_globals(
        &ours(caller_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        &parameters(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn plan_refresh_globals_rejects_a_wrong_accumulator_pda() {
    plan_refresh_globals(
        &signer(caller_id()),
        &ours(parameters_id()),
        &ours(other_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &clock_slot(),
        &parameters(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Market price oracle account_id does not match")]
fn plan_refresh_globals_rejects_an_unbound_oracle() {
    plan_refresh_globals(
        &signer(caller_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(new_oracle_id()),
        &clock_slot(),
        &parameters(),
        STABLECOIN_PROGRAM_ID,
    );
}

/// The best-effort half: both effects are emitted unconditionally, and the skip
/// is `try_advance_redemption` returning `None`.
#[test]
fn both_halves_run_when_the_interval_is_due_and_the_oracle_is_fresh() {
    let effect = advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW);
    assert!(try_advance_redemption(&ShardData::from(&redemption()), &effect).is_some());
    assert_ne!(
        accrue_fee(
            &ShardData::from(&accumulator()),
            &accrue_fee_effect(FEE_RATE, NOW)
        ),
        ShardData::from(&accumulator())
    );
}

/// The two pokes must not drift apart: the same effect drives both, and where
/// the strict one writes, the best-effort one writes identically.
#[test]
fn the_best_effort_half_matches_the_strict_one_byte_for_byte() {
    let effect = advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW);
    let pre = ShardData::from(&redemption());
    assert_eq!(
        try_advance_redemption(&pre, &effect),
        Some(advance_redemption(&pre, &effect))
    );
}

#[test]
fn the_redemption_half_skips_when_the_interval_has_not_elapsed() {
    let effect = advance_effect(MARKET_PRICE_BELOW_ANCHOR, T0, T0 + RATE_INTERVAL - 1);
    assert_eq!(
        try_advance_redemption(&ShardData::from(&redemption()), &effect),
        None
    );
}

#[test]
fn the_redemption_half_skips_when_the_oracle_is_stale() {
    let effect = advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - ORACLE_MAX_AGE - 1, NOW);
    assert_eq!(
        try_advance_redemption(&ShardData::from(&redemption()), &effect),
        None
    );
}

#[test]
fn the_redemption_half_skips_when_the_oracle_price_is_zero() {
    let effect = advance_effect(0, NOW - 1_000, NOW);
    assert_eq!(
        try_advance_redemption(&ShardData::from(&redemption()), &effect),
        None
    );
}

/// A skip is a decision not to write, so the fee half still has to run — which
/// it does, because the plan emits both effects regardless.
#[test]
fn the_fee_half_still_runs_when_the_redemption_half_skips() {
    let skipped = advance_effect(0, NOW - 1_000, NOW);
    assert_eq!(
        try_advance_redemption(&ShardData::from(&redemption()), &skipped),
        None
    );
    let accrued = StabilityFeeAccumulator::try_from(&accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FEE_RATE, NOW),
    ))
    .unwrap();
    assert_eq!(accrued.last_accrued_at, NOW);
}

#[test]
#[should_panic(expected = "RedemptionPriceState account must be initialized")]
fn try_advance_redemption_still_rejects_an_empty_shard() {
    // A skip is for a soft gate; an undecodable shard is not one.
    let _ = try_advance_redemption(
        &empty(),
        &advance_effect(MARKET_PRICE_BELOW_ANCHOR, NOW - 1_000, NOW),
    );
}

// ─── Admin setters ──────────────────────────────────────────────────────────

/// Apply the setter's effect to the canonical stored record.
fn applied(effect: &UpdateParameters) -> ProtocolParameters {
    ProtocolParameters::try_from(&update_parameters(&ShardData::from(&parameters()), effect))
        .unwrap()
}

#[test]
fn set_minimum_collateralization_ratio_writes_only_that_field() {
    let effect = plan_set_minimum_collateralization_ratio(
        &signer(admin_id()),
        &ours(parameters_id()),
        FIXED_POINT_ONE * 2,
        STABLECOIN_PROGRAM_ID,
    );
    assert_eq!(
        applied(&effect),
        ProtocolParameters {
            minimum_collateralization_ratio: FIXED_POINT_ONE * 2,
            ..parameters()
        }
    );
}

#[test]
fn set_minimum_collateralization_ratio_accepts_both_band_endpoints() {
    for ratio in [MIN_COLLATERALIZATION_RATIO, MAX_COLLATERALIZATION_RATIO] {
        let effect = plan_set_minimum_collateralization_ratio(
            &signer(admin_id()),
            &ours(parameters_id()),
            ratio,
            STABLECOIN_PROGRAM_ID,
        );
        assert_eq!(applied(&effect).minimum_collateralization_ratio, ratio);
    }
}

#[test]
#[should_panic(expected = "below 1.1x")]
fn set_minimum_collateralization_ratio_rejects_below_the_band() {
    let _ = plan_set_minimum_collateralization_ratio(
        &signer(admin_id()),
        &ours(parameters_id()),
        MIN_COLLATERALIZATION_RATIO - 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "above 10x")]
fn set_minimum_collateralization_ratio_rejects_above_the_band() {
    let _ = plan_set_minimum_collateralization_ratio(
        &signer(admin_id()),
        &ours(parameters_id()),
        MAX_COLLATERALIZATION_RATIO + 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn set_controller_gains_writes_both_and_preserves_the_integral_term() {
    let effect = plan_set_controller_gains(
        &signer(admin_id()),
        &ours(parameters_id()),
        -7,
        11,
        STABLECOIN_PROGRAM_ID,
    );
    let after = applied(&effect);
    assert_eq!(after.controller_proportional_gain, -7);
    assert_eq!(after.controller_integral_gain, 11);
    // The integral term lives in `RedemptionPriceState`, which this setter does
    // not touch at all — retuning the gains deliberately does not reset it.
    assert_eq!(
        after,
        ProtocolParameters {
            controller_proportional_gain: -7,
            controller_integral_gain: 11,
            ..parameters()
        }
    );
}

#[test]
#[should_panic(expected = "controller_proportional_gain out of band")]
fn set_controller_gains_rejects_an_oversized_proportional_gain() {
    let _ = plan_set_controller_gains(
        &signer(admin_id()),
        &ours(parameters_id()),
        i128::try_from(MAX_PROPORTIONAL_GAIN_MAGNITUDE).unwrap() + 1,
        0,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "controller_integral_gain out of band")]
fn set_controller_gains_rejects_an_oversized_integral_gain() {
    let _ = plan_set_controller_gains(
        &signer(admin_id()),
        &ours(parameters_id()),
        0,
        i128::try_from(MAX_INTEGRAL_GAIN_MAGNITUDE).unwrap() + 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn set_timing_parameters_writes_both_fields() {
    let effect = plan_set_timing_parameters(
        &signer(admin_id()),
        &ours(parameters_id()),
        1_000,
        2_000,
        STABLECOIN_PROGRAM_ID,
    );
    assert_eq!(
        applied(&effect),
        ProtocolParameters {
            minimum_milliseconds_between_rate_updates: 1_000,
            maximum_oracle_price_age_milliseconds: 2_000,
            ..parameters()
        }
    );
}

#[test]
#[should_panic(expected = "below minimum 1ms")]
fn set_timing_parameters_rejects_a_zero_interval() {
    let _ = plan_set_timing_parameters(
        &signer(admin_id()),
        &ours(parameters_id()),
        0,
        ORACLE_MAX_AGE,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "above maximum 86_400_000ms")]
fn set_timing_parameters_rejects_an_oversized_staleness_window() {
    let _ = plan_set_timing_parameters(
        &signer(admin_id()),
        &ours(parameters_id()),
        RATE_INTERVAL,
        MAX_TIMING_MILLISECONDS + 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn set_admin_rotates_the_handle_in_one_step() {
    let effect = plan_set_admin(
        &signer(admin_id()),
        &ours(parameters_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let after = applied(&effect);
    assert_eq!(after.admin_account_id, other_id());
    // Effective immediately: the old admin's next setter fails the stored-admin
    // gate against the rotated record.
    let next = plan_set_admin(
        &signer(admin_id()),
        &ours(parameters_id()),
        admin_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let rotated = ShardData::from(&after);
    assert!(std::panic::catch_unwind(|| update_parameters(&rotated, &next)).is_err());
}

#[test]
fn set_freeze_authority_rotates_the_handle() {
    let effect = plan_set_freeze_authority(
        &signer(admin_id()),
        &ours(parameters_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
    assert_eq!(
        applied(&effect),
        ProtocolParameters {
            freeze_authority_account_id: other_id(),
            ..parameters()
        }
    );
}

#[test]
fn set_market_price_oracle_rotates_both_the_address_and_the_producer() {
    let effect = plan_set_market_price_oracle(
        &signer(admin_id()),
        &ours(parameters_id()),
        &AccountMeta::new(new_oracle_id(), false, FOREIGN_PROGRAM_ID),
        stablecoin_definition_id(),
        collateral_definition_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let after = applied(&effect);
    assert_eq!(after.market_price_oracle_id, new_oracle_id());
    // The producing program is unconstrained — any well-formed feed works — but
    // it is now bound by the admin here rather than chosen per call later.
    assert_eq!(after.market_price_oracle_program_id, FOREIGN_PROGRAM_ID);
}

/// Half of the pair check: the declared pair must be the stored definitions.
/// The other half is `check_oracle_pair` on the replacement's own shard, and
/// neither apply sees both accounts.
#[test]
#[should_panic(expected = "New oracle base_asset must equal the stablecoin definition")]
fn set_market_price_oracle_rejects_a_declared_base_asset_that_is_not_bound() {
    let effect = plan_set_market_price_oracle(
        &signer(admin_id()),
        &ours(parameters_id()),
        &oracle_slot(new_oracle_id()),
        other_definition_id(),
        collateral_definition_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let _ = update_parameters(&ShardData::from(&parameters()), &effect);
}

#[test]
#[should_panic(expected = "New oracle quote_asset must equal the collateral definition")]
fn set_market_price_oracle_rejects_a_declared_quote_asset_that_is_not_bound() {
    let effect = plan_set_market_price_oracle(
        &signer(admin_id()),
        &ours(parameters_id()),
        &oracle_slot(new_oracle_id()),
        stablecoin_definition_id(),
        other_definition_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let _ = update_parameters(&ShardData::from(&parameters()), &effect);
}

#[test]
fn set_stability_fee_writes_the_new_rate_and_pins_the_old_one() {
    let effect = plan_set_stability_fee_per_millisecond(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &clock_slot(),
        FEE_RATE,
        FIXED_POINT_ONE,
        STABLECOIN_PROGRAM_ID,
    );
    assert_eq!(
        applied(&effect),
        ProtocolParameters {
            stability_fee_per_millisecond: FIXED_POINT_ONE,
            ..parameters()
        }
    );
}

/// The accrual runs at the *old* rate, so the new one is never retroactive.
/// `old_rate` is declared because the accumulator's apply cannot see the
/// parameters shard; the parameters apply is what pins it.
#[test]
fn set_stability_fee_accrues_the_elapsed_gap_at_the_old_rate() {
    let at_old_rate = accrue_fee(
        &ShardData::from(&accumulator()),
        &accrue_fee_effect(FEE_RATE, NOW),
    );
    assert_eq!(
        at_old_rate,
        ShardData::from(
            &FeeProjection {
                accumulator: accumulator(),
                stability_fee_per_millisecond: FEE_RATE,
                now: NOW,
            }
            .advanced()
        )
    );
}

#[test]
#[should_panic(expected = "Declared current stability fee does not match")]
fn set_stability_fee_rejects_a_declared_old_rate_that_is_not_stored() {
    let effect = plan_set_stability_fee_per_millisecond(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &clock_slot(),
        FEE_RATE + 1,
        FIXED_POINT_ONE,
        STABLECOIN_PROGRAM_ID,
    );
    let _ = update_parameters(&ShardData::from(&parameters()), &effect);
}

#[test]
fn set_stability_fee_accepts_both_band_endpoints() {
    for rate in [FIXED_POINT_ONE, MAX_STABILITY_FEE_PER_MILLISECOND] {
        let effect = plan_set_stability_fee_per_millisecond(
            &signer(admin_id()),
            &ours(parameters_id()),
            &ours(accumulator_id()),
            &clock_slot(),
            FEE_RATE,
            rate,
            STABLECOIN_PROGRAM_ID,
        );
        assert_eq!(applied(&effect).stability_fee_per_millisecond, rate);
    }
}

#[test]
#[should_panic(expected = "new_stability_fee_per_millisecond below FIXED_POINT_ONE")]
fn set_stability_fee_rejects_a_rate_below_the_band() {
    let _ = plan_set_stability_fee_per_millisecond(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &clock_slot(),
        FEE_RATE,
        FIXED_POINT_ONE - 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "new_stability_fee_per_millisecond above sane upper bound")]
fn set_stability_fee_rejects_a_rate_above_the_band() {
    let _ = plan_set_stability_fee_per_millisecond(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(accumulator_id()),
        &clock_slot(),
        FEE_RATE,
        MAX_STABILITY_FEE_PER_MILLISECOND + 1,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn set_stability_fee_rejects_the_accumulator_at_the_wrong_address() {
    let _ = plan_set_stability_fee_per_millisecond(
        &signer(admin_id()),
        &ours(parameters_id()),
        &ours(other_id()),
        &clock_slot(),
        FEE_RATE,
        FIXED_POINT_ONE,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Admin authorization is missing")]
fn an_admin_setter_requires_the_admin_to_sign() {
    let _ = plan_set_admin(
        &ours(admin_id()),
        &ours(parameters_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn an_admin_setter_rejects_parameters_at_the_wrong_address() {
    let _ = plan_set_admin(
        &signer(admin_id()),
        &ours(other_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
}

/// Which account *is* the admin is stored in the shard and nowhere else, so the
/// plan can only report who claims the role; the apply does the comparison.
#[test]
#[should_panic(expected = "Signer is not the protocol's admin")]
fn an_admin_setter_rejects_a_signer_other_than_the_stored_admin() {
    let effect = plan_set_admin(
        &signer(freeze_authority_id()),
        &ours(parameters_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let _ = update_parameters(&ShardData::from(&parameters()), &effect);
}

#[test]
#[should_panic(expected = "ProtocolParameters account must be initialized")]
fn an_admin_setter_rejects_an_empty_parameters_shard() {
    let effect = plan_set_admin(
        &signer(admin_id()),
        &ours(parameters_id()),
        other_id(),
        STABLECOIN_PROGRAM_ID,
    );
    let _ = update_parameters(&empty(), &effect);
}

// ─── Freeze / Unfreeze ──────────────────────────────────────────────────────

fn frozen(is_frozen: bool, signed_by: AccountId, pre: &ProtocolParameters) -> ProtocolParameters {
    let effect = plan_set_frozen(
        &signer(signed_by),
        &ours(parameters_id()),
        is_frozen,
        STABLECOIN_PROGRAM_ID,
    );
    ProtocolParameters::try_from(&set_frozen(&ShardData::from(pre), &effect)).unwrap()
}

#[test]
fn freeze_sets_the_flag_and_touches_nothing_else() {
    assert_eq!(
        frozen(true, freeze_authority_id(), &parameters()),
        frozen_parameters()
    );
}

#[test]
fn unfreeze_clears_the_flag_and_touches_nothing_else() {
    assert_eq!(
        frozen(false, freeze_authority_id(), &frozen_parameters()),
        parameters()
    );
}

#[test]
fn freeze_is_idempotent_in_both_directions() {
    assert_eq!(
        frozen(true, freeze_authority_id(), &frozen_parameters()),
        frozen_parameters()
    );
    assert_eq!(
        frozen(false, freeze_authority_id(), &parameters()),
        parameters()
    );
}

#[test]
#[should_panic(expected = "Freeze authority authorization is missing")]
fn freeze_requires_the_authority_to_sign() {
    let _ = plan_set_frozen(
        &ours(freeze_authority_id()),
        &ours(parameters_id()),
        true,
        STABLECOIN_PROGRAM_ID,
    );
}

/// The gate is a different handle from the admin's — rotating the freeze
/// authority is an *admin* action, so a compromised freeze authority cannot
/// entrench itself.
#[test]
#[should_panic(expected = "Signer is not the protocol's freeze authority")]
fn freeze_rejects_a_signer_other_than_the_freeze_authority() {
    frozen(true, admin_id(), &parameters());
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn freeze_rejects_parameters_at_the_wrong_address() {
    let _ = plan_set_frozen(
        &signer(freeze_authority_id()),
        &ours(other_id()),
        true,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account must be initialized")]
fn freeze_rejects_an_empty_parameters_shard() {
    let effect = plan_set_frozen(
        &signer(freeze_authority_id()),
        &ours(parameters_id()),
        true,
        STABLECOIN_PROGRAM_ID,
    );
    let _ = set_frozen(&empty(), &effect);
}

// ─── OpenPosition ───────────────────────────────────────────────────────────

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the plan's signature so each test varies exactly one slot"
)]
fn open(
    owner: &AccountMeta,
    position: &AccountMeta,
    vault: &AccountMeta,
    user_holding: &AccountMeta,
    collateral_definition: &AccountMeta,
    parameters_slot: &AccountMeta,
    clock: &AccountMeta,
    params: &ProtocolParameters,
    amount: u128,
) -> (ShardData, Vec<lee_core::program::ChainedCall>) {
    plan_open_position(
        owner,
        position,
        vault,
        user_holding,
        collateral_definition,
        parameters_slot,
        clock,
        params,
        NOW,
        NONCE,
        amount,
        STABLECOIN_PROGRAM_ID,
    )
}

fn open_ok(amount: u128) -> (ShardData, Vec<lee_core::program::ChainedCall>) {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        amount,
    )
}

#[test]
fn open_position_builds_a_debt_free_position_stamped_from_the_clock() {
    let (data, _) = open_ok(1_000);
    assert_eq!(
        Position::try_from(&data).unwrap(),
        Position {
            owner_account_id: owner_id(),
            position_nonce: NONCE,
            vault_account_id: vault_id(),
            collateral_amount: 1_000,
            // §10.4 deliberately omits an initial-debt parameter.
            normalized_debt_amount: 0,
            opened_at: NOW,
        }
    );
}

#[test]
fn open_position_chains_an_initialize_then_a_transfer() {
    let (_, calls) = open_ok(1_000);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].program_account_id, TOKEN_PROGRAM_ID);
    // The vault's PDA seed is the whole of the authority for creating it.
    assert_eq!(calls[0].pda_seeds.len(), 1);
    match borsh::from_slice(&calls[0].instruction_data).expect("Token instruction") {
        token_core::Instruction::InitializeAccount { kind } => {
            assert_eq!(kind, TokenKind::Fungible);
        }
        _ => panic!("expected an InitializeAccount instruction"),
    }
    // The funding transfer carries no seed: the sender is the user's own
    // holding, authorized by the transaction's witness set.
    assert!(calls[1].pda_seeds.is_empty());
    match borsh::from_slice(&calls[1].instruction_data).expect("Token instruction") {
        token_core::Instruction::Transfer {
            amount_to_transfer,
            descriptor: d,
        } => {
            assert_eq!(amount_to_transfer, 1_000);
            assert_eq!(d, descriptor(collateral_definition_id()));
        }
        _ => panic!("expected a Transfer instruction"),
    }
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn open_position_requires_owner_authorization() {
    open(
        &ours(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "User collateral holding authorization is missing")]
fn open_position_requires_user_holding_authorization() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

/// `!is_frozen` survives as a plan-phase check even though the plan cannot read
/// the flag: it is read off the declared parameters, which `check_parameters`
/// pins to the shard.
#[test]
#[should_panic(expected = "Protocol is frozen")]
fn open_position_is_refused_while_frozen() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &frozen_parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "Collateral definition does not match the one bound")]
fn open_position_rejects_a_definition_that_is_not_the_bound_collateral() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(other_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

/// Strictly stronger than v0.2.5's "the definition and the holding agree": the
/// Token Program is the one recorded at bootstrap, so a slot naming any other
/// program's shard is refused rather than merely cross-checked.
#[test]
#[should_panic(expected = "Collateral definition must select the protocol's bound Token Program")]
fn open_position_rejects_a_definition_under_a_foreign_token_program() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &AccountMeta::new(collateral_definition_id(), false, FOREIGN_PROGRAM_ID),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "User collateral holding must select the protocol's bound Token Program")]
fn open_position_rejects_a_user_holding_under_a_foreign_token_program() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &AccountMeta::new(user_collateral_holding_id(), true, FOREIGN_PROGRAM_ID),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "Position account ID does not match")]
fn open_position_rejects_a_wrong_position_address() {
    open(
        &signer(owner_id()),
        &ours(other_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "Position vault account ID does not match")]
fn open_position_rejects_a_wrong_vault_address() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(other_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn open_position_rejects_parameters_at_the_wrong_address() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(other_id()),
        &clock_slot(),
        &parameters(),
        1_000,
    );
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn open_position_rejects_a_wrong_clock_account() {
    open(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &token_slot(collateral_definition_id()),
        &ours(parameters_id()),
        &AccountMeta::new(other_id(), false, clock_account_id()),
        &parameters(),
        1_000,
    );
}

// ─── DepositCollateral ──────────────────────────────────────────────────────

fn deposit_plan(
    owner: &AccountMeta,
    vault: &AccountMeta,
    user_holding: &AccountMeta,
    parameters_slot: &AccountMeta,
    params: &ProtocolParameters,
    amount: u128,
) -> lee_core::program::ChainedCall {
    plan_deposit_collateral(
        owner,
        &ours(position_id()),
        vault,
        user_holding,
        parameters_slot,
        params,
        NONCE,
        amount,
        STABLECOIN_PROGRAM_ID,
    )
}

fn deposit_effect(vault_balance: u128, amount: u128) -> DepositCollateral {
    DepositCollateral {
        expected: identity(),
        vault_balance,
        amount,
    }
}

#[test]
fn plan_deposit_collateral_chains_a_seedless_transfer_into_the_vault() {
    let call = deposit_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &parameters(),
        250,
    );
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    assert!(call.pda_seeds.is_empty());
    match borsh::from_slice(&call.instruction_data).expect("Token instruction") {
        token_core::Instruction::Transfer {
            amount_to_transfer,
            descriptor: d,
        } => {
            assert_eq!(amount_to_transfer, 250);
            assert_eq!(d, descriptor(collateral_definition_id()));
        }
        _ => panic!("expected a Transfer instruction"),
    }
}

/// Deposits stay available while frozen: §7 keeps the one collateral move that
/// can only improve a position open in emergencies.
#[test]
fn plan_deposit_collateral_works_while_frozen() {
    deposit_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &frozen_parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn plan_deposit_collateral_requires_owner_authorization() {
    deposit_plan(
        &ours(owner_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "User collateral holding authorization is missing")]
fn plan_deposit_collateral_requires_user_holding_authorization() {
    deposit_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "Position vault must select the protocol's bound Token Program")]
fn plan_deposit_collateral_rejects_a_vault_under_a_foreign_token_program() {
    deposit_plan(
        &signer(owner_id()),
        &AccountMeta::new(vault_id(), false, FOREIGN_PROGRAM_ID),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "Position vault account ID does not match")]
fn plan_deposit_collateral_rejects_a_wrong_vault() {
    deposit_plan(
        &signer(owner_id()),
        &token_slot(other_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(parameters_id()),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn plan_deposit_collateral_rejects_parameters_at_the_wrong_address() {
    deposit_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &signed_token_slot(user_collateral_holding_id()),
        &ours(other_id()),
        &parameters(),
        250,
    );
}

#[test]
fn position_after_deposit_sets_collateral_from_the_vault_balance() {
    let after = Position::try_from(&position_after_deposit(
        &ShardData::from(&position(1_000, 10)),
        &deposit_effect(1_000, 250),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 1_250);
    // Debt is untouched — a deposit never moves it.
    assert_eq!(after.normalized_debt_amount, 10);
}

/// Anyone can transfer straight into a vault, so the position is *set* to the
/// vault's balance plus `amount` rather than incremented. Incrementing would
/// leave the position permanently below the vault and strand the difference.
/// A zero-amount deposit is the recovery path for exactly that case.
#[test]
fn position_after_deposit_absorbs_a_donated_vault_balance() {
    let after = Position::try_from(&position_after_deposit(
        &ShardData::from(&position(1_000, 0)),
        // The vault holds 1_300: 1_000 recorded plus 300 donated directly.
        &deposit_effect(1_300, 0),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 1_300);
}

#[test]
#[should_panic(expected = "Position collateral_amount overflow")]
fn position_after_deposit_rejects_an_overflowing_total() {
    let _ = position_after_deposit(
        &ShardData::from(&position(u128::MAX, 0)),
        &deposit_effect(u128::MAX, 1),
    );
}

#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_after_deposit_rejects_a_mismatched_identity() {
    let stored = Position {
        owner_account_id: other_id(),
        ..position(1_000, 0)
    };
    let _ = position_after_deposit(&ShardData::from(&stored), &deposit_effect(1_000, 1));
}

// ─── GenerateDebt ───────────────────────────────────────────────────────────

fn inputs(
    accumulator_anchor: u128,
    redemption_state: RedemptionPriceState,
    now: u64,
) -> CollateralizationInputs {
    CollateralizationInputs {
        fee: FeeProjection {
            accumulator: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: accumulator_anchor,
                last_accrued_at: now,
            },
            stability_fee_per_millisecond: FIXED_POINT_ONE,
            now,
        },
        redemption: redemption_state,
        minimum_collateralization_ratio: MIN_RATIO,
    }
}

/// A unit accumulator and a unit-rate redemption anchor, both already at `now` —
/// so the projections are identities and each test varies one figure.
fn unit_inputs() -> CollateralizationInputs {
    inputs(
        FIXED_POINT_ONE,
        RedemptionPriceState {
            redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: NOW,
        },
        NOW,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the plan's signature so each test varies exactly one slot"
)]
fn generate_plan(
    owner: &AccountMeta,
    stablecoin_definition: &AccountMeta,
    user_holding: &AccountMeta,
    accumulator_slot: &AccountMeta,
    redemption_slot: &AccountMeta,
    oracle: &AccountMeta,
    parameters_slot: &AccountMeta,
    clock: &AccountMeta,
    params: &ProtocolParameters,
    oracle_timestamp: u64,
    amount: u128,
) -> lee_core::program::ChainedCall {
    plan_generate_debt(
        owner,
        &ours(position_id()),
        stablecoin_definition,
        user_holding,
        accumulator_slot,
        redemption_slot,
        oracle,
        parameters_slot,
        clock,
        params,
        oracle_timestamp,
        NOW,
        NONCE,
        amount,
        STABLECOIN_PROGRAM_ID,
    )
}

fn generate_plan_ok(
    params: &ProtocolParameters,
    oracle_timestamp: u64,
) -> lee_core::program::ChainedCall {
    generate_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        params,
        oracle_timestamp,
        500,
    )
}

#[test]
fn plan_generate_debt_chains_a_mint_authorized_by_the_definition_seed() {
    let call = generate_plan_ok(&parameters(), NOW - 1_000);
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    // `initialize_program` made the definition its own mint authority.
    assert_eq!(call.pda_seeds.len(), 1);
    match borsh::from_slice(&call.instruction_data).expect("Token instruction") {
        token_core::Instruction::Mint { amount_to_mint } => assert_eq!(amount_to_mint, 500),
        _ => panic!("expected a Mint instruction"),
    }
}

/// The oracle is a liveness gate only — §10.7 never consumes its price — and the
/// gate moves into the plan, because both halves of the comparison are declared
/// and pinned elsewhere.
#[test]
#[should_panic(expected = "Market price oracle observation is stale")]
fn plan_generate_debt_rejects_a_stale_observation() {
    generate_plan_ok(&parameters(), NOW - ORACLE_MAX_AGE - 1);
}

/// A saturating subtraction would report age 0 for a future-dated observation,
/// reading as maximally fresh and defeating the gate.
#[test]
#[should_panic(expected = "Market price oracle observation is dated in the future")]
fn plan_generate_debt_rejects_a_future_dated_observation() {
    generate_plan_ok(&parameters(), NOW + 1);
}

#[test]
fn plan_generate_debt_accepts_an_observation_at_the_exact_staleness_bound() {
    generate_plan_ok(&parameters(), NOW - ORACLE_MAX_AGE);
}

#[test]
#[should_panic(expected = "Protocol is frozen")]
fn plan_generate_debt_is_refused_while_frozen() {
    generate_plan_ok(&frozen_parameters(), NOW - 1_000);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn plan_generate_debt_requires_owner_authorization() {
    generate_plan(
        &ours(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        NOW - 1_000,
        500,
    );
}

#[test]
#[should_panic(expected = "Stablecoin definition does not match the one bound")]
fn plan_generate_debt_rejects_an_unbound_stablecoin_definition() {
    generate_plan(
        &signer(owner_id()),
        &token_slot(other_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        NOW - 1_000,
        500,
    );
}

#[test]
#[should_panic(expected = "User stablecoin holding must select the protocol's bound Token Program")]
fn plan_generate_debt_rejects_a_user_holding_under_a_foreign_token_program() {
    generate_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &AccountMeta::new(user_stablecoin_holding_id(), false, FOREIGN_PROGRAM_ID),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        NOW - 1_000,
        500,
    );
}

#[test]
#[should_panic(expected = "Market price oracle account_id does not match")]
fn plan_generate_debt_rejects_an_unbound_oracle() {
    generate_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &oracle_slot(new_oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        NOW - 1_000,
        500,
    );
}

#[test]
#[should_panic(expected = "RedemptionPriceState account ID does not match")]
fn plan_generate_debt_rejects_a_wrong_redemption_pda() {
    generate_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(other_id()),
        &oracle_slot(oracle_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        NOW - 1_000,
        500,
    );
}

fn generate_effect(amount: u128, inputs: CollateralizationInputs) -> GenerateDebt {
    GenerateDebt {
        expected: identity(),
        amount,
        inputs,
    }
}

#[test]
fn position_after_generate_increases_the_normalized_debt() {
    let after = Position::try_from(&position_after_generate(
        &ShardData::from(&position(1_000, 0)),
        &generate_effect(100, unit_inputs()),
    ))
    .unwrap();
    // Unit accumulator, so the normalized delta equals the minted amount.
    assert_eq!(after.normalized_debt_amount, 100);
    assert_eq!(after.collateral_amount, 1_000);
}

/// §6.3 rounds the delta **up**: the borrower receives exactly `amount`, so
/// nominal debt must grow by at least `amount` and the remainder stays with the
/// protocol.
#[test]
fn position_after_generate_rounds_the_normalized_delta_up() {
    let fee_grown = inputs(
        FIXED_POINT_ONE * 3,
        RedemptionPriceState {
            redemption_price_at_last_update: 1,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: NOW,
        },
        NOW,
    );
    let after = Position::try_from(&position_after_generate(
        &ShardData::from(&position(u128::MAX / FIXED_POINT_ONE, 0)),
        &generate_effect(100, fee_grown),
    ))
    .unwrap();
    assert_eq!(
        after.normalized_debt_amount,
        mul_div_ceil(100, FIXED_POINT_ONE, FIXED_POINT_ONE * 3)
    );
    assert!(after.normalized_debt_amount * 3 >= 100);
}

#[test]
fn position_after_generate_at_the_exact_ratio_boundary_succeeds() {
    // Debt 100 at a 0.5 redemption price is worth 50 in collateral units; 1.5x
    // of that is exactly 75.
    let _ = position_after_generate(
        &ShardData::from(&position(75, 0)),
        &generate_effect(100, unit_inputs()),
    );
}

#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn position_after_generate_fails_one_unit_below_the_ratio() {
    let _ = position_after_generate(
        &ShardData::from(&position(74, 0)),
        &generate_effect(100, unit_inputs()),
    );
}

/// A rate below 1.0 decays the projected price to zero over a long enough gap,
/// which would zero the required collateral and let any mint through.
#[test]
#[should_panic(expected = "Redemption price projected to zero")]
fn position_after_generate_rejects_a_redemption_price_that_projects_to_zero() {
    let decayed = inputs(
        FIXED_POINT_ONE,
        RedemptionPriceState {
            redemption_price_at_last_update: 1,
            // A rate of zero collapses the price on the first millisecond.
            redemption_rate_per_millisecond: 0,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
        NOW,
    );
    let _ = position_after_generate(
        &ShardData::from(&position(1_000, 0)),
        &generate_effect(1, decayed),
    );
}

/// The projection is `U512` precisely so a rate at the controller's permitted
/// limit — which outgrows `u128` about 45 minutes out — compares rather than
/// overflowing. Such a price can never pass, which is the right answer.
#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn position_after_generate_compares_a_projected_price_beyond_u128() {
    let exploding = inputs(
        FIXED_POINT_ONE,
        RedemptionPriceState {
            redemption_price_at_last_update: REDEMPTION_PRICE_ANCHOR,
            redemption_rate_per_millisecond: FIXED_POINT_ONE * 2,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
        NOW,
    );
    let _ = position_after_generate(
        &ShardData::from(&position(u128::MAX, 0)),
        &generate_effect(1, exploding),
    );
}

#[test]
#[should_panic(expected = "Position normalized_debt_amount overflow")]
fn position_after_generate_rejects_an_overflowing_debt() {
    let _ = position_after_generate(
        &ShardData::from(&position(u128::MAX, u128::MAX)),
        &generate_effect(1, unit_inputs()),
    );
}

// ─── WithdrawCollateral ─────────────────────────────────────────────────────

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the plan's signature so each test varies exactly one slot"
)]
fn withdraw_plan(
    owner: &AccountMeta,
    vault: &AccountMeta,
    user_holding: &AccountMeta,
    accumulator_slot: &AccountMeta,
    redemption_slot: &AccountMeta,
    parameters_slot: &AccountMeta,
    clock: &AccountMeta,
    params: &ProtocolParameters,
    amount: u128,
) -> lee_core::program::ChainedCall {
    plan_withdraw_collateral(
        owner,
        &ours(position_id()),
        vault,
        user_holding,
        accumulator_slot,
        redemption_slot,
        parameters_slot,
        clock,
        params,
        NONCE,
        amount,
        STABLECOIN_PROGRAM_ID,
    )
}

fn withdraw_plan_ok(params: &ProtocolParameters) -> lee_core::program::ChainedCall {
    withdraw_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(parameters_id()),
        &clock_slot(),
        params,
        250,
    )
}

/// The vault is the sender, and a chained call ships shard selectors rather than
/// authorized accounts — so the vault's PDA seed is the whole of the authority
/// for debiting it.
#[test]
fn plan_withdraw_collateral_chains_a_seeded_transfer_out_of_the_vault() {
    let call = withdraw_plan_ok(&parameters());
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    assert_eq!(call.pda_seeds.len(), 1);
    match borsh::from_slice(&call.instruction_data).expect("Token instruction") {
        token_core::Instruction::Transfer {
            amount_to_transfer,
            descriptor: d,
        } => {
            assert_eq!(amount_to_transfer, 250);
            assert_eq!(d, descriptor(collateral_definition_id()));
        }
        _ => panic!("expected a Transfer instruction"),
    }
}

#[test]
#[should_panic(expected = "Protocol is frozen")]
fn plan_withdraw_collateral_is_refused_while_frozen() {
    withdraw_plan_ok(&frozen_parameters());
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn plan_withdraw_collateral_requires_owner_authorization() {
    withdraw_plan(
        &ours(owner_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn plan_withdraw_collateral_rejects_a_wrong_accumulator_pda() {
    withdraw_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &ours(other_id()),
        &ours(redemption_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "RedemptionPriceState account ID does not match")]
fn plan_withdraw_collateral_rejects_a_wrong_redemption_pda() {
    withdraw_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &token_slot(user_collateral_holding_id()),
        &ours(accumulator_id()),
        &ours(other_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        250,
    );
}

#[test]
#[should_panic(expected = "User collateral holding must select the protocol's bound Token Program")]
fn plan_withdraw_collateral_rejects_a_user_holding_under_a_foreign_token_program() {
    withdraw_plan(
        &signer(owner_id()),
        &token_slot(vault_id()),
        &AccountMeta::new(user_collateral_holding_id(), false, FOREIGN_PROGRAM_ID),
        &ours(accumulator_id()),
        &ours(redemption_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        250,
    );
}

fn withdraw_effect(amount: u128, inputs: CollateralizationInputs) -> WithdrawCollateral {
    WithdrawCollateral {
        expected: identity(),
        amount,
        inputs,
    }
}

#[test]
fn position_after_withdraw_decreases_the_collateral() {
    let after = Position::try_from(&position_after_withdraw(
        &ShardData::from(&position(1_000, 0)),
        &withdraw_effect(250, unit_inputs()),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 750);
}

#[test]
fn position_after_withdraw_allows_a_full_drain_of_a_debt_free_position() {
    let after = Position::try_from(&position_after_withdraw(
        &ShardData::from(&position(1_000, 0)),
        &withdraw_effect(1_000, unit_inputs()),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 0);
}

#[test]
fn position_after_withdraw_with_debt_succeeds_while_collateralization_holds() {
    // 200 collateral against normalized debt 100 at a 0.5 price and 1.5x needs
    // 75, so withdrawing 125 lands exactly on the boundary.
    let after = Position::try_from(&position_after_withdraw(
        &ShardData::from(&position(200, 100)),
        &withdraw_effect(125, unit_inputs()),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 75);
}

#[test]
#[should_panic(expected = "Position is undercollateralized")]
fn position_after_withdraw_fails_one_unit_below_the_ratio() {
    let _ = position_after_withdraw(
        &ShardData::from(&position(200, 100)),
        &withdraw_effect(126, unit_inputs()),
    );
}

/// §6.2 is an inequality on the *new* state, so skipping it where that state is
/// trivially safe is sound rather than lenient — and requiring a live redemption
/// price there would only make the instruction fail on a stale oracle.
#[test]
fn position_after_withdraw_from_a_debt_free_position_skips_the_projection() {
    let decayed = inputs(
        FIXED_POINT_ONE,
        RedemptionPriceState {
            redemption_price_at_last_update: 1,
            redemption_rate_per_millisecond: 0,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
        NOW,
    );
    // A price that projects to zero would panic if the check ran.
    let _ = position_after_withdraw(
        &ShardData::from(&position(1_000, 0)),
        &withdraw_effect(1_000, decayed),
    );
}

#[test]
fn a_zero_amount_withdrawal_skips_the_projection_even_with_debt() {
    let decayed = inputs(
        FIXED_POINT_ONE,
        RedemptionPriceState {
            redemption_price_at_last_update: 1,
            redemption_rate_per_millisecond: 0,
            controller_integral_term: 0,
            last_updated_at: T0,
        },
        NOW,
    );
    let after = Position::try_from(&position_after_withdraw(
        &ShardData::from(&position(1_000, 100)),
        &withdraw_effect(0, decayed),
    ))
    .unwrap();
    assert_eq!(after.collateral_amount, 1_000);
}

#[test]
#[should_panic(expected = "Withdraw amount exceeds position collateral")]
fn position_after_withdraw_rejects_an_overdraw() {
    let _ = position_after_withdraw(
        &ShardData::from(&position(1_000, 0)),
        &withdraw_effect(1_001, unit_inputs()),
    );
}

#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_after_withdraw_rejects_a_stale_vault_field() {
    let stored = Position {
        vault_account_id: other_id(),
        ..position(1_000, 0)
    };
    let _ = position_after_withdraw(
        &ShardData::from(&stored),
        &withdraw_effect(1, unit_inputs()),
    );
}

// ─── RepayDebt ──────────────────────────────────────────────────────────────

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the plan's signature so each test varies exactly one slot"
)]
fn repay_plan(
    owner: &AccountMeta,
    stablecoin_definition: &AccountMeta,
    user_holding: &AccountMeta,
    accumulator_slot: &AccountMeta,
    parameters_slot: &AccountMeta,
    clock: &AccountMeta,
    params: &ProtocolParameters,
    amount: u128,
) -> lee_core::program::ChainedCall {
    plan_repay_debt(
        owner,
        &ours(position_id()),
        stablecoin_definition,
        user_holding,
        accumulator_slot,
        parameters_slot,
        clock,
        params,
        NONCE,
        amount,
        STABLECOIN_PROGRAM_ID,
    )
}

fn repay_plan_ok(params: &ProtocolParameters) -> lee_core::program::ChainedCall {
    repay_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &clock_slot(),
        params,
        100,
    )
}

#[test]
fn plan_repay_debt_chains_a_seedless_fungible_burn() {
    let call = repay_plan_ok(&parameters());
    assert_eq!(call.program_account_id, TOKEN_PROGRAM_ID);
    // No seed: the burn debits the user's own holding, authorized by the
    // transaction's witness set.
    assert!(call.pda_seeds.is_empty());
    match borsh::from_slice(&call.instruction_data).expect("Token instruction") {
        token_core::Instruction::Burn {
            amount_to_burn,
            kind,
        } => {
            assert_eq!(amount_to_burn, 100);
            assert_eq!(kind, TokenKind::Fungible);
        }
        _ => panic!("expected a Burn instruction"),
    }
}

/// Repayment stays available while frozen: it only improves the protocol's
/// position (§7).
#[test]
fn plan_repay_debt_works_while_frozen() {
    repay_plan_ok(&frozen_parameters());
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn plan_repay_debt_requires_owner_authorization() {
    repay_plan(
        &ours(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        100,
    );
}

#[test]
#[should_panic(expected = "User stablecoin holding authorization is missing")]
fn plan_repay_debt_requires_user_holding_authorization() {
    repay_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        100,
    );
}

#[test]
#[should_panic(expected = "Stablecoin definition does not match the one bound")]
fn plan_repay_debt_rejects_an_unbound_stablecoin_definition() {
    repay_plan(
        &signer(owner_id()),
        &token_slot(other_definition_id()),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        100,
    );
}

/// The attack the bootstrap binding closes: without `token_program_id` recorded,
/// a caller routes the burn through a program of their own, which burns nothing
/// yet still lets the position's debt fall.
#[test]
#[should_panic(expected = "Stablecoin definition must select the protocol's bound Token Program")]
fn plan_repay_debt_rejects_a_definition_under_a_foreign_token_program() {
    repay_plan(
        &signer(owner_id()),
        &AccountMeta::new(stablecoin_definition_id(), false, FOREIGN_PROGRAM_ID),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        100,
    );
}

#[test]
#[should_panic(expected = "StabilityFeeAccumulator account ID does not match")]
fn plan_repay_debt_rejects_a_wrong_accumulator_pda() {
    repay_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(other_id()),
        &ours(parameters_id()),
        &clock_slot(),
        &parameters(),
        100,
    );
}

#[test]
#[should_panic(expected = "Clock account must be the system CLOCK_01 account")]
fn plan_repay_debt_rejects_a_wrong_clock_account() {
    repay_plan(
        &signer(owner_id()),
        &token_slot(stablecoin_definition_id()),
        &signed_token_slot(user_stablecoin_holding_id()),
        &ours(accumulator_id()),
        &ours(parameters_id()),
        &AccountMeta::new(other_id(), false, clock_account_id()),
        &parameters(),
        100,
    );
}

/// A unit projection already at `now`, so the normalized delta equals the
/// amount burned.
fn unit_fee() -> FeeProjection {
    FeeProjection {
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: NOW,
        },
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        now: NOW,
    }
}

fn repay_effect(amount: u128, fee: FeeProjection) -> RepayDebt {
    RepayDebt {
        expected: identity(),
        amount,
        fee,
    }
}

#[test]
fn position_after_repay_decreases_the_normalized_debt() {
    let after = Position::try_from(&position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(40, unit_fee()),
    ))
    .unwrap();
    assert_eq!(after.normalized_debt_amount, 60);
    assert_eq!(after.collateral_amount, 1_000);
}

#[test]
fn position_after_repay_allows_a_full_repayment() {
    let after = Position::try_from(&position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(100, unit_fee()),
    ))
    .unwrap();
    assert_eq!(after.normalized_debt_amount, 0);
}

#[test]
fn position_after_repay_allows_a_zero_amount() {
    let after = position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(0, unit_fee()),
    );
    assert_eq!(after, ShardData::from(&position(1_000, 100)));
}

/// §6.3 rounds the delta **down**: the borrower burned exactly `amount`, their
/// debt shrinks by at most that much, and the remainder is fee credit for the
/// protocol.
#[test]
fn position_after_repay_rounds_the_normalized_delta_down() {
    let grown = FeeProjection {
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE * 3,
            last_accrued_at: NOW,
        },
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        now: NOW,
    };
    let after = Position::try_from(&position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(100, grown),
    ))
    .unwrap();
    let delta = mul_div(100, FIXED_POINT_ONE, FIXED_POINT_ONE * 3);
    assert_eq!(after.normalized_debt_amount, 100 - delta);
    assert!(delta * 3 <= 100);
}

#[test]
#[should_panic(expected = "Repay amount exceeds outstanding normalized debt")]
fn position_after_repay_rejects_an_overrepayment() {
    let _ = position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(101, unit_fee()),
    );
}

/// With fees accrued the floored delta is smaller than the amount burned, so the
/// overrepay boundary moves with the accumulator rather than sitting at the
/// recorded debt.
#[test]
fn position_after_repay_accepts_an_amount_above_the_debt_once_fees_have_accrued() {
    let grown = FeeProjection {
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE * 2,
            last_accrued_at: NOW,
        },
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        now: NOW,
    };
    // 150 burned against a 2.0 accumulator normalizes to 75, which is inside a
    // recorded debt of 100 even though the nominal figure is not.
    let after = Position::try_from(&position_after_repay(
        &ShardData::from(&position(1_000, 100)),
        &repay_effect(150, grown),
    ))
    .unwrap();
    assert_eq!(after.normalized_debt_amount, 25);
}

#[test]
#[should_panic(expected = "Declared position identity does not match")]
fn position_after_repay_rejects_a_stale_owner_field() {
    let stored = Position {
        owner_account_id: other_id(),
        ..position(1_000, 100)
    };
    let _ = position_after_repay(&ShardData::from(&stored), &repay_effect(1, unit_fee()));
}

// ─── ClosePosition ──────────────────────────────────────────────────────────

fn close_plan(
    owner: &AccountMeta,
    position_slot: &AccountMeta,
    vault: &AccountMeta,
    parameters_slot: &AccountMeta,
    params: &ProtocolParameters,
) {
    plan_close_position(
        owner,
        position_slot,
        vault,
        parameters_slot,
        params,
        NONCE,
        STABLECOIN_PROGRAM_ID,
    );
}

#[test]
fn plan_close_position_accepts_the_canonical_accounts() {
    close_plan(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &ours(parameters_id()),
        &parameters(),
    );
}

/// Closing a settled position removes an obligation, so §7 keeps it available
/// while frozen.
#[test]
fn plan_close_position_works_while_frozen() {
    close_plan(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &ours(parameters_id()),
        &frozen_parameters(),
    );
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn plan_close_position_requires_owner_authorization() {
    close_plan(
        &ours(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &ours(parameters_id()),
        &parameters(),
    );
}

#[test]
#[should_panic(expected = "Position vault account ID does not match")]
fn plan_close_position_rejects_a_wrong_vault() {
    close_plan(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(other_id()),
        &ours(parameters_id()),
        &parameters(),
    );
}

#[test]
#[should_panic(expected = "ProtocolParameters account ID does not match")]
fn plan_close_position_rejects_parameters_at_the_wrong_address() {
    close_plan(
        &signer(owner_id()),
        &ours(position_id()),
        &token_slot(vault_id()),
        &ours(other_id()),
        &parameters(),
    );
}

fn close_effect() -> stablecoin_core::ClosePosition {
    stablecoin_core::ClosePosition {
        expected: identity(),
    }
}

/// v0.2.5 could only zero the *data*, so the account lingered stablecoin-owned
/// and the `(owner, nonce)` pair could never be reused. v0.3.0 removes ownership
/// from an address entirely, so clearing the shard *is* the close — and the pair
/// becomes reusable, because `create` asks only that the shard be empty.
#[test]
fn position_after_close_clears_the_shard_and_frees_the_owner_and_nonce_pair() {
    let cleared = position_after_close(&ShardData::from(&position(0, 0)), &close_effect());
    assert_eq!(cleared, empty());
    // Which is exactly what `open_position` needs to reuse the pair.
    let reopened = ShardData::from(&position(500, 0));
    assert_eq!(create(&cleared, reopened.clone()), reopened);
}

#[test]
#[should_panic(expected = "Position still has outstanding debt")]
fn position_after_close_rejects_outstanding_debt() {
    let _ = position_after_close(&ShardData::from(&position(0, 1)), &close_effect());
}

#[test]
#[should_panic(expected = "Position still holds collateral")]
fn position_after_close_rejects_remaining_collateral() {
    let _ = position_after_close(&ShardData::from(&position(1, 0)), &close_effect());
}

/// The vault's emptiness is a guard on its own shard rather than a read here,
/// which catches the case where position and vault disagree — otherwise tokens
/// would be stranded behind a cleared position.
#[test]
#[should_panic(expected = "Declared vault balance does not match")]
fn close_position_refuses_a_vault_that_still_holds_a_balance() {
    check_holding_balance(
        &holding(collateral_definition_id(), 1),
        &CheckHoldingBalance {
            descriptor: descriptor(collateral_definition_id()),
            balance: 0,
        },
    );
}

#[test]
#[should_panic(expected = "Position account must be initialized")]
fn position_after_close_rejects_an_empty_shard() {
    let _ = position_after_close(&empty(), &close_effect());
}
