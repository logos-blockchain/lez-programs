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

/// Hard cap on the elapsed window (in milliseconds) fed to [`compound_rate`].
///
/// Seven days. [`compound_rate`] is not self-bounding: over an unbounded window
/// any rate above [`FIXED_POINT_ONE`] eventually overflows `u128`, so the
/// projection helpers clamp `Δt` to this constant before compounding (spec
/// §5.3, §8). Seven days comfortably covers realistic keeper-poke gaps; beyond
/// it, fee accrual and redemption drift pause until someone pokes.
pub const MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS: u64 = 604_800_000;

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

/// Project the stability fee accumulator from its persisted anchor to `now`.
///
/// `current = anchor × compound_rate(rate, min(now − last_accrued_at,
/// MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS)) / FIXED_POINT_ONE`
///
/// `Δt` (milliseconds) uses `saturating_sub` so monotonic time is not assumed,
/// then is clamped to [`MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS`]. That clamp is
/// what prevents [`compound_rate`] from overflowing over an unbounded poke gap —
/// the §8 rate bound alone does not (spec §5.2 / §5.3).
#[must_use]
pub fn compute_current_accumulated_rate(
    accumulated_rate_at_last_accrual: u128,
    stability_fee_per_millisecond: u128,
    last_accrued_at: u64,
    now: u64,
) -> u128 {
    let elapsed = now
        .saturating_sub(last_accrued_at)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let factor = compound_rate(stability_fee_per_millisecond, elapsed);
    mul_div(accumulated_rate_at_last_accrual, factor, FIXED_POINT_ONE)
}

/// Project the redemption price from its persisted anchor to `now`.
///
/// `current = anchor × compound_rate(rate_per_millisecond, min(now − last_updated_at,
/// MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS)) / FIXED_POINT_ONE`
///
/// Same `Δt` clamp rationale as [`compute_current_accumulated_rate`]:
/// `redemption_rate_per_millisecond` can also exceed `1.0`, so the window cap is
/// load-bearing for overflow safety.
#[must_use]
pub fn compute_current_redemption_price(
    redemption_price_at_last_update: u128,
    redemption_rate_per_millisecond: u128,
    last_updated_at: u64,
    now: u64,
) -> u128 {
    let elapsed = now
        .saturating_sub(last_updated_at)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let factor = compound_rate(redemption_rate_per_millisecond, elapsed);
    mul_div(redemption_price_at_last_update, factor, FIXED_POINT_ONE)
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

    #[test]
    fn maximum_compounding_window_is_seven_days() {
        assert_eq!(
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
            7 * 24 * 60 * 60 * 1_000
        );
    }

    #[test]
    fn current_accumulated_rate_zero_elapsed_returns_anchor() {
        let anchor = FIXED_POINT_ONE * 12345 / 10000; // 1.2345
        assert_eq!(
            compute_current_accumulated_rate(anchor, FIXED_POINT_ONE + 10u128.pow(20), 100, 100),
            anchor,
        );
    }

    #[test]
    fn current_accumulated_rate_no_drift_returns_anchor() {
        let anchor = FIXED_POINT_ONE * 12345 / 10000;
        for &(last_accrued_at, now) in &[
            (0u64, 60u64),
            (1_000, 86_400),
            (1_700_000_000, 1_700_000_000 + 31_536_000),
        ] {
            assert_eq!(
                compute_current_accumulated_rate(anchor, FIXED_POINT_ONE, last_accrued_at, now),
                anchor,
                "no-drift at (last_accrued_at={last_accrued_at}, now={now}) should return anchor",
            );
        }
    }

    #[test]
    fn current_accumulated_rate_now_before_last_accrued_returns_anchor() {
        let anchor = FIXED_POINT_ONE;
        // `now < last_accrued_at` saturates the elapsed window to 0, so the
        // projection collapses to the anchor.
        assert_eq!(
            compute_current_accumulated_rate(anchor, FIXED_POINT_ONE * 2, 1_000_000, 100),
            anchor,
        );
    }

    #[test]
    fn current_accumulated_rate_one_millisecond_equals_compound_factor() {
        let anchor = FIXED_POINT_ONE;
        let rate = FIXED_POINT_ONE + 10u128.pow(20); // 1.0000001
        let projected = compute_current_accumulated_rate(anchor, rate, 0, 1);
        // With anchor = FIXED_POINT_ONE, current at 1 ms = anchor * rate / FIXED_POINT_ONE = rate.
        assert_eq!(projected, rate);
    }

    #[test]
    fn current_accumulated_rate_growth_is_monotonic() {
        let anchor = FIXED_POINT_ONE;
        let rate = FIXED_POINT_ONE + 10u128.pow(20);
        let mut prev = anchor;
        for millis in 0..30u64 {
            let now = compute_current_accumulated_rate(anchor, rate, 0, millis);
            assert!(now >= prev, "regression at millis={millis}");
            prev = now;
        }
    }

    #[test]
    fn current_accumulated_rate_clamps_elapsed_to_maximum_window() {
        let anchor = FIXED_POINT_ONE;
        // A realistic ~5% annual rate: 1 + 1.5e-12 per millisecond.
        let rate = FIXED_POINT_ONE + 1_500_000_000_000_000;
        let at_window = compute_current_accumulated_rate(
            anchor,
            rate,
            0,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        // Anything past the window projects to exactly the same value — accrual
        // pauses rather than compounding an unbounded exponent.
        for &now in &[
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS * 1_000,
            u64::MAX,
        ] {
            assert_eq!(
                compute_current_accumulated_rate(anchor, rate, 0, now),
                at_window,
                "elapsed window not clamped at now={now}",
            );
        }
        assert!(at_window > anchor);
    }

    #[test]
    fn current_redemption_price_zero_elapsed_returns_anchor() {
        let anchor = FIXED_POINT_ONE / 2;
        assert_eq!(
            compute_current_redemption_price(anchor, FIXED_POINT_ONE + 10u128.pow(20), 100, 100),
            anchor,
        );
    }

    #[test]
    fn current_redemption_price_decay_below_one_shrinks_anchor() {
        let anchor = FIXED_POINT_ONE / 2;
        let rate = FIXED_POINT_ONE - 10u128.pow(20); // < 1
        let projected = compute_current_redemption_price(anchor, rate, 0, 100);
        assert!(projected < anchor);
        assert!(projected > 0);
    }

    #[test]
    fn current_redemption_price_growth_above_one_grows_anchor() {
        let anchor = FIXED_POINT_ONE / 2;
        let rate = FIXED_POINT_ONE + 10u128.pow(20); // > 1
        let projected = compute_current_redemption_price(anchor, rate, 0, 100);
        assert!(projected > anchor);
    }

    #[test]
    fn current_redemption_price_clamps_elapsed_to_maximum_window() {
        let anchor = FIXED_POINT_ONE;
        let rate = FIXED_POINT_ONE - 1_500_000_000_000_000; // < 1, decaying
        let at_window = compute_current_redemption_price(
            anchor,
            rate,
            0,
            MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        assert_eq!(
            compute_current_redemption_price(anchor, rate, 0, u64::MAX),
            at_window,
        );
        assert!(at_window < anchor);
    }
}
