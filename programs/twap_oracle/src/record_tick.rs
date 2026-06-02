use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ProgramId},
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_price_observations_pda, CurrentTickAccount,
    ObservationEntry, PriceObservations, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
};

/// Records the current tick from a [`CurrentTickAccount`] into a [`PriceObservations`] ring
/// buffer.
///
/// Both PDAs are verified against `price_source_id`, ensuring the tick was written by whoever
/// controls that price source. The sampling guard silently returns all accounts unchanged when
/// less than `window_duration / OBSERVATIONS_CAPACITY` milliseconds have elapsed since the last
/// observation — callers may invoke this on every block without correctness concerns.
///
/// The timestamp is taken from `clock`, which must be [`CLOCK_01_PROGRAM_ACCOUNT_ID`] — the same
/// canonical 1-block clock the observations account was seeded from. It drives the sampling guard,
/// the new observation's timestamp, and the `tick × elapsed_ms` accumulator, so a forged or
/// caller-controlled clock could skew the TWAP arbitrarily; it must never be caller-supplied.
///
/// Tick-delta truncation clamps the per-observation price move to [`MAX_TICK_DELTA`] before
/// advancing the accumulator. `last_recorded_tick` is updated to the raw (untruncated) tick so
/// the next delta is computed from the true price position.
///
/// # Panics
/// Panics if:
/// - `current_tick_account.account_id` does not match
///   `compute_current_tick_account_pda(oracle_program_id, price_source_id)`.
/// - `price_observations.account_id` does not match
///   `compute_price_observations_pda(oracle_program_id, price_source_id, window_duration)`.
/// - `clock.account_id` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`].
/// - Either account is not a valid initialised account of its respective type.
pub fn record_tick(
    price_observations: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    price_source_id: AccountId,
    window_duration: u64,
    oracle_program_id: ProgramId,
) -> Vec<AccountPostState> {
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
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "RecordTick: clock account must be the canonical 1-block LEZ clock account"
    );

    let clock_data = ClockAccountData::from_bytes(clock.account.data.as_ref());
    let now = clock_data.timestamp;

    let current_tick_data = CurrentTickAccount::try_from(&current_tick_account.account.data)
        .expect("RecordTick: current tick account must be initialized");

    let mut observations = PriceObservations::try_from(&price_observations.account.data)
        .expect("RecordTick: price observations account must be initialized");

    let capacity =
        usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");

    // Sampling guard: enforce minimum interval between observations. Floored to 1 so a degenerate
    // `window_duration < OBSERVATIONS_CAPACITY` (rejected at creation, but guarded here too) cannot
    // zero out the interval and let same-timestamp writes through.
    let min_interval = window_duration
        .checked_div(u64::from(OBSERVATIONS_CAPACITY))
        .expect("OBSERVATIONS_CAPACITY is non-zero")
        .max(1);
    let last_index = if observations.write_index == 0 {
        capacity
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero")
    } else {
        usize::try_from(
            observations
                .write_index
                .checked_sub(1)
                .expect("write_index > 0"),
        )
        .expect("write_index - 1 fits in usize")
    };
    let last_entry = observations
        .entries
        .get(last_index)
        .expect("last_index is within bounds");
    let last_timestamp = last_entry.timestamp;
    let last_cumulative = last_entry.tick_cumulative;
    let elapsed_ms = now.saturating_sub(last_timestamp);

    if elapsed_ms < min_interval {
        return vec![
            AccountPostState::new(price_observations.account.clone()),
            AccountPostState::new(current_tick_account.account.clone()),
            AccountPostState::new(clock.account.clone()),
        ];
    }

    // Cap the per-observation tick move (anti-manipulation), then integrate the resulting *tick*
    // — not the delta — so a constant tick still accumulates `tick × dt` per the TWAP formula.
    let current_tick = current_tick_data.tick;
    let delta = current_tick.saturating_sub(observations.last_recorded_tick);
    let clamped_delta = delta.clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA);
    let clamped_tick = observations
        .last_recorded_tick
        .saturating_add(clamped_delta);

    // Advance cumulative (tick × elapsed milliseconds).
    let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let new_cumulative = i64::from(clamped_tick)
        .checked_mul(elapsed_ms_i64)
        .and_then(|product| last_cumulative.checked_add(product))
        .expect("tick_cumulative fits in i64");

    // Write new entry and advance the ring buffer.
    let write_index = usize::try_from(observations.write_index).expect("write_index fits in usize");
    *observations
        .entries
        .get_mut(write_index)
        .expect("write_index is within bounds") = ObservationEntry {
        timestamp: now,
        tick_cumulative: new_cumulative,
    };
    let next_index = write_index
        .checked_add(1)
        .expect("write_index + 1 fits in usize")
        .checked_rem(capacity)
        .expect("capacity is non-zero");
    observations.write_index = u32::try_from(next_index).expect("next write_index fits in u32");
    observations.total_entries = observations
        .total_entries
        .checked_add(1)
        .expect("total_entries does not overflow");
    observations.last_recorded_tick = current_tick;

    let mut price_observations_post = price_observations.account.clone();
    price_observations_post.data = Data::from(&observations);

    vec![
        AccountPostState::new(price_observations_post),
        AccountPostState::new(current_tick_account.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{Account, AccountId, Nonce};
    use twap_oracle_core::{
        compute_current_tick_account_pda, compute_price_observations_pda, OBSERVATIONS_CAPACITY,
    };

    use super::*;

    const ORACLE_PROGRAM_ID: ProgramId = [77u32; 8];
    const CLOCK_PROGRAM_ID: ProgramId = [88u32; 8];
    const WINDOW_24H: u64 = 24 * 60 * 60 * 1_000;

    fn price_source_id() -> AccountId {
        AccountId::new([1u8; 32])
    }

    /// Minimum interval enforced by the sampling guard for `WINDOW_24H`.
    fn min_interval() -> u64 {
        WINDOW_24H
            .checked_div(u64::from(OBSERVATIONS_CAPACITY))
            .expect("OBSERVATIONS_CAPACITY is non-zero")
    }

    fn clock_account_with_id(timestamp: u64, account_id: AccountId) -> AccountWithMetadata {
        let data = ClockAccountData {
            block_id: 0,
            timestamp,
        }
        .to_bytes();
        AccountWithMetadata {
            account: Account {
                program_owner: CLOCK_PROGRAM_ID,
                balance: 0,
                data: Data::try_from(data).expect("ClockAccountData fits in Data"),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id,
        }
    }

    fn clock_account_with_timestamp(timestamp: u64) -> AccountWithMetadata {
        clock_account_with_id(timestamp, CLOCK_01_PROGRAM_ACCOUNT_ID)
    }

    fn make_current_tick_account(tick: i32, last_updated: u64) -> AccountWithMetadata {
        let stored = CurrentTickAccount { tick, last_updated };
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&stored),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
        }
    }

    /// Builds a [`PriceObservations`] placing a seeded entry at the slot just before
    /// `write_index` so `record_tick` reads it as the last observation.
    fn make_price_observations(
        write_index: u32,
        last_recorded_tick: i32,
        last_timestamp: u64,
        last_cumulative: i64,
    ) -> AccountWithMetadata {
        let capacity =
            usize::try_from(OBSERVATIONS_CAPACITY).expect("OBSERVATIONS_CAPACITY fits in usize");
        let last_index = if write_index == 0 {
            capacity
                .checked_sub(1)
                .expect("OBSERVATIONS_CAPACITY is non-zero")
        } else {
            usize::try_from(write_index.checked_sub(1).expect("write_index > 0"))
                .expect("write_index - 1 fits in usize")
        };
        let mut entries = vec![ObservationEntry::default(); capacity];
        *entries
            .get_mut(last_index)
            .expect("last_index is within bounds") = ObservationEntry {
            timestamp: last_timestamp,
            tick_cumulative: last_cumulative,
        };
        let obs = PriceObservations {
            price_source_id: price_source_id(),
            write_index,
            total_entries: 1,
            last_recorded_tick,
            entries,
        };
        AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&obs),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_price_observations_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        }
    }

    // ── happy path ────────────────────────────────────────────────────────────

    #[test]
    fn returns_three_post_states() {
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(post_states.len(), 3);
    }

    #[test]
    fn cumulative_advances_correctly() {
        // last_recorded_tick = 0, current_tick = 100, elapsed = 10_000 ms
        // expected: 0 + 100 * 10_000 = 1_000_000
        let elapsed_ms = 10_000u64.max(min_interval());
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = 100_i64
            .checked_mul(elapsed_ms_i64)
            .expect("100 * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn cumulative_advances_from_non_zero_base() {
        // last_cumulative = 500_000, current_tick = 50, elapsed = 20_000 ms
        // expected: 500_000 + 50 * 20_000 = 1_500_000
        let elapsed_ms = 20_000u64.max(min_interval());
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 500_000),
            make_current_tick_account(50, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = 500_000_i64
            .checked_add(
                50_i64
                    .checked_mul(elapsed_ms_i64)
                    .expect("50 * elapsed_ms fits in i64"),
            )
            .expect("500_000 + 50 * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn negative_tick_decrements_cumulative() {
        // last_recorded_tick = 0, current_tick = -100, elapsed = 10_000 ms
        // expected: 0 + (-100) * 10_000 = -1_000_000
        let elapsed_ms = 10_000u64.max(min_interval());
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(-100, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = (-100_i64)
            .checked_mul(elapsed_ms_i64)
            .expect("-100 * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn new_observation_timestamp_is_clock_timestamp() {
        let now = min_interval()
            .checked_mul(2)
            .expect("min_interval * 2 fits in u64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(now),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.entries[1].timestamp, now);
    }

    #[test]
    fn write_index_advances_after_write() {
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.write_index, 2);
    }

    #[test]
    fn write_index_wraps_at_capacity() {
        // write_index = CAPACITY - 1 → entry written to slot CAPACITY - 1 → write_index wraps to 0
        let capacity_minus_one = OBSERVATIONS_CAPACITY
            .checked_sub(1)
            .expect("OBSERVATIONS_CAPACITY is non-zero");
        let post_states = record_tick(
            make_price_observations(capacity_minus_one, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.write_index, 0);
    }

    #[test]
    fn total_entries_incremented_after_write() {
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.total_entries, 2);
    }

    #[test]
    fn last_recorded_tick_updated_to_untruncated_current_tick() {
        // Delta large enough to be clamped, but last_recorded_tick must store the raw tick.
        let current_tick = MAX_TICK_DELTA
            .checked_mul(3)
            .expect("MAX_TICK_DELTA * 3 fits in i32");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(current_tick, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.last_recorded_tick, current_tick);
    }

    #[test]
    fn current_tick_and_clock_post_states_are_unchanged() {
        let tick_account = make_current_tick_account(100, 0);
        let clock = clock_account_with_timestamp(min_interval());
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            tick_account.clone(),
            clock.clone(),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(*post_states[1].account(), tick_account.account);
        assert_eq!(*post_states[2].account(), clock.account);
    }

    // ── write_index = 0 path ──────────────────────────────────────────────────

    #[test]
    fn write_index_zero_reads_from_last_slot() {
        // write_index = 0 means the last written entry is at CAPACITY - 1.
        // Exercises the write_index == 0 branch in last_index computation.
        // The new entry goes to slot 0 and write_index advances to 1.
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(0, 0, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = 100_i64
            .checked_mul(elapsed_ms_i64)
            .expect("100 * elapsed_ms fits in i64");
        assert_eq!(obs.write_index, 1);
        assert_eq!(obs.entries[0].timestamp, elapsed_ms);
        assert_eq!(obs.entries[0].tick_cumulative, expected);
    }

    // ── sampling guard ────────────────────────────────────────────────────────

    #[test]
    fn sampling_guard_skips_write_when_elapsed_below_min_interval() {
        let before_interval = min_interval().checked_sub(1).expect("min_interval > 0");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(before_interval),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.write_index, 1);
        assert_eq!(obs.total_entries, 1);
    }

    #[test]
    fn sampling_guard_does_not_update_last_recorded_tick() {
        // When the guard fires, the entire observations account must be returned unchanged —
        // including last_recorded_tick, which is the baseline for the next delta computation.
        let before_interval = min_interval().checked_sub(1).expect("min_interval > 0");
        let post_states = record_tick(
            make_price_observations(1, 500, 0, 0),
            make_current_tick_account(999, 0),
            clock_account_with_timestamp(before_interval),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.last_recorded_tick, 500);
    }

    /// A degenerate `window_duration < OBSERVATIONS_CAPACITY` floors the raw interval to zero. The
    /// `.max(1)` defense-in-depth must still block a same-timestamp (`elapsed_ms = 0`) write, so a
    /// caller cannot trample the ring buffer even if such an account ever existed. (Creation
    /// rejects these windows; this guards `record_tick` independently.)
    #[test]
    fn min_interval_floored_to_one_blocks_same_timestamp_write() {
        let tiny_window = 1u64; // 1 / OBSERVATIONS_CAPACITY == 0 before the floor
        let last_timestamp = 5_000u64;
        let mut observations = make_price_observations(1, 0, last_timestamp, 0);
        observations.account_id =
            compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), tiny_window);
        let post_states = record_tick(
            observations,
            make_current_tick_account(100, 0),
            clock_account_with_timestamp(last_timestamp), // same timestamp → elapsed_ms = 0
            price_source_id(),
            tiny_window,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.write_index, 1, "guard must fire: no entry written");
        assert_eq!(
            obs.total_entries, 1,
            "guard must fire: total_entries unchanged"
        );
    }

    #[test]
    fn sampling_guard_allows_write_at_exactly_min_interval() {
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        assert_eq!(obs.write_index, 2);
        assert_eq!(obs.total_entries, 2);
    }

    // ── tick-delta truncation ─────────────────────────────────────────────────

    #[test]
    fn large_positive_delta_clamped_to_max_tick_delta() {
        // current_tick = MAX_TICK_DELTA * 2, last_recorded_tick = 0 → clamped to MAX_TICK_DELTA
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let double_max = MAX_TICK_DELTA
            .checked_mul(2)
            .expect("MAX_TICK_DELTA * 2 fits in i32");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(double_max, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = i64::from(MAX_TICK_DELTA)
            .checked_mul(elapsed_ms_i64)
            .expect("MAX_TICK_DELTA * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn large_negative_delta_clamped_to_min_tick_delta() {
        // current_tick = -(MAX_TICK_DELTA * 2), last_recorded_tick = 0 → clamped to -MAX_TICK_DELTA
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let neg_double_max = MAX_TICK_DELTA
            .checked_mul(2)
            .and_then(|v| v.checked_neg())
            .expect("-(MAX_TICK_DELTA * 2) fits in i32");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(neg_double_max, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let neg_max = MAX_TICK_DELTA
            .checked_neg()
            .expect("negation of MAX_TICK_DELTA fits in i32");
        let expected = i64::from(neg_max)
            .checked_mul(elapsed_ms_i64)
            .expect("-MAX_TICK_DELTA * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn delta_exactly_at_max_tick_delta_passes_through_untruncated() {
        // delta == MAX_TICK_DELTA is at the boundary — clamp() must not reduce it.
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(MAX_TICK_DELTA, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = i64::from(MAX_TICK_DELTA)
            .checked_mul(elapsed_ms_i64)
            .expect("MAX_TICK_DELTA * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    /// Regression: with a constant non-zero tick the delta is zero, but the cumulative must still
    /// advance by `tick × dt`. The earlier formula integrated the delta and would freeze here.
    #[test]
    fn constant_nonzero_tick_still_accumulates() {
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 100, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = 100_i64
            .checked_mul(elapsed_ms_i64)
            .expect("100 * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    /// With a non-zero baseline, a jump beyond `MAX_TICK_DELTA` integrates the *clamped tick*
    /// (`last_recorded_tick + MAX_TICK_DELTA`), not the clamped delta alone.
    #[test]
    fn clamped_tick_integrates_baseline_plus_max_delta() {
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let baseline = 1_000_i32;
        let current_tick = baseline
            .checked_add(MAX_TICK_DELTA.checked_mul(2).expect("fits in i32"))
            .expect("fits in i32");
        let post_states = record_tick(
            make_price_observations(1, baseline, 0, 0),
            make_current_tick_account(current_tick, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let clamped_tick = baseline
            .checked_add(MAX_TICK_DELTA)
            .expect("baseline + MAX_TICK_DELTA fits in i32");
        let expected = i64::from(clamped_tick)
            .checked_mul(elapsed_ms_i64)
            .expect("clamped_tick * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    #[test]
    fn small_delta_passes_through_untruncated() {
        // current_tick = 100, last_recorded_tick = 0 → delta 100, well within MAX_TICK_DELTA
        let elapsed_ms = min_interval();
        let elapsed_ms_i64 = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
        let post_states = record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_timestamp(elapsed_ms),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
        let obs = PriceObservations::try_from(&post_states[0].account().data)
            .expect("valid PriceObservations");
        let expected = 100_i64
            .checked_mul(elapsed_ms_i64)
            .expect("100 * elapsed_ms fits in i64");
        assert_eq!(obs.entries[1].tick_cumulative, expected);
    }

    // ── cross-source spoofing ─────────────────────────────────────────────────

    /// An attacker who controls their own price source cannot inject ticks into a victim's
    /// observations account. They would pass the victim's `price_source_id` to match the
    /// observations PDA, but their `current_tick_account` is derived from their own source ID,
    /// so the current tick account PDA check fails.
    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn cannot_inject_tick_into_another_sources_observations() {
        let attacker_source_id = AccountId::new([2u8; 32]);
        let attacker_tick_account = AccountWithMetadata {
            account: Account {
                program_owner: ORACLE_PROGRAM_ID,
                balance: 0,
                data: Data::from(&CurrentTickAccount {
                    tick: 99_999,
                    last_updated: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, attacker_source_id),
        };
        record_tick(
            make_price_observations(1, 0, 0, 0), /* victim's observations (price_source_id =
                                                  * [1u8;32]) */
            attacker_tick_account,
            clock_account_with_timestamp(min_interval()),
            price_source_id(), /* victim's price_source_id — attacker claims this to match
                                * observations */
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    // ── clock validation ──────────────────────────────────────────────────────

    /// The coarser-cadence clock accounts (10-block, 50-block) are rejected: the oracle must read
    /// the same canonical 1-block clock the observations account was seeded from.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn non_canonical_clock_account_id_panics() {
        use clock_core::CLOCK_10_PROGRAM_ACCOUNT_ID;
        record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_id(min_interval(), CLOCK_10_PROGRAM_ACCOUNT_ID),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    /// An attacker cannot supply an account they control — even one whose data deserializes as a
    /// valid [`ClockAccountData`] with a forged timestamp — in place of the system clock. Without
    /// this guard a forged `elapsed_ms` would let the attacker skew the `tick × elapsed_ms`
    /// accumulator arbitrarily.
    #[test]
    #[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
    fn forged_clock_account_panics() {
        record_tick(
            make_price_observations(1, 0, 0, 0),
            make_current_tick_account(100, 0),
            clock_account_with_id(min_interval(), AccountId::new([7u8; 32])),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    // ── precondition violations ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "current tick account ID does not match expected PDA")]
    fn wrong_current_tick_account_id_panics() {
        let mut wrong = make_current_tick_account(0, 0);
        wrong.account_id = AccountId::new([0u8; 32]);
        record_tick(
            make_price_observations(1, 0, 0, 0),
            wrong,
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations account ID does not match expected PDA")]
    fn wrong_price_observations_id_panics() {
        let mut wrong = make_price_observations(1, 0, 0, 0);
        wrong.account_id = AccountId::new([0u8; 32]);
        record_tick(
            wrong,
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "current tick account must be initialized")]
    fn uninitialized_current_tick_account_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id()),
        };
        record_tick(
            make_price_observations(1, 0, 0, 0),
            uninit,
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }

    #[test]
    #[should_panic(expected = "price observations account must be initialized")]
    fn uninitialized_price_observations_panics() {
        let uninit = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_price_observations_pda(
                ORACLE_PROGRAM_ID,
                price_source_id(),
                WINDOW_24H,
            ),
        };
        record_tick(
            uninit,
            make_current_tick_account(0, 0),
            clock_account_with_timestamp(min_interval()),
            price_source_id(),
            WINDOW_24H,
            ORACLE_PROGRAM_ID,
        );
    }
}
