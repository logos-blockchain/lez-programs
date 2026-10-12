//! `Instruction::AccrueStabilityFee` (spec §10.2).
//!
//! Wall-clock time is a declared instruction field pinned against the system
//! `CLOCK_01` shard; the pinned `spel-framework` exposes no `ProgramContext`
//! clock.

use lee_core::{account::ShardData, program::AccountMeta};
use stablecoin_core::{AccrueFee, FeeProjection, StabilityFeeAccumulator};

/// Plan half: check the caller and the two global addresses.
///
/// Permissionless and idempotent — there is no minimum-interval throttle, since
/// the read-side projection keeps every position current regardless of cadence.
/// Allowed while frozen.
///
/// Nothing here reads the accumulator: advancing it is the accumulator shard's
/// own effect, so the only values that have to travel are the fee rate (copied
/// from the declared parameters, which the parameters guard pins) and `now`.
///
/// # Panics
/// - `caller` did not sign.
/// - either global is not at its canonical PDA.
/// - `clock` is not the system `CLOCK_01` shard.
pub fn plan_accrue_stability_fee(
    caller: &AccountMeta,
    protocol_parameters: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    clock: &AccountMeta,
    stablecoin_program_id: lee_core::account::AccountId,
) {
    assert!(caller.is_authorized, "Caller authorization is missing");
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
}

/// Apply half: advance the accumulator to `now` at the declared rate.
///
/// Shared by `AccrueStabilityFee`, `RefreshGlobals` and
/// `SetStabilityFeePerMillisecond` — which is why the rate is carried rather
/// than read from the parameters shard: for that last one the stored rate is
/// already the *new* one by the time this runs, and the accrual must use the old
/// (spec §10.10).
///
/// # Panics
/// If the shard is not a valid [`StabilityFeeAccumulator`].
#[must_use]
pub fn accrue_fee(pre_data: &ShardData, effect: &AccrueFee) -> ShardData {
    let accumulator = StabilityFeeAccumulator::try_from(pre_data)
        .expect("StabilityFeeAccumulator account must be initialized and decode");
    ShardData::from(
        &FeeProjection {
            accumulator,
            stability_fee_per_millisecond: effect.stability_fee_per_millisecond,
            now: effect.now,
        }
        .advanced(),
    )
}

/// The [`AccrueFee`] payload for a declared rate and a declared clock. Shared so
/// the three instructions that accrue cannot disagree.
///
/// The rate is a parameter rather than read off a [`ProtocolParameters`] record
/// because `SetStabilityFeePerMillisecond` accrues at the *old* rate while
/// overwriting the record with the new one.
#[must_use]
pub fn accrue_fee_effect(stability_fee_per_millisecond: u128, now: u64) -> AccrueFee {
    AccrueFee {
        stability_fee_per_millisecond,
        now,
    }
}
