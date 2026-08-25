//! Global lazy accumulator for stability fees.
//!
//! Created at [`initialize_program`] time, advanced by `accrue_stability_fee`
//! and by the auto-accrue inline in `set_stability_fee_per_millisecond`. Read by
//! every debt-touching instruction to compute current nominal debt from
//! normalized debt.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// PDA seed domain for the [`StabilityFeeAccumulator`].
const STABILITY_FEE_ACCUMULATOR_PDA_DOMAIN: &[u8] = b"STABILITY_FEE_ACCUMULATOR";

/// Compounded stability-fee multiplier, lazy form.
///
/// The current accumulator at time `now` is
/// `accumulated_rate_at_last_accrual * compound_rate(stability_fee_per_millisecond, now -
/// last_accrued_at) / FIXED_POINT_ONE`. See spec §5.3 for the read-side projection.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct StabilityFeeAccumulator {
    /// Accumulator value at [`Self::last_accrued_at`], fixed-point. Initialized
    /// to `FIXED_POINT_ONE` and monotonically non-decreasing.
    pub accumulated_rate_at_last_accrual: u128,
    /// Unix milliseconds of the last accrual.
    pub last_accrued_at: u64,
}

impl TryFrom<&Data> for StabilityFeeAccumulator {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&StabilityFeeAccumulator> for Data {
    fn from(state: &StabilityFeeAccumulator) -> Self {
        let len =
            borsh::object_length(state).expect("StabilityFeeAccumulator length must be known");
        let mut buf = Vec::with_capacity(len);
        BorshSerialize::serialize(state, &mut buf)
            .expect("StabilityFeeAccumulator serialization should not fail");
        Self::try_from(buf).expect("StabilityFeeAccumulator encoded data should fit into Data")
    }
}

#[must_use]
pub fn compute_stability_fee_accumulator_pda_seed() -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(STABILITY_FEE_ACCUMULATOR_PDA_DOMAIN).as_bytes());
    PdaSeed::new(out)
}

#[must_use]
pub fn compute_stability_fee_accumulator_pda(stablecoin_program_id: ProgramId) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_stability_fee_accumulator_pda_seed(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::FIXED_POINT_ONE;

    fn sample() -> StabilityFeeAccumulator {
        StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: 1_700_000_000,
        }
    }

    #[test]
    fn borsh_roundtrip_preserves_both_fields() {
        let state = sample();
        let data: Data = (&state).into();
        let decoded = StabilityFeeAccumulator::try_from(&data).expect("decode");
        assert_eq!(decoded, state);
    }

    #[test]
    fn borsh_roundtrip_handles_grown_accumulator() {
        let state = StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE * 12345 / 10000, // 1.2345
            last_accrued_at: 2_000_000_000,
        };
        let data: Data = (&state).into();
        let decoded = StabilityFeeAccumulator::try_from(&data).expect("decode");
        assert_eq!(decoded, state);
    }

    #[test]
    fn pda_is_deterministic() {
        let program_id: ProgramId = [7u32; 8];
        assert_eq!(
            compute_stability_fee_accumulator_pda(program_id),
            compute_stability_fee_accumulator_pda(program_id),
        );
    }

    #[test]
    fn pda_differs_from_protocol_parameters_pda() {
        use crate::compute_protocol_parameters_pda;
        let program_id: ProgramId = [7u32; 8];
        assert_ne!(
            compute_stability_fee_accumulator_pda(program_id),
            compute_protocol_parameters_pda(program_id),
        );
    }
}
