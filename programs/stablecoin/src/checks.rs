//! Shared validation helpers: the §6.2 collateralization invariant, global-account
//! validation, admin authorization, and the §8 parameter bands.
//!
//! The bands live here rather than in `initialize_program` because spec §8 requires
//! the same limits at bootstrap *and* in every `set_*` instruction. One definition
//! means the two can't drift apart.

use alloy_primitives::U512;
use lee_core::{account::AccountId, program::AccountMeta};
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
#[expect(
    clippy::arithmetic_side_effects,
    reason = "the collateral side cannot overflow U512: `u128::MAX * FIXED_POINT_ONE^3` is about 10^119, and only the debt side needs to saturate"
)]
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

// --- Sane-band constants per spec §8 -----------------------------------------

pub(crate) const MAX_STABILITY_FEE_PER_MILLISECOND: u128 = FIXED_POINT_ONE * 2;
pub(crate) const MIN_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 110 / 100; // 1.1x
pub(crate) const MAX_COLLATERALIZATION_RATIO: u128 = FIXED_POINT_ONE * 10;
// Gain magnitude caps (spec §8; placeholders pending the §15 tuning pass):
// |Kp| <= FIXED_POINT_ONE * 10^3, |Ki| <= FIXED_POINT_ONE.
pub(crate) const MAX_PROPORTIONAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE * 1_000;
pub(crate) const MAX_INTEGRAL_GAIN_MAGNITUDE: u128 = FIXED_POINT_ONE;
pub(crate) const MAX_TIMING_MILLISECONDS: u64 = 86_400_000; // 1 day

/// Plan half of the admin gate: the caller signed.
///
/// The other half — that the caller is the handle bound as
/// `ProtocolParameters.admin_account_id` — is stored in that shard and nowhere
/// else, so it travels into the apply as
/// [`UpdateParameters::expected_admin`](stablecoin_core::UpdateParameters) and is
/// checked by [`assert_is_stored_admin`].
///
/// # Panics
/// - `"Admin authorization is missing"` when `admin` did not sign.
pub fn assert_admin_signed(admin: &AccountMeta) {
    assert!(admin.is_authorized, "Admin authorization is missing");
}

/// Apply half of the admin gate: the account the plan saw sign is the stored
/// admin.
///
/// Rotating the admin takes effect immediately for every later instruction,
/// since this always reads the stored handle.
///
/// # Panics
/// - `"Signer is not the protocol's admin"` when the handle does not match.
pub fn assert_is_stored_admin(stored_admin: AccountId, expected_admin: AccountId) {
    assert_eq!(
        stored_admin, expected_admin,
        "Signer is not the protocol's admin"
    );
}

/// Spec §8: `FIXED_POINT_ONE <= rate <= 2 x FIXED_POINT_ONE`.
///
/// The lower bound keeps the fee from decaying debt; the upper bound is an
/// anti-typo cap, not an overflow guard — that is the compounding-window clamp.
pub fn assert_stability_fee_in_band(rate: u128, label: &str) {
    assert!(rate >= FIXED_POINT_ONE, "{label} below FIXED_POINT_ONE");
    assert!(
        rate <= MAX_STABILITY_FEE_PER_MILLISECOND,
        "{label} above sane upper bound"
    );
}

/// Spec §8: `1.1x <= ratio <= 10x`. Below 1.1x a position is insolvent on arrival.
pub fn assert_collateralization_ratio_in_band(ratio: u128, label: &str) {
    assert!(ratio >= MIN_COLLATERALIZATION_RATIO, "{label} below 1.1x");
    assert!(ratio <= MAX_COLLATERALIZATION_RATIO, "{label} above 10x");
}

/// Spec §8 magnitude caps on the PI gains. Either sign is legal; only the
/// magnitude is bounded.
pub fn assert_controller_gains_in_band(proportional_gain: i128, integral_gain: i128) {
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
pub fn assert_timing_milliseconds_in_band(milliseconds: u64, label: &str) {
    assert!(milliseconds >= 1, "{label} below minimum 1ms");
    assert!(
        milliseconds <= MAX_TIMING_MILLISECONDS,
        "{label} above maximum 86_400_000ms"
    );
}
