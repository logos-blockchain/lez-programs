use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_price_observations_pda, record_observation,
    PriceObservations,
};

/// Plan half: verify both PDAs against `price_source_id`.
///
/// Permissionless — anyone may call this. The PDA checks are what ensure the tick
/// being recorded was written by whoever controls that price source.
///
/// Nothing is read here. The tick lives in the `CurrentTickAccount` shard and the
/// time in the clock's, and the apply that integrates them sees neither — so both
/// arrive as declared values with a guard each
/// ([`crate::update_current_tick::check_current_tick`],
/// [`crate::shared::check_clock`]).
///
/// # Panics
/// Panics if either account ID does not match its expected PDA derivation.
pub fn plan_record_tick(
    price_observations: &AccountMeta,
    current_tick_account: &AccountMeta,
    price_source_id: AccountId,
    window_duration: u64,
    oracle_program_id: AccountId,
) {
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(oracle_program_id, price_source_id),
        "RecordTick: current tick account ID does not match expected PDA"
    );
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(oracle_program_id, price_source_id, window_duration),
        "RecordTick: price observations account ID does not match expected PDA"
    );
}

/// Apply half on the observations shard: append the tick, or keep the shard.
///
/// `None` is the sampling guard: fewer than `window_duration / OBSERVATIONS_CAPACITY`
/// milliseconds since the last observation means this call changes nothing, which
/// is exactly what returning every account unchanged used to express. Callers may
/// still invoke `RecordTick` on every block.
///
/// # Panics
/// If the shard does not hold a valid [`PriceObservations`].
#[must_use]
pub fn append_observation(
    pre_data: &ShardData,
    tick: i32,
    now: u64,
    window_duration: u64,
) -> Option<ShardData> {
    let observations = PriceObservations::try_from(pre_data)
        .expect("RecordTick: price observations account must be initialized");

    record_observation(&observations, tick, now, window_duration)
        .as_ref()
        .map(ShardData::from)
}
