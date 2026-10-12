use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{
    compute_price_observations_pda, ObservationEntry, PriceObservations, OBSERVATIONS_CAPACITY,
};

/// Plan half: verify the observations PDA, the price source's authority and the
/// window, then build the initial ring buffer.
///
/// Authorization is implicit in the PDA relationship: the observations account is
/// derived from `price_source.account_id` and `window_duration`, so whoever
/// controls the price source controls the observations account.
///
/// The emptiness assert the v0.2.5 version made here is now [`crate::shared::create`]'s,
/// and the initial timestamp is the declared `now`, pinned by
/// [`crate::shared::check_clock`] rather than read here.
///
/// # Panics
/// Panics if:
/// - `price_observations.account_id` does not match
///   `compute_price_observations_pda(oracle_program_id, price_source.account_id, window_duration)`.
/// - `price_source.is_authorized` is false (caller does not control the price source account).
/// - `window_duration` is smaller than [`OBSERVATIONS_CAPACITY`]. The sampling interval enforced by
///   `RecordTick` is `window_duration / OBSERVATIONS_CAPACITY` (integer division); a smaller window
///   floors it to zero, disabling the guard and letting same-timestamp writes trample the buffer.
pub fn plan_create_price_observations(
    price_observations: &AccountMeta,
    price_source: &AccountMeta,
    initial_tick: i32,
    window_duration: u64,
    now: u64,
    oracle_program_id: AccountId,
) -> ShardData {
    let price_source_id = price_source.account_id;
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(oracle_program_id, price_source_id, window_duration),
        "CreatePriceObservations: price observations account ID does not match expected PDA"
    );
    assert!(
        price_source.is_authorized,
        "CreatePriceObservations: price source account must be authorized (caller must control it via a PDA)"
    );
    assert!(
        window_duration >= u64::from(OBSERVATIONS_CAPACITY),
        "CreatePriceObservations: window_duration must be >= OBSERVATIONS_CAPACITY so the RecordTick \
         sampling interval (window_duration / OBSERVATIONS_CAPACITY) is at least one millisecond"
    );

    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");
    let mut entries = vec![ObservationEntry::default(); capacity];
    *entries
        .first_mut()
        .expect("OBSERVATIONS_CAPACITY is non-zero") = ObservationEntry {
        timestamp: now,
        tick_cumulative: 0,
    };

    ShardData::from(&PriceObservations {
        price_source_id,
        write_index: 1,
        total_entries: 1,
        last_recorded_tick: initial_tick,
        entries,
    })
}
