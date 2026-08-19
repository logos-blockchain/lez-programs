//! Global lazy state for the redemption price and the PI controller.
//!
//! Created at [`initialize_program`] time, advanced by `update_redemption_rate`.
//! Read by debt-touching instructions to compute the current redemption price
//! for collateralization checks.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

const REDEMPTION_PRICE_STATE_PDA_DOMAIN: &[u8] = b"REDEMPTION_PRICE_STATE";

/// Redemption-price anchor + PI controller state, lazy form.
///
/// Current redemption price at time `now` =
/// `redemption_price_at_last_update * compound_rate(redemption_rate_per_millisecond, now -
/// last_updated_at) / FIXED_POINT_ONE`. See spec §5.3 and §6.4.
#[account_type]
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

impl TryFrom<&Data> for RedemptionPriceState {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&RedemptionPriceState> for Data {
    fn from(state: &RedemptionPriceState) -> Self {
        let len = borsh::object_length(state).expect("RedemptionPriceState length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(state, &mut buf)
            .expect("RedemptionPriceState serialization should not fail");
        Self::try_from(buf).expect("RedemptionPriceState encoded data should fit into Data")
    }
}

#[must_use]
pub fn compute_redemption_price_state_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(REDEMPTION_PRICE_STATE_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

#[must_use]
pub fn compute_redemption_price_state_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_redemption_price_state_pda_seed(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::FIXED_POINT_ONE;

    fn sample() -> RedemptionPriceState {
        RedemptionPriceState {
            redemption_price_at_last_update: FIXED_POINT_ONE / 2, // 0.5 col/sc
            redemption_rate_per_millisecond: FIXED_POINT_ONE,     // no drift
            controller_integral_term: 0,
            last_updated_at: 1_700_000_000,
        }
    }

    #[test]
    fn borsh_roundtrip_initial_state() {
        let state = sample();
        let data: Data = (&state).into();
        let decoded = RedemptionPriceState::try_from(&data).expect("decode");
        assert_eq!(decoded, state);
    }

    #[test]
    fn borsh_roundtrip_handles_negative_integral_term() {
        let state = RedemptionPriceState {
            redemption_price_at_last_update: FIXED_POINT_ONE,
            redemption_rate_per_millisecond: FIXED_POINT_ONE - 10_000_000_000_000_000_000_000_000,
            controller_integral_term: -987_654_321_i128,
            last_updated_at: 1_700_000_001,
        };
        let data: Data = (&state).into();
        let decoded = RedemptionPriceState::try_from(&data).expect("decode");
        assert_eq!(decoded.controller_integral_term, -987_654_321_i128);
        assert_eq!(decoded, state);
    }

    #[test]
    fn pda_is_deterministic() {
        let program_id: ProgramId = [9u32; 8];
        assert_eq!(
            compute_redemption_price_state_pda(program_id),
            compute_redemption_price_state_pda(program_id),
        );
    }

    #[test]
    fn pda_distinct_from_other_global_pdas() {
        use crate::{compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda};
        let program_id: ProgramId = [9u32; 8];
        let me = compute_redemption_price_state_pda(program_id);
        assert_ne!(me, compute_protocol_parameters_pda(program_id));
        assert_ne!(me, compute_stability_fee_accumulator_pda(program_id));
    }
}
