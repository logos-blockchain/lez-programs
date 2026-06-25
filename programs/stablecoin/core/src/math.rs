//! Fixed-point arithmetic primitives for the Stablecoin program.
//!
//! All rate, ratio, and price-multiplier values in the protocol are stored as
//! `u128` integers scaled by [`FIXED_POINT_ONE`], so the integer `1.0` is
//! `10^27`. Multiplications use `U256` intermediates to avoid overflow.

use alloy_primitives::U256;

/// The value `1.0` in our 27-decimal fixed-point representation.
///
/// Rate fields store `actual_value * FIXED_POINT_ONE`.
pub const FIXED_POINT_ONE: u128 = 10u128.pow(27);

/// `(a * b) / c` computed via `U256` intermediates and rounded toward zero.
///
/// # Panics
/// - `c == 0` (division by zero).
/// - The result exceeds `u128::MAX`.
#[must_use]
pub fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    assert!(c != 0, "mul_div: division by zero");
    let product = U256::from(a)
        .checked_mul(U256::from(b))
        .expect("mul_div: intermediate product overflows U256");
    let quotient = product
        .checked_div(U256::from(c))
        .expect("mul_div: division by zero");
    quotient.try_into().expect("mul_div: result exceeds u128")
}

/// `ceil((a * b) / c)` via `U256` intermediates.
///
/// # Panics
/// - `c == 0`.
/// - Result exceeds `u128::MAX`.
#[must_use]
pub fn mul_div_ceil(a: u128, b: u128, c: u128) -> u128 {
    assert!(c != 0, "mul_div_ceil: division by zero");
    let product = U256::from(a)
        .checked_mul(U256::from(b))
        .expect("mul_div_ceil: intermediate product overflows U256");
    let divisor = U256::from(c);
    let quotient = product
        .checked_div(divisor)
        .expect("mul_div_ceil: division by zero");
    let remainder = product
        .checked_rem(divisor)
        .expect("mul_div_ceil: division by zero");
    let ceiled = if remainder.is_zero() {
        quotient
    } else {
        quotient
            .checked_add(U256::ONE)
            .expect("mul_div_ceil: ceil increment overflows U256")
    };
    ceiled
        .try_into()
        .expect("mul_div_ceil: result exceeds u128")
}

/// Compute `per_millisecond_rate^milliseconds_elapsed` in fixed-point semantics, where
/// `per_millisecond_rate == FIXED_POINT_ONE` represents `1.0`.
///
/// Algorithm: exponentiation by squaring. `O(log milliseconds_elapsed)`.
///
/// # Edge cases
/// - `milliseconds_elapsed == 0` returns `FIXED_POINT_ONE` (identity).
/// - `per_millisecond_rate == FIXED_POINT_ONE` returns `FIXED_POINT_ONE` regardless of
///   `milliseconds_elapsed`.
///
/// # Overflow
/// NOT self-bounding. For any `per_millisecond_rate > FIXED_POINT_ONE` this
/// eventually overflows `u128` as `milliseconds_elapsed` grows. Callers MUST
/// clamp the elapsed window to a bounded maximum before calling.
#[must_use]
pub fn compound_rate(per_millisecond_rate: u128, milliseconds_elapsed: u64) -> u128 {
    if milliseconds_elapsed == 0 {
        return FIXED_POINT_ONE;
    }
    if per_millisecond_rate == FIXED_POINT_ONE {
        return FIXED_POINT_ONE;
    }
    let mut result = FIXED_POINT_ONE;
    let mut base = per_millisecond_rate;
    let mut exponent = milliseconds_elapsed;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = mul_div(result, base, FIXED_POINT_ONE);
        }
        exponent >>= 1;
        if exponent > 0 {
            base = mul_div(base, base, FIXED_POINT_ONE);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_one_is_ten_to_the_twenty_seventh() {
        assert_eq!(FIXED_POINT_ONE, 1_000_000_000_000_000_000_000_000_000_u128);
    }

    #[test]
    fn mul_div_zero_inputs() {
        assert_eq!(mul_div(0, 5, 3), 0);
        assert_eq!(mul_div(5, 0, 3), 0);
    }

    #[test]
    fn mul_div_exact() {
        assert_eq!(mul_div(100, FIXED_POINT_ONE, FIXED_POINT_ONE), 100);
    }

    #[test]
    fn mul_div_floors_remainder() {
        // 10 * 3 / 4 = 7.5 -> 7
        assert_eq!(mul_div(10, 3, 4), 7);
    }

    #[test]
    fn mul_div_handles_full_u128() {
        // a * b would overflow u128 if not promoted to U256.
        assert_eq!(mul_div(u128::MAX, 2, 4), u128::MAX / 2);
    }

    #[test]
    #[should_panic(expected = "mul_div: division by zero")]
    fn mul_div_panics_on_zero_divisor() {
        let _ = mul_div(1, 1, 0);
    }

    #[test]
    fn mul_div_ceil_exact_equals_floor() {
        assert_eq!(mul_div_ceil(100, FIXED_POINT_ONE, FIXED_POINT_ONE), 100);
    }

    #[test]
    fn mul_div_ceil_rounds_up_when_remainder_non_zero() {
        // 10 * 3 / 4 = 7.5 -> 8
        assert_eq!(mul_div_ceil(10, 3, 4), 8);
    }

    #[test]
    fn mul_div_ceil_never_less_than_floor() {
        for &(a, b, c) in &[(7_u128, 3, 4), (101, 99, 100), (FIXED_POINT_ONE, 3, 7)] {
            assert!(mul_div_ceil(a, b, c) >= mul_div(a, b, c));
        }
    }

    #[test]
    #[should_panic(expected = "mul_div_ceil: division by zero")]
    fn mul_div_ceil_panics_on_zero_divisor() {
        let _ = mul_div_ceil(1, 1, 0);
    }

    #[test]
    fn compound_rate_identity_on_zero_milliseconds() {
        assert_eq!(compound_rate(FIXED_POINT_ONE * 2, 0), FIXED_POINT_ONE);
        assert_eq!(compound_rate(123, 0), FIXED_POINT_ONE);
    }

    #[test]
    fn compound_rate_one_is_one() {
        for &milliseconds in &[0u64, 1, 1_000, 86_400_000, 31_536_000_000] {
            assert_eq!(
                compound_rate(FIXED_POINT_ONE, milliseconds),
                FIXED_POINT_ONE
            );
        }
    }

    #[test]
    fn compound_rate_one_millisecond_equals_rate() {
        let rate = FIXED_POINT_ONE + 1_000_000_000_000_000_000; // 1.001 in fixed-point
        assert_eq!(compound_rate(rate, 1), rate);
    }

    #[test]
    fn compound_rate_two_milliseconds_squares_rate() {
        // Pick a rate where rate * rate / FIXED_POINT_ONE is easy to verify.
        let rate = FIXED_POINT_ONE + 10u128.pow(25); // 1.01 in fixed-point
        let expected = mul_div(rate, rate, FIXED_POINT_ONE);
        assert_eq!(compound_rate(rate, 2), expected);
    }

    #[test]
    fn compound_rate_growth_is_monotonic_above_one() {
        let rate = FIXED_POINT_ONE + 10u128.pow(25); // 1.01 in fixed-point
        let mut prev = FIXED_POINT_ONE;
        for milliseconds in 0..20u64 {
            let now = compound_rate(rate, milliseconds);
            assert!(
                now >= prev,
                "compound_rate not monotonic: {milliseconds}ms -> {now} < {prev}"
            );
            prev = now;
        }
    }

    #[test]
    fn compound_rate_decay_below_one() {
        // 0.99 in fixed-point (i.e. rate < 1).
        let rate = FIXED_POINT_ONE - 10u128.pow(25);
        // After many milliseconds, should be strictly less than FIXED_POINT_ONE.
        let result = compound_rate(rate, 100);
        assert!(result < FIXED_POINT_ONE);
        assert!(result > 0);
    }
}
