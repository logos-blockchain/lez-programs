//! Pure proportional-integral feedback controller for the redemption rate.
//!
//! See spec §6.4 for the full algorithm. [`run_controller_tick`] consumes one
//! tick of input (the current redemption price, the observed market price, the
//! prior integral term, the gains, and the elapsed window) and produces the new
//! redemption rate plus the new integral term, with anti-windup and
//! rate-explosion clamps applied.

use alloy_primitives::I256;

use crate::math::FIXED_POINT_ONE;

/// Anti-windup clamp on the integral term, per spec §8:
/// `± FIXED_POINT_ONE × 10^6`.
///
/// This is a HARD clamp — v1's deliberate divergence from RAI's leaky
/// integrator. Placeholder pending the §15 simulation tuning pass.
pub const INTEGRAL_CLAMP: i128 = (FIXED_POINT_ONE as i128) * 1_000_000;

/// Per-update rate-adjustment clamp (rate-explosion guard), per spec §8:
/// `± FIXED_POINT_ONE / 100_000` — about ±0.001% per call, so even a
/// once-per-second keeper cadence caps near ±1%/s.
///
/// Placeholder pending the §15 simulation tuning pass.
pub const RATE_DELTA_CLAMP: i128 = (FIXED_POINT_ONE / 100_000) as i128;

/// Result of one controller tick — the two values the caller persists into
/// [`crate::RedemptionPriceState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerOutput {
    /// New `redemption_rate_per_millisecond` (fixed-point, unsigned).
    /// Guaranteed `> 0` and within
    /// `[FIXED_POINT_ONE − RATE_DELTA_CLAMP, FIXED_POINT_ONE + RATE_DELTA_CLAMP]`.
    pub redemption_rate_per_millisecond: u128,
    /// Updated integral term (signed fixed-point), hard-clamped to
    /// `[−INTEGRAL_CLAMP, +INTEGRAL_CLAMP]`.
    pub controller_integral_term: i128,
}

/// Run one tick of the redemption-rate controller.
///
/// Sign convention: a positive error (redemption price above market price)
/// drives the rate *above* [`FIXED_POINT_ONE`], so the redemption price rises
/// and pulls the market price up toward it — negative feedback, matching RAI's
/// `redemptionRate = RAY + (Kp·error + Ki·∫)`. There is no negation anywhere in
/// this function; operators tune gain *magnitude*, while the stabilizing
/// direction is embedded here.
///
/// `milliseconds_elapsed` is the window since the last update. Callers pass the
/// same value they clamped to [`crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS`]
/// when projecting `current_redemption_price`, which also bounds the integral
/// term's growth per tick.
///
/// # Panics
/// Only on internal arithmetic overflow of the 256-bit intermediates, which the
/// §8 gain bounds and the elapsed-window clamp make unreachable in practice. If
/// it ever fires, a bound has regressed.
#[must_use]
pub fn run_controller_tick(
    current_redemption_price: u128,
    market_price: u128,
    controller_integral_term: i128,
    controller_proportional_gain: i128,
    controller_integral_gain: i128,
    milliseconds_elapsed: u64,
) -> ControllerOutput {
    let fixed_point_one = I256::try_from(FIXED_POINT_ONE).expect("FIXED_POINT_ONE fits I256");

    // error = redemption − market (signed).
    let error = signed(current_redemption_price)
        .checked_sub(signed(market_price))
        .expect("controller: error subtraction overflows I256");

    // proportional_term = Kp × error / FIXED_POINT_ONE
    let proportional_term = I256::try_from(controller_proportional_gain)
        .expect("proportional gain fits I256")
        .checked_mul(error)
        .expect("controller: proportional term overflows I256")
        / fixed_point_one;

    // integral_delta = Ki × error × Δt / FIXED_POINT_ONE
    let integral_delta = I256::try_from(controller_integral_gain)
        .expect("integral gain fits I256")
        .checked_mul(error)
        .expect("controller: integral delta overflows I256")
        .checked_mul(I256::try_from(milliseconds_elapsed).expect("elapsed window fits I256"))
        .expect("controller: integral delta overflows I256")
        / fixed_point_one;

    // Anti-windup: the integral term is pinned at ±INTEGRAL_CLAMP.
    let new_integral_term = clamp_to_magnitude(
        I256::try_from(controller_integral_term)
            .expect("integral term fits I256")
            .checked_add(integral_delta)
            .expect("controller: integral term overflows I256"),
        INTEGRAL_CLAMP,
    );

    // rate_adjustment = proportional_term + new_integral_term, clamped.
    // No negation — the adjustment carries the same sign as the error.
    let rate_adjustment = clamp_to_magnitude(
        proportional_term
            .checked_add(I256::try_from(new_integral_term).expect("integral term fits I256"))
            .expect("controller: rate adjustment overflows I256"),
        RATE_DELTA_CLAMP,
    );

    // RATE_DELTA_CLAMP is far below FIXED_POINT_ONE, so this stays positive;
    // the floor at 1 is belt-and-suspenders against a future clamp change.
    let redemption_rate_per_millisecond = (FIXED_POINT_ONE as i128)
        .saturating_add(rate_adjustment)
        .max(1) as u128;

    ControllerOutput {
        redemption_rate_per_millisecond,
        controller_integral_term: new_integral_term,
    }
}

/// Widen a `u128` to a signed 256-bit intermediate. Always fits — the sign bit
/// of an `I256` is far above `u128::MAX`.
fn signed(value: u128) -> I256 {
    I256::try_from(value).expect("u128 fits I256")
}

/// Clamp `value` to `[−magnitude, +magnitude]` and narrow to `i128`.
///
/// `magnitude` is an `i128`, so anything that survives the clamp fits.
fn clamp_to_magnitude(value: I256, magnitude: i128) -> i128 {
    let upper = I256::try_from(magnitude).expect("clamp magnitude fits I256");
    if value > upper {
        magnitude
    } else if value < -upper {
        -magnitude
    } else {
        i128::try_from(value).expect("clamped value fits i128")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_clamp_matches_spec_section_eight() {
        assert_eq!(INTEGRAL_CLAMP, (FIXED_POINT_ONE as i128) * 1_000_000);
    }

    #[test]
    fn rate_delta_clamp_matches_spec_section_eight() {
        assert_eq!(RATE_DELTA_CLAMP, (FIXED_POINT_ONE / 100_000) as i128);
    }

    #[test]
    fn positive_error_drives_rate_above_one() {
        // redemption > market => coin trading below target. The controller drives
        // the rate UP: the redemption price rises and the market is pulled up
        // toward it (negative feedback, matching RAI's RAY + Kp·error + Ki·∫).
        let output = run_controller_tick(
            FIXED_POINT_ONE,         // redemption = 1.0
            FIXED_POINT_ONE / 2,     // market = 0.5
            0,                       // no accumulated integral
            FIXED_POINT_ONE as i128, // Kp = 1.0
            0,                       // Ki = 0
            1,                       // 1 ms
        );
        assert!(
            output.redemption_rate_per_millisecond > FIXED_POINT_ONE,
            "rate should rise; got {}",
            output.redemption_rate_per_millisecond,
        );
    }

    #[test]
    fn negative_error_drives_rate_below_one() {
        // redemption < market => coin trading above target. The controller drives
        // the rate DOWN, pulling the market back down toward the target.
        let output = run_controller_tick(
            FIXED_POINT_ONE / 2,
            FIXED_POINT_ONE,
            0,
            FIXED_POINT_ONE as i128,
            0,
            1,
        );
        assert!(
            output.redemption_rate_per_millisecond < FIXED_POINT_ONE,
            "rate should fall; got {}",
            output.redemption_rate_per_millisecond,
        );
    }

    #[test]
    fn zero_error_keeps_rate_at_one() {
        let output = run_controller_tick(
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
            0,
            FIXED_POINT_ONE as i128,
            FIXED_POINT_ONE as i128,
            1,
        );
        assert_eq!(output.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(output.controller_integral_term, 0);
    }

    #[test]
    fn zero_error_leaves_a_standing_integral_term_untouched() {
        // With no error there is nothing to integrate, so the accumulated term
        // is carried over verbatim — v1 clamps, it does not leak (spec §6.4).
        let standing = INTEGRAL_CLAMP / 4;
        let output = run_controller_tick(
            FIXED_POINT_ONE,
            FIXED_POINT_ONE,
            standing,
            0,
            FIXED_POINT_ONE as i128,
            60_000,
        );
        assert_eq!(output.controller_integral_term, standing);
    }

    #[test]
    fn anti_windup_clamps_integral_after_many_ticks() {
        let mut integral_term = 0i128;
        // Persistent positive error of 0.5 on every tick. Without the clamp the
        // integral would grow without bound; with it, the magnitude is capped
        // forever.
        for _ in 0..10 {
            let output = run_controller_tick(
                FIXED_POINT_ONE,
                FIXED_POINT_ONE / 2,
                integral_term,
                0,
                FIXED_POINT_ONE as i128, // Ki = 1.0
                3_600_000,               // a 1h tick, in ms
            );
            integral_term = output.controller_integral_term;
        }
        assert_eq!(integral_term, INTEGRAL_CLAMP);
    }

    #[test]
    fn anti_windup_clamps_negative_integral_too() {
        let mut integral_term = 0i128;
        for _ in 0..10 {
            let output = run_controller_tick(
                FIXED_POINT_ONE / 2,
                FIXED_POINT_ONE,
                integral_term,
                0,
                FIXED_POINT_ONE as i128,
                3_600_000,
            );
            integral_term = output.controller_integral_term;
        }
        assert_eq!(integral_term, -INTEGRAL_CLAMP);
    }

    #[test]
    fn rate_clamp_caps_per_tick_change_even_with_huge_gains() {
        // The maximum proportional gain spec §8 allows (|Kp| ≤ FIXED_POINT_ONE × 10^3)
        // against a huge price error. The per-update adjustment must still be
        // capped at RATE_DELTA_CLAMP.
        let proportional_gain = (FIXED_POINT_ONE as i128) * 1_000;
        let output = run_controller_tick(
            FIXED_POINT_ONE * 2, // redemption = 2.0
            FIXED_POINT_ONE / 4, // market = 0.25
            0,
            proportional_gain,
            0,
            1,
        );
        let delta = (output.redemption_rate_per_millisecond as i128) - (FIXED_POINT_ONE as i128);
        assert_eq!(delta, RATE_DELTA_CLAMP);
    }

    #[test]
    fn rate_clamp_caps_per_tick_change_in_the_negative_direction() {
        let proportional_gain = (FIXED_POINT_ONE as i128) * 1_000;
        let output = run_controller_tick(
            FIXED_POINT_ONE / 4,
            FIXED_POINT_ONE * 2,
            0,
            proportional_gain,
            0,
            1,
        );
        let delta = (output.redemption_rate_per_millisecond as i128) - (FIXED_POINT_ONE as i128);
        assert_eq!(delta, -RATE_DELTA_CLAMP);
        assert!(output.redemption_rate_per_millisecond > 0);
    }

    #[test]
    fn saturated_integral_alone_still_respects_the_rate_clamp() {
        // INTEGRAL_CLAMP is far larger than RATE_DELTA_CLAMP, so a saturated
        // integral must not be able to blow past the per-update rate cap.
        let output = run_controller_tick(FIXED_POINT_ONE, FIXED_POINT_ONE, INTEGRAL_CLAMP, 0, 0, 1);
        let delta = (output.redemption_rate_per_millisecond as i128) - (FIXED_POINT_ONE as i128);
        assert_eq!(delta, RATE_DELTA_CLAMP);
    }

    #[test]
    fn extreme_inputs_do_not_overflow() {
        // Worst case the §8 bounds and the §5.3 elapsed-window clamp permit:
        // max gains, an enormous price error, and a full compounding window.
        let output = run_controller_tick(
            u128::MAX / 2,
            0,
            INTEGRAL_CLAMP,
            (FIXED_POINT_ONE as i128) * 1_000,
            FIXED_POINT_ONE as i128,
            crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        assert_eq!(output.controller_integral_term, INTEGRAL_CLAMP);
        assert_eq!(
            output.redemption_rate_per_millisecond,
            FIXED_POINT_ONE + (RATE_DELTA_CLAMP as u128),
        );
    }
}
