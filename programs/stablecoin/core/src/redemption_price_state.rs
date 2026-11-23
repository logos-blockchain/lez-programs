//! Global lazy state for the redemption price and the PI controller.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Redemption-price anchor + PI controller state, lazy form.
///
/// Current redemption price at time `now` =
/// `redemption_price_at_last_update * compound_rate(redemption_rate_per_millisecond, now -
/// last_updated_at) / FIXED_POINT_ONE`. See spec §5.3 and §6.4.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct RedemptionPriceState {
    /// Redemption price at [`Self::last_updated_at`], collateral-per-stablecoin
    /// fixed-point.
    pub redemption_price_at_last_update: u128,
    /// Per-millisecond drift multiplier, fixed-point. Below `FIXED_POINT_ONE` means
    /// decay; above means growth.
    pub redemption_rate_per_millisecond: u128,
    /// Persisted PI integral state, signed. Clamped against windup on every
    /// `update_redemption_rate` (anti-windup bounds live in code constants in
    /// v1; see spec §8).
    pub controller_integral_term: i128,
    /// Unix milliseconds of the last `update_redemption_rate` call (or of
    /// `initialize_program`, for the initial state).
    pub last_updated_at: u64,
}
