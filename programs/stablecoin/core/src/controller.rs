//! Pure proportional-integral feedback controller for the redemption rate.
//!
//! See spec §6.4 for the full algorithm. [`run_controller_tick`] consumes one
//! tick of input (the current redemption price, the observed market price, the
//! prior integral term, the gains, and the elapsed window) and produces the new
//! redemption rate plus the new integral term, with anti-windup and
//! rate-explosion clamps applied.

use alloy_primitives::{I256, U512};

use crate::math::FIXED_POINT_ONE;

/// Anti-windup clamp on the integral term, per spec §8: equal to
/// [`RATE_DELTA_CLAMP`].
///
/// The integral term is added straight into the rate adjustment, so it is in
/// rate units. Bounding it by the output clamp means a saturated integral just
/// saturates the output, and starts moving the rate back the moment the error
/// reverses. A larger bound would let it wind up far past what the output can
/// express and keep the rate pinned long after the market turned.
///
/// This is a HARD clamp — v1's deliberate divergence from RAI's leaky
/// integrator. Placeholder pending the §15 simulation tuning pass.
pub const INTEGRAL_CLAMP: i128 = RATE_DELTA_CLAMP;

/// Rate-adjustment clamp (rate-explosion guard), per spec §8:
/// `± FIXED_POINT_ONE / 10^11`, i.e. `±10^-11` per millisecond.
///
/// The redemption rate compounds every millisecond whether or not a keeper
/// calls in, so this bounds the drift per unit of *time*, not per call. At the
/// clamp the redemption price moves by at most about `×1.37` or `×0.73` a year
/// (`e^±0.315`), and by at most about `0.6%` across a full
/// [`crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS`] window. Together with
/// [`MAXIMUM_REDEMPTION_PRICE`] that keeps every `u128` projection in range.
///
/// Placeholder pending the §15 simulation tuning pass.
pub const RATE_DELTA_CLAMP: i128 = (FIXED_POINT_ONE / 100_000_000_000) as i128;

/// Lowest redemption price the controller will steer toward, per spec §8:
/// `10^9`, i.e. `10^-18` collateral units per stablecoin unit.
///
/// Integer compounding at a rate below `1.0` reaches zero eventually, and a
/// redemption price near zero makes any collateral look like enough to mint
/// against. At or below this floor the controller may only raise the price.
/// Low enough for any realistic pair of token decimals.
pub const MINIMUM_REDEMPTION_PRICE: u128 = 1_000_000_000;

/// Highest redemption price the controller will steer toward, per spec §8:
/// `u128::MAX / 2`.
///
/// At or above this ceiling the controller may only lower the price. A full
/// compounding window at [`RATE_DELTA_CLAMP`] grows the price by about `0.6%`,
/// far less than the `×2` headroom, so the `u128` projection cannot overflow.
pub const MAXIMUM_REDEMPTION_PRICE: u128 = u128::MAX / 2;

/// Clamp a projected redemption price into
/// `[MINIMUM_REDEMPTION_PRICE, MAXIMUM_REDEMPTION_PRICE]`.
///
/// Takes the `U512` projection so a state written before these bounds existed
/// re-anchors instead of panicking.
#[must_use]
pub fn bound_redemption_price(projected_redemption_price: U512) -> u128 {
    let capped = projected_redemption_price.min(U512::from(MAXIMUM_REDEMPTION_PRICE));
    u128::try_from(capped)
        .expect("capped redemption price fits u128")
        .max(MINIMUM_REDEMPTION_PRICE)
}

/// Result of one controller tick — the two values the caller persists into
/// [`crate::RedemptionPriceState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerOutput {
    /// New `redemption_rate_per_millisecond` (fixed-point, unsigned).
    /// Guaranteed `> 0` and within
    /// `[FIXED_POINT_ONE − RATE_DELTA_CLAMP, FIXED_POINT_ONE + RATE_DELTA_CLAMP]`.
    /// Never below `FIXED_POINT_ONE` when the price is at
    /// [`MINIMUM_REDEMPTION_PRICE`] or lower, and never above it when the price is
    /// at [`MAXIMUM_REDEMPTION_PRICE`] or higher.
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
    let unbounded_rate = (FIXED_POINT_ONE as i128)
        .saturating_add(rate_adjustment)
        .max(1) as u128;

    // At a price bound the rate may only point back inside the band.
    let redemption_rate_per_millisecond = if current_redemption_price <= MINIMUM_REDEMPTION_PRICE {
        unbounded_rate.max(FIXED_POINT_ONE)
    } else if current_redemption_price >= MAXIMUM_REDEMPTION_PRICE {
        unbounded_rate.min(FIXED_POINT_ONE)
    } else {
        unbounded_rate
    };

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
        assert_eq!(INTEGRAL_CLAMP, RATE_DELTA_CLAMP);
    }

    #[test]
    fn a_saturated_integral_unwinds_as_soon_as_the_error_reverses() {
        // Saturated upward, then one tick of a reversed error. With Ki = 1.0 and
        // a tiny error held for long enough, the integral moves by more than two
        // clamps, so it lands at the opposite bound and the rate falls at once.
        let output = run_controller_tick(
            FIXED_POINT_ONE,
            FIXED_POINT_ONE + 1_000_000_000_000,
            INTEGRAL_CLAMP,
            0,
            FIXED_POINT_ONE as i128,
            3_600_000,
        );
        assert_eq!(output.controller_integral_term, -INTEGRAL_CLAMP);
        assert!(output.redemption_rate_per_millisecond < FIXED_POINT_ONE);
    }

    #[test]
    fn rate_delta_clamp_matches_spec_section_eight() {
        assert_eq!(
            RATE_DELTA_CLAMP,
            (FIXED_POINT_ONE / 100_000_000_000) as i128
        );
    }

    #[test]
    fn a_full_window_at_the_upward_clamp_cannot_overflow_from_the_ceiling() {
        let rate = FIXED_POINT_ONE + RATE_DELTA_CLAMP as u128;
        let projected = crate::math::compute_current_redemption_price(
            MAXIMUM_REDEMPTION_PRICE,
            rate,
            0,
            crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        // About 0.6% growth over seven days.
        assert!(projected > MAXIMUM_REDEMPTION_PRICE);
        assert!(projected < MAXIMUM_REDEMPTION_PRICE / 100 * 101);
    }

    #[test]
    fn a_full_window_at_the_downward_clamp_stays_well_above_zero() {
        let rate = FIXED_POINT_ONE - RATE_DELTA_CLAMP as u128;
        let projected = crate::math::compute_current_redemption_price(
            MINIMUM_REDEMPTION_PRICE,
            rate,
            0,
            crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        assert!(projected < MINIMUM_REDEMPTION_PRICE);
        assert!(projected > MINIMUM_REDEMPTION_PRICE / 100 * 99);
    }

    #[test]
    fn a_year_at_the_clamp_moves_the_price_by_less_than_half() {
        // Compounded one week at a time, as keepers re-anchoring would.
        let week = crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS;
        let up = FIXED_POINT_ONE + RATE_DELTA_CLAMP as u128;
        let down = FIXED_POINT_ONE - RATE_DELTA_CLAMP as u128;
        let (mut high, mut low) = (FIXED_POINT_ONE, FIXED_POINT_ONE);
        for _ in 0..52 {
            high = crate::math::compute_current_redemption_price(high, up, 0, week);
            low = crate::math::compute_current_redemption_price(low, down, 0, week);
        }
        assert!(high < FIXED_POINT_ONE / 2 * 3, "got {high}");
        assert!(low > FIXED_POINT_ONE / 2, "got {low}");
    }

    #[test]
    fn at_the_price_floor_the_rate_cannot_fall_below_one() {
        // Market far above the redemption price would normally push the rate down.
        let output = run_controller_tick(
            MINIMUM_REDEMPTION_PRICE,
            FIXED_POINT_ONE,
            0,
            FIXED_POINT_ONE as i128,
            0,
            1,
        );
        assert_eq!(output.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    }

    #[test]
    fn at_the_price_floor_the_rate_may_still_rise() {
        let output = run_controller_tick(
            MINIMUM_REDEMPTION_PRICE,
            0,
            0,
            FIXED_POINT_ONE as i128,
            0,
            1,
        );
        assert!(output.redemption_rate_per_millisecond > FIXED_POINT_ONE);
    }

    #[test]
    fn at_the_price_ceiling_the_rate_cannot_rise_above_one() {
        let output = run_controller_tick(
            MAXIMUM_REDEMPTION_PRICE,
            FIXED_POINT_ONE,
            0,
            FIXED_POINT_ONE as i128,
            0,
            1,
        );
        assert_eq!(output.redemption_rate_per_millisecond, FIXED_POINT_ONE);
    }

    #[test]
    fn at_the_price_ceiling_the_rate_may_still_fall() {
        let output = run_controller_tick(
            MAXIMUM_REDEMPTION_PRICE,
            u128::MAX,
            0,
            FIXED_POINT_ONE as i128,
            0,
            1,
        );
        assert!(output.redemption_rate_per_millisecond < FIXED_POINT_ONE);
    }

    #[test]
    fn bound_redemption_price_clamps_into_the_band() {
        assert_eq!(bound_redemption_price(U512::ZERO), MINIMUM_REDEMPTION_PRICE);
        assert_eq!(
            bound_redemption_price(U512::from(FIXED_POINT_ONE)),
            FIXED_POINT_ONE
        );
        assert_eq!(bound_redemption_price(U512::MAX), MAXIMUM_REDEMPTION_PRICE);
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
            MAXIMUM_REDEMPTION_PRICE - 1,
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
