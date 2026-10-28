//! Exact comparisons in raw stablecoin atomic units, without nominal-debt rounding.

use alloy_primitives::U256;

use crate::math::FIXED_POINT_ONE;

/// Both limits are required and positive; the floor cannot exceed the ceiling.
#[must_use]
pub fn valid_debt_limits(minimum_debt_amount: u128, debt_ceiling: u128) -> bool {
    minimum_debt_amount > 0 && minimum_debt_amount <= debt_ceiling
}

/// Settled positions are exempt. Positive debt must meet the exact accrued floor.
#[must_use]
pub fn meets_debt_floor(normalized_debt: u128, accumulator: u128, minimum: u128) -> bool {
    normalized_debt == 0
        || U256::from(normalized_debt) * U256::from(accumulator)
            >= U256::from(minimum) * U256::from(FIXED_POINT_ONE)
}

/// Admission test for positive minting, including normalization and accrued fees.
#[must_use]
pub fn within_debt_ceiling(total_normalized_debt: u128, accumulator: u128, ceiling: u128) -> bool {
    U256::from(total_normalized_debt) * U256::from(accumulator)
        <= U256::from(ceiling) * U256::from(FIXED_POINT_ONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_require_positive_ordered_atomic_amounts() {
        assert!(!valid_debt_limits(0, 0));
        assert!(!valid_debt_limits(0, 10));
        assert!(!valid_debt_limits(11, 10));
        assert!(valid_debt_limits(10, 10));
        assert!(valid_debt_limits(1, u128::MAX));
    }

    #[test]
    fn fractional_floor_is_not_rounded_up() {
        let a = FIXED_POINT_ONE * 3 / 2;
        assert!(meets_debt_floor(0, a, 2));
        assert!(!meets_debt_floor(1, a, 2));
        assert!(meets_debt_floor(2, a, 3));
        assert!(meets_debt_floor(3, a, 3));
    }

    #[test]
    fn ceiling_preserves_fraction_and_equality() {
        let a = FIXED_POINT_ONE * 3 / 2;
        assert!(!within_debt_ceiling(1, a, 1));
        assert!(within_debt_ceiling(2, a, 3));
        assert!(!within_debt_ceiling(3, a, 4));
    }

    #[test]
    fn full_width_products_fit_without_truncation() {
        assert!(meets_debt_floor(u128::MAX, u128::MAX, u128::MAX));
        assert!(!within_debt_ceiling(u128::MAX, u128::MAX, u128::MAX));
        assert!(within_debt_ceiling(u128::MAX, FIXED_POINT_ONE, u128::MAX));
    }
}
