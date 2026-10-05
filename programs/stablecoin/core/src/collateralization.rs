//! Non-panicking evaluation of the program's exact collateralization invariant.

use alloy_primitives::U512;

use crate::math::FIXED_POINT_ONE;

/// Cross-multiplied operands used by the collateralization comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollateralizationValues {
    /// `collateral_amount * FIXED_POINT_ONE^3`, which always fits U512.
    pub collateral_value: U512,
    /// `normalized_debt * accumulator * redemption_price * ratio`, capped at U512::MAX.
    pub required_collateral_value: U512,
    /// Whether a requirement multiplication exceeded U512.
    pub requirement_saturated: bool,
}

impl CollateralizationValues {
    /// Equality is healthy; a saturated requirement exceeds all possible collateral.
    #[must_use]
    pub fn is_collateralized(&self) -> bool {
        self.collateral_value >= self.required_collateral_value
    }
}

/// Compare fractional debt without first flooring its nominal value.
///
/// A zero normalized debt produces a zero requirement. Saturating the requirement
/// is conservative because even maximum collateral is far below U512::MAX.
#[must_use]
pub fn collateralization_values(
    collateral_amount: u128,
    normalized_debt_amount: u128,
    current_accumulator: U512,
    current_redemption_price: U512,
    minimum_collateralization_ratio: u128,
) -> CollateralizationValues {
    let one = U512::from(FIXED_POINT_ONE);
    let collateral_value = U512::from(collateral_amount) * one * one * one;
    let mut required = U512::from(normalized_debt_amount);
    let mut requirement_saturated = false;
    for operand in [
        current_accumulator,
        current_redemption_price,
        U512::from(minimum_collateralization_ratio),
    ] {
        required = match required.checked_mul(operand) {
            Some(product) => product,
            None => {
                requirement_saturated = true;
                U512::MAX
            }
        };
    }
    CollateralizationValues {
        collateral_value,
        required_collateral_value: required,
        requirement_saturated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_and_fractional_debt_are_compared_without_flooring() {
        let one = U512::from(FIXED_POINT_ONE);
        let equality = collateralization_values(110, 100, one, one, FIXED_POINT_ONE * 11 / 10);
        assert!(equality.is_collateralized());
        assert_eq!(
            equality.collateral_value,
            equality.required_collateral_value
        );
        let below = collateralization_values(109, 100, one, one, FIXED_POINT_ONE * 11 / 10);
        assert!(!below.is_collateralized());
        let fractional = collateralization_values(
            2,
            1,
            U512::from(FIXED_POINT_ONE * 19 / 10),
            one,
            FIXED_POINT_ONE * 11 / 10,
        );
        assert!(!fractional.is_collateralized());
        assert!(!fractional.requirement_saturated);
    }

    #[test]
    fn saturated_requirement_and_zero_debt_preserve_the_native_predicate() {
        let saturated = collateralization_values(
            u128::MAX,
            1,
            U512::MAX,
            U512::MAX,
            FIXED_POINT_ONE * 11 / 10,
        );
        assert!(saturated.requirement_saturated);
        assert_eq!(saturated.required_collateral_value, U512::MAX);
        assert!(!saturated.is_collateralized());
        let zero = collateralization_values(0, 0, U512::MAX, U512::MAX, FIXED_POINT_ONE * 11 / 10);
        assert_eq!(zero.required_collateral_value, U512::ZERO);
        assert!(zero.is_collateralized());
    }
}
