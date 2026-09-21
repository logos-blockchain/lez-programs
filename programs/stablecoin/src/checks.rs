//! Shared validation helpers reused across the position-lifecycle instructions.

use alloy_primitives::U512;
use stablecoin_core::{math::FIXED_POINT_ONE, Position};

/// Assert that `position` satisfies the collateralization invariant from spec §6.2:
///
/// ```text
/// position.collateral_amount * FIXED_POINT_ONE^2
///   >= nominal_debt * current_redemption_price * minimum_collateralization_ratio
/// ```
///
/// where `nominal_debt = position.normalized_debt_amount * current_accumulator /
/// FIXED_POINT_ONE`. That division is folded into the left-hand side instead of
/// being performed, so the comparison is **exact**:
///
/// ```text
/// collateral * FIXED_POINT_ONE^3
///   >= normalized_debt * accumulator * redemption_price * ratio
/// ```
///
/// Flooring the nominal debt first would understate it and tilt the check toward
/// the borrower — the opposite of §6.3's rounding direction.
///
/// Computed in `U512`. `U256` is **not** wide enough: `collateral × FIXED_POINT_ONE²`
/// alone exceeds it once collateral passes 115792089237316195423570 — about
/// 115_792 whole tokens at 18 decimals.
///
/// `current_accumulator` and `current_redemption_price` arrive as `U512` because
/// neither projection is bounded by `u128` — a rate at its permitted limit
/// outgrows that type well inside the compounding window (see
/// [`stablecoin_core::math::compute_current_redemption_price_wide`]). The caller
/// is responsible for projecting both forward to the current timestamp (spec
/// §5.3); this helper only compares.
///
/// The right-hand side multiplies with [`U512::saturating_mul`] rather than
/// panicking on overflow. Saturating is the safe direction here: the left-hand
/// side tops out around `10^119` (`u128::MAX × FIXED_POINT_ONE^3`), far below
/// `U512::MAX`, so a saturated requirement always loses the comparison — which
/// is the right answer for a debt whose value has outgrown the type.
///
/// **A zero-debt position always passes**, regardless of collateral — there is
/// nothing to collateralize.
///
/// # Panics
///
/// - `"Position is undercollateralized"` when `lhs >= rhs` does not hold.
pub fn assert_position_is_collateralized(
    position: &Position,
    current_accumulator: U512,
    current_redemption_price: U512,
    minimum_collateralization_ratio: u128,
) {
    if position.normalized_debt_amount == 0 {
        return;
    }

    let one = U512::from(FIXED_POINT_ONE);

    // No division anywhere: `/ FIXED_POINT_ONE` on the debt side is carried as an
    // extra `× FIXED_POINT_ONE` on the collateral side, keeping the check exact.
    // `U512` holds this product outright — `u128::MAX × FIXED_POINT_ONE^3` is
    // about `10^119` — so only the right-hand side needs to saturate.
    let collateral_value = U512::from(position.collateral_amount) * one * one * one;
    let required_collateral_value = U512::from(position.normalized_debt_amount)
        .saturating_mul(current_accumulator)
        .saturating_mul(current_redemption_price)
        .saturating_mul(U512::from(minimum_collateralization_ratio));

    assert!(
        collateral_value >= required_collateral_value,
        "Position is undercollateralized"
    );
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "tests build fixture ratios from constants and deliberately panic via #[should_panic]"
)]
mod tests {
    use lee_core::account::AccountId;

    use super::*;

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
        Position {
            owner_account_id: AccountId::new([1u8; 32]),
            position_nonce: 0,
            vault_account_id: AccountId::new([2u8; 32]),
            collateral_amount,
            normalized_debt_amount,
            opened_at: 0,
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
}
