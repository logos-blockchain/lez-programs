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
/// FIXED_POINT_ONE`. This is the cross-multiplied form the spec prescribes — it
/// divides once instead of twice, so no intermediate rounding creeps into the
/// comparison.
///
/// Computed in `U512`. `U256` is **not** wide enough: `collateral × FIXED_POINT_ONE²`
/// alone exceeds it once collateral passes 115792089237316195423570 — about
/// 115_792 whole tokens at 18 decimals — and the right-hand side, where nominal
/// debt is scaled by both the redemption price and the ratio, overflows sooner
/// still. Neither side has a bound below `u128::MAX`. The caller is responsible for
/// projecting `current_accumulator` and `current_redemption_price` forward to the
/// current timestamp (spec §5.3) before calling; this helper only compares.
///
/// **A zero-debt position always passes**, regardless of collateral — there is
/// nothing to collateralize.
///
/// # Panics
///
/// - `"Position is undercollateralized"` when `lhs >= rhs` does not hold.
/// - When an intermediate product exceeds `U512`.
pub fn assert_position_is_collateralized(
    position: &Position,
    current_accumulator: u128,
    current_redemption_price: u128,
    minimum_collateralization_ratio: u128,
) {
    if position.normalized_debt_amount == 0 {
        return;
    }

    let multiply = |a: U512, b: U512| {
        a.checked_mul(b)
            .expect("collateralization check: intermediate product overflows U512")
    };

    let one = U512::from(FIXED_POINT_ONE);

    let nominal_debt = multiply(
        U512::from(position.normalized_debt_amount),
        U512::from(current_accumulator),
    )
    .checked_div(one)
    .expect("collateralization check: FIXED_POINT_ONE is non-zero");

    let collateral_value = multiply(multiply(U512::from(position.collateral_amount), one), one);
    let required_collateral_value = multiply(
        multiply(nominal_debt, U512::from(current_redemption_price)),
        U512::from(minimum_collateralization_ratio),
    );

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
        assert_position_is_collateralized(
            &position_with(collateral, 1),
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE * 11 / 10,
        );
    }

    #[test]
    fn maximum_collateral_does_not_overflow_the_cross_product() {
        assert_position_is_collateralized(
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
        assert_position_is_collateralized(
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
        assert_position_is_collateralized(
            &position_with(u128::MAX, u128::MAX),
            u128::MAX,
            u128::MAX,
            u128::MAX,
        );
    }

    #[test]
    fn zero_debt_passes_even_with_zero_collateral() {
        assert_position_is_collateralized(
            &position_with(0, 0),
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE * 3 / 2,
        );
    }

    #[test]
    fn zero_debt_passes_with_collateral() {
        assert_position_is_collateralized(
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
        assert_position_is_collateralized(
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
        assert_position_is_collateralized(
            &position_with(100, 100),
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
        );
    }

    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn one_unit_below_the_boundary_fails() {
        assert_position_is_collateralized(
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
        assert_position_is_collateralized(
            &position_with(75, 100),
            FIXED_POINT_ONE,
            FIXED_POINT_ONE / 2,
            FIXED_POINT_ONE * 3 / 2,
        );
    }

    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn one_unit_below_one_and_a_half_times_collateral_fails() {
        assert_position_is_collateralized(
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

        assert_position_is_collateralized(
            &position,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE / 2,
            FIXED_POINT_ONE * 3 / 2,
        );

        assert_position_is_collateralized(
            &position,
            FIXED_POINT_ONE * 12 / 10,
            FIXED_POINT_ONE / 2,
            FIXED_POINT_ONE * 3 / 2,
        );
    }
}
