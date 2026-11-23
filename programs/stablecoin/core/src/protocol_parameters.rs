//! The engine's configuration, written by the admin setters and by freeze / unfreeze.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};

/// Upper bound on `stability_fee_per_millisecond`, per spec §8: about 100% a
/// year (`1 + 2.1965 × 10^-11` per millisecond; `e^(2.1965e-11 × 3.15576e10) ≈ 2`).
///
/// This is what keeps the stored accumulator inside `u128`. At the bound a full
/// seven-day compounding window grows it by about 1.3%, so no single accrual can
/// overflow, and starting from `1.0` the accumulator needs about 38 years at the
/// maximum fee to approach `u128::MAX`. The old `2 × FIXED_POINT_ONE` cap
/// overflowed the accumulator 38 ms after an accrual.
pub const MAXIMUM_STABILITY_FEE_PER_MILLISECOND: u128 =
    crate::math::FIXED_POINT_ONE + 21_965_000_000_000_000;

/// Upper bound on `controller_proportional_gain`, per spec §8. The band is
/// `0 ..= MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN`: a negative gain is positive
/// feedback and drives the redemption price away from the market.
///
/// The cap is a typo guard, not the safety bound — `RATE_DELTA_CLAMP` bounds what
/// any gain can do to the rate. It stays wide because the gain multiplies a price
/// error whose raw scale depends on the two tokens' decimals (spec §8).
pub const MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN: i128 = 10_i128.pow(30);

/// Upper bound on `controller_integral_gain`, per spec §8. The band is
/// `0 ..= MAXIMUM_CONTROLLER_INTEGRAL_GAIN`, for the same reasons as
/// [`MAXIMUM_CONTROLLER_PROPORTIONAL_GAIN`].
pub const MAXIMUM_CONTROLLER_INTEGRAL_GAIN: i128 = 10_i128.pow(27);

/// Single source of truth for the protocol's configurable knobs.
///
/// Field order is the stored layout.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct ProtocolParameters {
    /// Authority required for every parameter-update and admin-rotation action.
    pub admin_account_id: AccountId,
    /// Authority required for `Freeze` / `Unfreeze`.
    pub freeze_authority_account_id: AccountId,
    /// The token program every stablecoin and collateral token lives in. IMMUTABLE.
    pub token_program_id: AccountId,
    /// The stablecoin's token definition PDA. IMMUTABLE: changing it would break supply
    /// accounting against the existing float.
    pub stablecoin_definition_id: AccountId,
    /// The single accepted collateral's token definition. IMMUTABLE: changing it would orphan
    /// every position vault.
    pub collateral_definition_id: AccountId,
    /// Address of the market price oracle's price actor used by the controller.
    pub market_price_oracle_id: AccountId,
    /// The program the bound oracle's price actor must belong to. Set with its address by the
    /// admin, including when the oracle does not exist yet.
    pub market_price_oracle_program_id: AccountId,
    /// Per-millisecond stability fee multiplier in fixed-point, stored as
    /// `(1 + r_per_millisecond) * FIXED_POINT_ONE`.
    pub stability_fee_per_millisecond: u128,
    /// PI controller `Kp`. Signed.
    pub controller_proportional_gain: i128,
    /// PI controller `Ki`. Signed.
    pub controller_integral_gain: i128,
    /// Minimum collateralization ratio in fixed-point (`1.5 * FIXED_POINT_ONE` = 150%).
    pub minimum_collateralization_ratio: u128,
    /// Minimum milliseconds between successful `UpdateRedemptionRate` calls.
    pub minimum_milliseconds_between_rate_updates: u64,
    /// Reject oracle observations older than this.
    pub maximum_oracle_price_age_milliseconds: u64,
    /// `true` blocks `OpenPosition`, `GenerateDebt`, and `WithdrawCollateral` for positions
    /// with nonzero normalized debt. Debt-free withdrawals remain allowed.
    pub is_frozen: bool,
    /// Minimum positive accrued position debt, in raw stablecoin atomic units.
    pub minimum_debt_amount: u128,
    /// Aggregate accrued debt ceiling for new borrowing, in raw atomic units.
    pub debt_ceiling: u128,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::FIXED_POINT_ONE;

    #[test]
    fn the_maximum_stability_fee_is_about_one_hundred_percent_a_year() {
        // Compounded one week at a time, as keepers re-anchoring would.
        let week = crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS;
        let mut accumulator = FIXED_POINT_ONE;
        for _ in 0..52 {
            accumulator = crate::math::compute_current_accumulated_rate(
                accumulator,
                MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
                0,
                week,
            );
        }
        // 52 weeks is 364 days, so a little under ×2.
        assert!(accumulator > FIXED_POINT_ONE / 10 * 19, "got {accumulator}");
        assert!(accumulator < FIXED_POINT_ONE * 2, "got {accumulator}");
    }

    #[test]
    fn a_full_window_at_the_maximum_fee_cannot_overflow_a_large_accumulator() {
        let anchor = u128::MAX / 2;
        let projected = crate::math::compute_current_accumulated_rate(
            anchor,
            MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
            0,
            crate::math::MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        );
        assert!(projected > anchor);
        assert!(projected < anchor / 100 * 102);
    }
}
