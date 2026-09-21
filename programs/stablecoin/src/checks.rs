//! Shared validation helpers: the §6.2 collateralization invariant, global-account
//! validation, admin authorization, and the §8 parameter bands.
//!
//! The bands live here rather than in `initialize_program` because spec §8 requires
//! the same limits at bootstrap *and* in every `set_*` instruction. One definition
//! means the two can't drift apart.

use alloy_primitives::U512;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramId,
};
use stablecoin_core::{math::FIXED_POINT_ONE, Position, ProtocolParameters};

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
/// 115_792 whole tokens at 18 decimals. `U512` holds the full product of four
/// `u128::MAX` inputs (`(2^128 − 1)^4 < 2^512`), so no input can overflow it.
/// The caller is responsible for projecting `current_accumulator` and
/// `current_redemption_price` forward to the current timestamp (spec §5.3) before
/// calling; this helper only compares.
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

    // No division anywhere: `/ FIXED_POINT_ONE` on the debt side is carried as an
    // extra `× FIXED_POINT_ONE` on the collateral side, keeping the check exact.
    let collateral_value = multiply(
        multiply(multiply(U512::from(position.collateral_amount), one), one),
        one,
    );
    let required_collateral_value = multiply(
        multiply(
            multiply(
                U512::from(position.normalized_debt_amount),
                U512::from(current_accumulator),
            ),
            U512::from(current_redemption_price),
        ),
        U512::from(minimum_collateralization_ratio),
    );

    assert!(
        collateral_value >= required_collateral_value,
        "Position is undercollateralized"
    );
}

// --- Sane-band constants per spec §8 -----------------------------------------

pub(crate) const MAX_STABILITY_FEE_PER_MILLISECOND: u128 = FIXED_POINT_ONE * 2;
pub(crate) const MIN_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 110 / 100; // 1.1x
pub(crate) const MAX_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 10;
// Gain magnitude caps (spec §8; placeholders pending the §15 tuning pass):
// |Kp| <= FIXED_POINT_ONE * 10^3, |Ki| <= FIXED_POINT_ONE.
pub(crate) const MAX_PROPORTIONAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE * 1_000;
pub(crate) const MAX_INTEGRAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE;
pub(crate) const MAX_TIMING_MILLISECONDS: u64 = 86_400_000; // 1 day

/// The two-part gate every admin instruction performs: the caller signed, and the
/// caller is the handle currently bound as `ProtocolParameters.admin_account_id`.
///
/// Rotating the admin (`set_admin`) therefore takes effect immediately for every
/// later instruction, since the check always reads the stored handle.
///
/// # Panics
/// - `"Admin authorization is missing"` when `admin` did not sign.
/// - `"Signer is not the protocol's admin"` when the handle does not match.
pub(crate) fn assert_admin_authorized(
    admin: &AccountWithMetadata,
    parameters: &ProtocolParameters,
) {
    assert!(admin.is_authorized, "Admin authorization is missing");
    assert_eq!(
        admin.account_id, parameters.admin_account_id,
        "Signer is not the protocol's admin"
    );
}

/// Spec §8: `FIXED_POINT_ONE <= rate <= 2 x FIXED_POINT_ONE`.
///
/// The lower bound keeps the fee from decaying debt; the upper bound is an
/// anti-typo cap, not an overflow guard — that is the compounding-window clamp.
pub(crate) fn assert_stability_fee_in_band(rate: u128, label: &str) {
    assert!(rate >= FIXED_POINT_ONE, "{label} below FIXED_POINT_ONE");
    assert!(
        rate <= MAX_STABILITY_FEE_PER_MILLISECOND,
        "{label} above sane upper bound"
    );
}

/// Spec §8: `1.1x <= ratio <= 10x`. Below 1.1x a position is insolvent on arrival.
pub(crate) fn assert_collateralization_ratio_in_band(ratio: u128, label: &str) {
    assert!(ratio >= MIN_COLLATERALIZATION_RATIO, "{label} below 1.1x");
    assert!(ratio <= MAX_COLLATERALIZATION_RATIO, "{label} above 10x");
}

/// Spec §8 magnitude caps on the PI gains. Either sign is legal; only the
/// magnitude is bounded.
pub(crate) fn assert_controller_gains_in_band(proportional_gain: i128, integral_gain: i128) {
    assert!(
        proportional_gain.unsigned_abs() <= MAX_PROPORTIONAL_GAIN_MAGNITUDE,
        "controller_proportional_gain out of band"
    );
    assert!(
        integral_gain.unsigned_abs() <= MAX_INTEGRAL_GAIN_MAGNITUDE,
        "controller_integral_gain out of band"
    );
}

/// Spec §8: `1 <= milliseconds <= 86_400_000`. Zero would allow spam; beyond a
/// day is self-evidently wrong for both the rate interval and oracle staleness.
pub(crate) fn assert_timing_milliseconds_in_band(milliseconds: u64, label: &str) {
    assert!(milliseconds >= 1, "{label} below minimum 1ms");
    assert!(
        milliseconds <= MAX_TIMING_MILLISECONDS,
        "{label} above maximum 86_400_000ms"
    );
}

/// Validate a read-only global: initialized, program-owned, and at its canonical
/// PDA. Returns its `Data` for the caller to decode.
pub(crate) fn decode_global(
    account: &AccountWithMetadata,
    expected_id: AccountId,
    stablecoin_program_id: ProgramId,
    label: &str,
) -> Data {
    assert_ne!(
        account.account,
        Account::default(),
        "{label} account must be initialized"
    );
    assert_eq!(
        account.account.program_owner, stablecoin_program_id,
        "{label} account must be owned by the stablecoin program"
    );
    assert_eq!(
        account.account_id, expected_id,
        "{label} account ID does not match expected PDA derivation"
    );
    account.account.data.clone()
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

    // --- admin authorization + §8 bounds ---

    fn parameters() -> stablecoin_core::ProtocolParameters {
        stablecoin_core::ProtocolParameters::try_from(
            &crate::test_support::protocol_parameters_account(
                crate::test_support::ParameterOverrides::default(),
            )
            .account
            .data,
        )
        .expect("valid ProtocolParameters")
    }

    fn admin_account() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: crate::test_support::admin_id(),
        }
    }

    #[test]
    fn admin_authorization_accepts_the_bound_admin() {
        assert_admin_authorized(&admin_account(), &parameters());
    }

    #[test]
    #[should_panic(expected = "Admin authorization is missing")]
    fn admin_authorization_requires_a_signature() {
        let mut admin = admin_account();
        admin.is_authorized = false;
        assert_admin_authorized(&admin, &parameters());
    }

    #[test]
    #[should_panic(expected = "Signer is not the protocol's admin")]
    fn admin_authorization_rejects_another_signer() {
        // Authorized, but not the handle bound at initialize_program — e.g. the
        // freeze authority, which has its own powers and must not gain the admin's.
        let mut impostor = admin_account();
        impostor.account_id = crate::test_support::freeze_authority_id();
        assert_admin_authorized(&impostor, &parameters());
    }

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

    /// Flooring `normalized × accumulator / FIXED_POINT_ONE` before comparing
    /// understates the debt and tilts the check toward the borrower. Here the true
    /// nominal debt is 1.9, which needs 2.85 collateral at 1.5x — so 2 must fail,
    /// even though a floored nominal debt of 1 would only ask for 1.5.
    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn fractional_nominal_debt_is_not_rounded_down() {
        assert_position_is_collateralized(
            &position_with(2, 1),
            FIXED_POINT_ONE * 19 / 10,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE * 3 / 2,
        );
    }

    #[test]
    fn fractional_nominal_debt_passes_once_fully_covered() {
        assert_position_is_collateralized(
            &position_with(3, 1),
            FIXED_POINT_ONE * 19 / 10,
            FIXED_POINT_ONE,
            FIXED_POINT_ONE * 3 / 2,
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
