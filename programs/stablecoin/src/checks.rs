//! Shared validation helpers: who may send a message, the collateralization invariant and the
//! parameter bands.
//!
//! The bands live here because the design requires the same limits at bootstrap and in every
//! setter. One definition means the two can't drift apart.

use alloy_primitives::U512;
use lee_core::program::ReceiveInput;
use stablecoin_core::{
    collateralization::collateralization_values, math::FIXED_POINT_ONE,
    MAXIMUM_CONTROLLER_INTEGRAL_GAIN, MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN,
    MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
};

use crate::engine_actor;

/// The message is the transaction's root message, signed by the receiving account's owner.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
pub(crate) fn assert_owner(input: &ReceiveInput) {
    assert!(
        input.is_authorized && input.from.is_none(),
        "only the account's owner may send this"
    );
}

/// The message comes from the engine.
///
/// # Panics
/// - `"only the engine may send this"` when the sender is anyone else.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used from Task 4: open_position")
)]
pub(crate) fn assert_from_engine(input: &ReceiveInput) {
    assert!(
        input.from == Some(engine_actor(input)),
        "only the engine may send this"
    );
}

/// Assert that a position of `collateral_amount` and `normalized_debt_amount` satisfies the
/// collateralization invariant:
///
/// ```text
/// collateral_amount * FIXED_POINT_ONE^2
///   >= nominal_debt * current_redemption_price * minimum_collateralization_ratio
/// ```
///
/// where `nominal_debt = normalized_debt_amount * current_accumulator / FIXED_POINT_ONE`. That
/// division is folded into the left-hand side instead of being performed, so the comparison is
/// **exact**:
///
/// ```text
/// collateral * FIXED_POINT_ONE^3
///   >= normalized_debt * accumulator * redemption_price * ratio
/// ```
///
/// Flooring the nominal debt first would understate it and tilt the check toward the borrower.
///
/// Computed in `U512`: `U256` is **not** wide enough, `collateral × FIXED_POINT_ONE²` alone
/// exceeds it past about 115_792 whole tokens at 18 decimals.
///
/// `current_accumulator` and `current_redemption_price` arrive as `U512` because neither
/// projection is bounded by `u128`. The caller projects both forward to the current time; this
/// helper only compares. The right-hand side saturates rather than panicking on overflow, which
/// is the safe direction: the left-hand side tops out around `10^119`, far below `U512::MAX`, so
/// a saturated requirement always loses the comparison.
///
/// **A zero-debt position always passes**, regardless of collateral.
///
/// # Panics
/// - `"Position is undercollateralized"` when the inequality does not hold.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used from Task 5: generate_debt")
)]
pub(crate) fn assert_position_is_collateralized(
    collateral_amount: u128,
    normalized_debt_amount: u128,
    current_accumulator: U512,
    current_redemption_price: U512,
    minimum_collateralization_ratio: u128,
) {
    if normalized_debt_amount == 0 {
        return;
    }

    let values = collateralization_values(
        collateral_amount,
        normalized_debt_amount,
        current_accumulator,
        current_redemption_price,
        minimum_collateralization_ratio,
    );
    assert!(
        values.is_collateralized(),
        "Position is undercollateralized"
    );
}

// --- Sane-band constants ----------------------------------------------------------------

pub(crate) const MIN_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 110 / 100; // 1.1x
pub(crate) const MAX_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 10;
pub(crate) const MAX_TIMING_MILLISECONDS: u64 = 86_400_000; // 1 day

/// `FIXED_POINT_ONE <= rate <= MAXIMUM_STABILITY_FEE_PER_MILLISECOND` (about 100% a year).
///
/// The lower bound keeps the fee from decaying debt. The upper bound is the overflow guard: it
/// keeps every accrual, including the setter's catch-up at the old rate, inside `u128`.
pub(crate) fn assert_stability_fee_in_band(rate: u128, label: &str) {
    assert!(rate >= FIXED_POINT_ONE, "{label} below FIXED_POINT_ONE");
    assert!(
        rate <= MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
        "{label} above sane upper bound"
    );
}

/// `1.1x <= ratio <= 10x`. Below 1.1x a position is insolvent on arrival.
pub(crate) fn assert_collateralization_ratio_in_band(ratio: u128, label: &str) {
    assert!(ratio >= MIN_COLLATERALIZATION_RATIO, "{label} below 1.1x");
    assert!(ratio <= MAX_COLLATERALIZATION_RATIO, "{label} above 10x");
}

/// Bands on the PI gains: `0 ..= MAXIMUM_CONTROLLER_*_GAIN`.
///
/// Negative gains are rejected. The controller embeds the stabilizing direction, so a negative
/// gain would be positive feedback and push the redemption price away from the market.
pub(crate) fn assert_controller_gains_in_band(proportional_gain: i128, integral_gain: i128) {
    assert!(
        (0..=MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN).contains(&proportional_gain),
        "controller_proportional_gain out of band"
    );
    assert!(
        (0..=MAXIMUM_CONTROLLER_INTEGRAL_GAIN).contains(&integral_gain),
        "controller_integral_gain out of band"
    );
}

/// `1 <= milliseconds <= 86_400_000`. Zero would allow spam; beyond a day is self-evidently
/// wrong for both the rate interval and oracle staleness.
pub(crate) fn assert_timing_milliseconds_in_band(milliseconds: u64, label: &str) {
    assert!(milliseconds >= 1, "{label} below minimum 1ms");
    assert!(
        milliseconds <= MAX_TIMING_MILLISECONDS,
        "{label} above maximum 86_400_000ms"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    fn input(from: Option<Actor>, is_authorized: bool) -> ReceiveInput {
        ReceiveInput {
            receiver: stablecoin(OWNER),
            from,
            is_authorized,
            pre_state: ActorState::default(),
            message: Vec::new(),
        }
    }

    #[test]
    fn assert_owner_passes_for_a_signed_root_message() {
        assert_owner(&input(None, true));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn assert_owner_rejects_a_nested_call() {
        assert_owner(&input(Some(outsider(OTHER_PROGRAM)), true));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn assert_owner_rejects_an_unsigned_message() {
        assert_owner(&input(None, false));
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn assert_from_engine_rejects_another_stablecoin_actor() {
        assert_from_engine(&input(Some(stablecoin(OWNER)), false));
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn assert_from_engine_rejects_a_root_message() {
        assert_from_engine(&input(None, false));
    }

    #[test]
    fn assert_from_engine_passes_for_the_engine() {
        assert_from_engine(&input(Some(engine()), false));
    }

    /// The projections are `U512` in production; the fixtures below are all
    /// `u128`-sized, so widen them at the call boundary.
    fn assert_collateralized(
        position: &Position,
        current_accumulator: u128,
        current_redemption_price: u128,
        minimum_collateralization_ratio: u128,
    ) {
        assert_position_is_collateralized(
            position.collateral_amount,
            position.normalized_debt_amount,
            U512::from(current_accumulator),
            U512::from(current_redemption_price),
            minimum_collateralization_ratio,
        );
    }

    fn position_with(collateral_amount: u128, normalized_debt_amount: u128) -> Position {
        Position {
            token_program_id: TOKEN,
            collateral_amount,
            normalized_debt_amount,
        }
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

    #[test]
    fn stability_fee_band_accepts_both_endpoints() {
        assert_stability_fee_in_band(FIXED_POINT_ONE, "rate");
        assert_stability_fee_in_band(MAXIMUM_STABILITY_FEE_PER_MILLISECOND, "rate");
    }

    #[test]
    #[should_panic(expected = "rate below FIXED_POINT_ONE")]
    fn stability_fee_band_rejects_below_one() {
        assert_stability_fee_in_band(FIXED_POINT_ONE - 1, "rate");
    }

    #[test]
    #[should_panic(expected = "rate above sane upper bound")]
    fn stability_fee_band_rejects_above_two() {
        assert_stability_fee_in_band(MAXIMUM_STABILITY_FEE_PER_MILLISECOND + 1, "rate");
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
    fn controller_gain_band_accepts_zero_and_the_upper_limits() {
        assert_controller_gains_in_band(0, 0);
        assert_controller_gains_in_band(
            MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN,
            MAXIMUM_CONTROLLER_INTEGRAL_GAIN,
        );
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn controller_gain_band_rejects_an_oversized_proportional_gain() {
        assert_controller_gains_in_band(MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN + 1, 0);
    }

    #[test]
    #[should_panic(expected = "controller_integral_gain out of band")]
    fn controller_gain_band_rejects_an_oversized_integral_gain() {
        assert_controller_gains_in_band(0, MAXIMUM_CONTROLLER_INTEGRAL_GAIN + 1);
    }

    #[test]
    #[should_panic(expected = "controller_proportional_gain out of band")]
    fn controller_gain_band_rejects_a_negative_proportional_gain() {
        assert_controller_gains_in_band(-1, 0);
    }

    #[test]
    #[should_panic(expected = "controller_integral_gain out of band")]
    fn controller_gain_band_rejects_a_negative_integral_gain() {
        assert_controller_gains_in_band(0, -1);
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
            u128::MAX,
            1,
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
            u128::MAX,
            1,
            U512::MAX,
            U512::from(FIXED_POINT_ONE),
            FIXED_POINT_ONE,
        );
    }

    /// Saturation on the right-hand side must not turn into a panic.
    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn every_input_at_the_u512_ceiling_compares_rather_than_panicking() {
        assert_position_is_collateralized(u128::MAX, u128::MAX, U512::MAX, U512::MAX, u128::MAX);
    }
}
