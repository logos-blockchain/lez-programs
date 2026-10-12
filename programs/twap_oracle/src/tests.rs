#![cfg(test)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "fixtures use fixed values to lock boundary behavior, index fixed-capacity buffers, and assert panics deliberately"
)]

//! Unit tests for the TWAP oracle's two phases.
//!
//! The accumulator and TWAP maths moved into `twap_oracle_core` as pure
//! functions ([`record_observation`], [`compute_twap_price`]) so they can run
//! inside an apply half, and they are tested directly here — which is a better
//! home than through a handler that also had to assemble state diffs.
//!
//! Two families of v0.2.5 test have no successor, both because the thing they
//! asserted no longer exists:
//!
//! * `returns_three_post_states` / `returns_four_post_states` — a handler no longer returns a diff
//!   per declared account. It emits effects only for the shards it touches.
//! * `*_post_states_are_unchanged` / `*_account_is_not_modified` — "unchanged" is now simply the
//!   absence of an effect, so there is no post-state to assert on. A guard's apply returning `None`
//!   is the surviving form of that claim, and the runtime rejects a guard that writes.

use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, compute_twap_price, price_to_tick, record_observation,
    tick_to_oracle_price, CurrentTickAccount, ObservationEntry, OraclePriceAccount,
    PriceObservations, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
};

use crate::{
    create_current_tick_account::plan_create_current_tick_account,
    create_oracle_price_account::plan_create_oracle_price_account,
    create_price_observations::plan_create_price_observations,
    publish_price::{check_twap, plan_publish_price, publish_twap},
    record_tick::{append_observation, plan_record_tick},
    shared::{assert_canonical_clock, check_clock, create},
    update_current_tick::{check_current_tick, plan_update_current_tick, write_current_tick},
};

const ORACLE_PROGRAM_ID: AccountId = AccountId::new([77u8; 32]);
const WINDOW: u64 = 3_600_000;
const NOW: u64 = 1_700_000_000_000;
/// `1.0` in Q64.64 — the spot price at tick 0.
const UNIT_PRICE: u128 = 1u128 << 64;

fn price_source_id() -> AccountId {
    AccountId::new([1u8; 32])
}
fn other_price_source_id() -> AccountId {
    AccountId::new([2u8; 32])
}
fn base_asset() -> AccountId {
    AccountId::new([3u8; 32])
}
fn quote_asset() -> AccountId {
    AccountId::new([4u8; 32])
}

/// An oracle-owned slot (the shards this program writes).
fn ours(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, ORACLE_PROGRAM_ID)
}

fn price_source(is_authorized: bool) -> AccountMeta {
    AccountMeta::new(price_source_id(), is_authorized, ORACLE_PROGRAM_ID)
}

fn clock_slot() -> AccountMeta {
    AccountMeta::new(CLOCK_01_PROGRAM_ACCOUNT_ID, false, clock_account_id())
}

fn observations_slot() -> AccountMeta {
    ours(compute_price_observations_pda(
        ORACLE_PROGRAM_ID,
        price_source_id(),
        WINDOW,
    ))
}
fn price_account_slot() -> AccountMeta {
    ours(compute_oracle_price_account_pda(
        ORACLE_PROGRAM_ID,
        price_source_id(),
        WINDOW,
    ))
}
fn current_tick_slot() -> AccountMeta {
    ours(compute_current_tick_account_pda(
        ORACLE_PROGRAM_ID,
        price_source_id(),
    ))
}

fn empty() -> ShardData {
    ShardData::default()
}
fn clock_data(timestamp: u64) -> ShardData {
    ShardData::try_from(
        ClockAccountData {
            block_id: 1,
            timestamp,
        }
        .to_bytes(),
    )
    .expect("a clock record fits in a shard")
}
fn capacity() -> usize {
    usize::try_from(OBSERVATIONS_CAPACITY).unwrap()
}

/// A ring buffer with `entries[..n]` filled from `points`.
fn observations(points: &[(u64, i64)], last_recorded_tick: i32) -> PriceObservations {
    let mut entries = vec![ObservationEntry::default(); capacity()];
    for (i, (timestamp, tick_cumulative)) in points.iter().enumerate() {
        entries[i] = ObservationEntry {
            timestamp: *timestamp,
            tick_cumulative: *tick_cumulative,
        };
    }
    PriceObservations {
        price_source_id: price_source_id(),
        write_index: u32::try_from(points.len()).unwrap(),
        total_entries: u64::try_from(points.len()).unwrap(),
        last_recorded_tick,
        entries,
    }
}

fn as_observations(data: &ShardData) -> PriceObservations {
    PriceObservations::try_from(data).expect("a PriceObservations was written")
}
fn as_current_tick(data: &ShardData) -> CurrentTickAccount {
    CurrentTickAccount::try_from(data).expect("a CurrentTickAccount was written")
}
fn as_price_account(data: &ShardData) -> OraclePriceAccount {
    OraclePriceAccount::try_from(data).expect("an OraclePriceAccount was written")
}

fn current_tick_data(tick: i32, last_updated: u64) -> ShardData {
    ShardData::from(&CurrentTickAccount { tick, last_updated })
}

fn price_account_data(price: u128, timestamp: u64) -> ShardData {
    ShardData::from(&OraclePriceAccount {
        base_asset: base_asset(),
        quote_asset: quote_asset(),
        price,
        timestamp,
        source_id: price_source_id(),
        confidence_interval: 7,
    })
}

// ── the clock, shared by every instruction ──────────────────────────────────

#[test]
fn canonical_clock_slot_is_accepted() {
    assert_canonical_clock(&clock_slot(), "Ctx");
}

#[test]
#[should_panic(expected = "clock account must be the canonical 1-block LEZ clock account")]
fn non_canonical_clock_account_id_panics() {
    assert_canonical_clock(
        &AccountMeta::new(AccountId::new([9u8; 32]), false, clock_account_id()),
        "Ctx",
    );
}

#[test]
fn check_clock_accepts_the_clocks_own_time() {
    check_clock(&clock_data(NOW), NOW);
}

/// Exact, not bounded: an oracle timestamp is data consumers act on, so slack in
/// either direction is unsafe. A forged clock is exactly what this closes.
#[test]
#[should_panic(expected = "Declared timestamp does not match the canonical clock")]
fn check_clock_rejects_an_understated_time() {
    check_clock(&clock_data(NOW), NOW - 1);
}

#[test]
#[should_panic(expected = "Declared timestamp does not match the canonical clock")]
fn check_clock_rejects_an_overstated_time() {
    check_clock(&clock_data(NOW), NOW + 1);
}

#[test]
#[should_panic(expected = "Clock account must be initialized")]
fn check_clock_rejects_an_empty_clock_shard() {
    check_clock(&empty(), NOW);
}

// ── create: the shared empty-shard guard ────────────────────────────────────

#[test]
fn create_writes_into_an_empty_shard() {
    let data = current_tick_data(5, NOW);
    assert_eq!(as_current_tick(&create(&empty(), data)).tick, 5);
}

/// Replaces every `*_account must be uninitialized` test: emptiness is visible
/// only in the apply phase now.
#[test]
#[should_panic(expected = "Target account must not already hold data")]
fn create_refuses_a_shard_that_already_holds_data() {
    let _ = create(&current_tick_data(1, NOW), current_tick_data(2, NOW));
}

// ── CreatePriceObservations ─────────────────────────────────────────────────

#[test]
fn initial_observation_has_zero_cumulative_and_the_declared_timestamp() {
    let data = plan_create_price_observations(
        &observations_slot(),
        &price_source(true),
        42,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
    let o = as_observations(&data);
    assert_eq!(o.entries[0].timestamp, NOW);
    assert_eq!(o.entries[0].tick_cumulative, 0);
    assert_eq!(o.last_recorded_tick, 42, "initial tick is the baseline");
    assert_eq!(o.write_index, 1);
    assert_eq!(o.total_entries, 1);
    assert_eq!(o.price_source_id, price_source_id());
    assert!(
        o.entries[1..]
            .iter()
            .all(|e| *e == ObservationEntry::default()),
        "remaining entries stay default"
    );
}

#[test]
fn negative_initial_tick_is_stored_as_the_baseline() {
    let data = plan_create_price_observations(
        &observations_slot(),
        &price_source(true),
        -500,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
    assert_eq!(as_observations(&data).last_recorded_tick, -500);
}

#[test]
#[should_panic(expected = "price observations account ID does not match expected PDA")]
fn wrong_price_observations_account_id_panics() {
    let _ = plan_create_price_observations(
        &ours(AccountId::new([0xEE; 32])),
        &price_source(true),
        0,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

/// The PDA binds the observations account to one price source, so registering
/// against another source's feed cannot derive.
#[test]
#[should_panic(expected = "price observations account ID does not match expected PDA")]
fn cannot_register_observations_for_another_price_source() {
    let foreign = ours(compute_price_observations_pda(
        ORACLE_PROGRAM_ID,
        other_price_source_id(),
        WINDOW,
    ));
    let _ = plan_create_price_observations(
        &foreign,
        &price_source(true),
        0,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price source account must be authorized")]
fn create_observations_with_unauthorized_price_source_panics() {
    let _ = plan_create_price_observations(
        &observations_slot(),
        &price_source(false),
        0,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "window_duration must be >= OBSERVATIONS_CAPACITY")]
fn window_duration_below_capacity_panics() {
    let window = u64::from(OBSERVATIONS_CAPACITY) - 1;
    let slot = ours(compute_price_observations_pda(
        ORACLE_PROGRAM_ID,
        price_source_id(),
        window,
    ));
    let _ = plan_create_price_observations(
        &slot,
        &price_source(true),
        0,
        window,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
fn window_duration_equal_to_capacity_is_accepted() {
    let window = u64::from(OBSERVATIONS_CAPACITY);
    let slot = ours(compute_price_observations_pda(
        ORACLE_PROGRAM_ID,
        price_source_id(),
        window,
    ));
    let _ = plan_create_price_observations(
        &slot,
        &price_source(true),
        0,
        window,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
fn different_windows_produce_distinct_observation_pdas() {
    assert_ne!(
        compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW),
        compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW * 2),
    );
}

// ── CreateOraclePriceAccount ────────────────────────────────────────────────

#[test]
fn price_account_initialised_with_price_and_declared_timestamp() {
    let data = plan_create_oracle_price_account(
        &price_account_slot(),
        &price_source(true),
        base_asset(),
        quote_asset(),
        UNIT_PRICE,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
    let a = as_price_account(&data);
    assert_eq!(a.price, UNIT_PRICE);
    assert_eq!(a.timestamp, NOW);
    assert_eq!(a.base_asset, base_asset());
    assert_eq!(a.quote_asset, quote_asset());
    assert_eq!(a.source_id, price_source_id(), "source is the price source");
    assert_eq!(a.confidence_interval, 0, "no confidence interval at seed");
}

#[test]
#[should_panic(expected = "initial price must be non-zero")]
fn zero_initial_price_panics() {
    let _ = plan_create_oracle_price_account(
        &price_account_slot(),
        &price_source(true),
        base_asset(),
        quote_asset(),
        0,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "clock timestamp must be non-zero")]
fn zero_clock_timestamp_panics() {
    let _ = plan_create_oracle_price_account(
        &price_account_slot(),
        &price_source(true),
        base_asset(),
        quote_asset(),
        UNIT_PRICE,
        WINDOW,
        0,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "oracle price account ID does not match expected PDA")]
fn wrong_oracle_price_account_id_panics() {
    let _ = plan_create_oracle_price_account(
        &ours(AccountId::new([0xED; 32])),
        &price_source(true),
        base_asset(),
        quote_asset(),
        UNIT_PRICE,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price source account must be authorized")]
fn create_price_account_with_unauthorized_price_source_panics() {
    let _ = plan_create_oracle_price_account(
        &price_account_slot(),
        &price_source(false),
        base_asset(),
        quote_asset(),
        UNIT_PRICE,
        WINDOW,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
fn the_three_oracle_pdas_are_all_distinct() {
    let obs = compute_price_observations_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW);
    let price = compute_oracle_price_account_pda(ORACLE_PROGRAM_ID, price_source_id(), WINDOW);
    let tick = compute_current_tick_account_pda(ORACLE_PROGRAM_ID, price_source_id());
    assert_ne!(obs, price);
    assert_ne!(obs, tick);
    assert_ne!(price, tick);
}

// ── CreateCurrentTickAccount / UpdateCurrentTick ────────────────────────────

#[test]
fn unit_price_stores_tick_zero_and_the_declared_timestamp() {
    let data = plan_create_current_tick_account(
        &current_tick_slot(),
        &price_source(true),
        UNIT_PRICE,
        NOW,
        ORACLE_PROGRAM_ID,
    );
    let a = as_current_tick(&data);
    assert_eq!(a.tick, 0, "1.0 is tick 0");
    assert_eq!(a.last_updated, NOW);
}

#[test]
fn initial_price_is_converted_via_price_to_tick() {
    for price in [UNIT_PRICE / 2, UNIT_PRICE, UNIT_PRICE * 3] {
        let data = plan_create_current_tick_account(
            &current_tick_slot(),
            &price_source(true),
            price,
            NOW,
            ORACLE_PROGRAM_ID,
        );
        assert_eq!(as_current_tick(&data).tick, price_to_tick(price));
    }
}

#[test]
#[should_panic(expected = "current tick account ID does not match expected PDA")]
fn wrong_current_tick_account_id_panics() {
    let _ = plan_create_current_tick_account(
        &ours(AccountId::new([0xEC; 32])),
        &price_source(true),
        UNIT_PRICE,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price source account must be authorized")]
fn create_current_tick_with_unauthorized_price_source_panics() {
    let _ = plan_create_current_tick_account(
        &current_tick_slot(),
        &price_source(false),
        UNIT_PRICE,
        NOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
fn update_converts_the_price_and_rewrites_the_timestamp() {
    let tick = plan_update_current_tick(
        &current_tick_slot(),
        &price_source(true),
        UNIT_PRICE * 2,
        ORACLE_PROGRAM_ID,
    );
    assert_eq!(tick, price_to_tick(UNIT_PRICE * 2));

    let written = write_current_tick(&current_tick_data(0, NOW - 5_000), tick, NOW);
    let a = as_current_tick(&written);
    assert_eq!(a.tick, tick);
    assert_eq!(a.last_updated, NOW);
}

#[test]
#[should_panic(expected = "current tick account must be initialized")]
fn update_of_an_uninitialized_current_tick_account_panics() {
    let _ = write_current_tick(&empty(), 1, NOW);
}

#[test]
#[should_panic(expected = "price source account must be authorized")]
fn update_with_unauthorized_price_source_panics() {
    let _ = plan_update_current_tick(
        &current_tick_slot(),
        &price_source(false),
        UNIT_PRICE,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "current tick account ID does not match expected PDA")]
fn cannot_update_another_price_sources_tick_account() {
    let foreign = ours(compute_current_tick_account_pda(
        ORACLE_PROGRAM_ID,
        other_price_source_id(),
    ));
    let _ = plan_update_current_tick(&foreign, &price_source(true), UNIT_PRICE, ORACLE_PROGRAM_ID);
}

// ── the current-tick guard ──────────────────────────────────────────────────

#[test]
fn check_current_tick_accepts_the_stored_values() {
    check_current_tick(&current_tick_data(123, NOW), 123, Some(NOW));
}

/// `RecordTick` cares only about the tick, so it passes `None` and the timestamp
/// is not constrained.
#[test]
fn check_current_tick_ignores_the_timestamp_when_not_declared() {
    check_current_tick(&current_tick_data(123, NOW), 123, None);
}

#[test]
#[should_panic(expected = "Declared current tick does not match the current tick account")]
fn check_current_tick_rejects_a_forged_tick() {
    check_current_tick(&current_tick_data(123, NOW), 124, None);
}

#[test]
#[should_panic(expected = "Declared current tick timestamp does not match")]
fn check_current_tick_rejects_a_forged_timestamp() {
    check_current_tick(&current_tick_data(123, NOW), 123, Some(NOW + 1));
}

#[test]
#[should_panic(expected = "current tick account must be initialized")]
fn check_current_tick_rejects_an_empty_shard() {
    check_current_tick(&empty(), 0, None);
}

// ── RecordTick: PDA checks ──────────────────────────────────────────────────

#[test]
#[should_panic(expected = "current tick account ID does not match expected PDA")]
fn record_tick_rejects_a_foreign_current_tick_account() {
    plan_record_tick(
        &observations_slot(),
        &ours(AccountId::new([0xEB; 32])),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price observations account ID does not match expected PDA")]
fn record_tick_rejects_a_foreign_observations_account() {
    plan_record_tick(
        &ours(AccountId::new([0xEA; 32])),
        &current_tick_slot(),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

/// The PDAs bind both accounts to the same price source, so a tick cannot be
/// injected into another source's feed.
#[test]
#[should_panic(expected = "current tick account ID does not match expected PDA")]
fn cannot_inject_a_tick_into_another_sources_observations() {
    plan_record_tick(
        &observations_slot(),
        &ours(compute_current_tick_account_pda(
            ORACLE_PROGRAM_ID,
            other_price_source_id(),
        )),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "price observations account must be initialized")]
fn record_tick_on_an_uninitialized_observations_shard_panics() {
    let _ = append_observation(&empty(), 0, NOW, WINDOW);
}

// ── RecordTick: the accumulator (pure, in core) ─────────────────────────────

/// The sampling interval `RecordTick` enforces for [`WINDOW`].
///
/// A fn rather than a const: `u64::from` is not const-callable yet, and an `as`
/// cast is denied by the workspace lints.
fn min_interval() -> u64 {
    WINDOW / u64::from(OBSERVATIONS_CAPACITY)
}

#[test]
fn cumulative_advances_by_tick_times_elapsed() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, 10, NOW + min_interval(), WINDOW).expect("interval elapsed");
    assert_eq!(
        next.entries[1].tick_cumulative,
        10 * i64::try_from(min_interval()).unwrap()
    );
    assert_eq!(next.entries[1].timestamp, NOW + min_interval());
    assert_eq!(next.write_index, 2, "write index advances");
    assert_eq!(next.total_entries, 2);
    assert_eq!(next.last_recorded_tick, 10);
}

#[test]
fn cumulative_advances_from_a_non_zero_base() {
    let o = observations(&[(NOW, 1_000)], 0);
    let next = record_observation(&o, 10, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        1_000 + 10 * i64::try_from(min_interval()).unwrap()
    );
}

#[test]
fn negative_tick_decrements_the_cumulative() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, -10, NOW + min_interval(), WINDOW).unwrap();
    assert!(next.entries[1].tick_cumulative < 0);
}

#[test]
fn a_constant_non_zero_tick_still_accumulates() {
    let o = observations(&[(NOW, 0)], 7);
    let next = record_observation(&o, 7, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        7 * i64::try_from(min_interval()).unwrap(),
        "a flat tick integrates tick x dt, not the delta"
    );
}

#[test]
fn sampling_guard_keeps_the_shard_when_below_min_interval() {
    let o = observations(&[(NOW, 0)], 0);
    assert!(
        record_observation(&o, 10, NOW + min_interval() - 1, WINDOW).is_none(),
        "too soon means no write at all"
    );
}

#[test]
fn sampling_guard_allows_a_write_at_exactly_min_interval() {
    let o = observations(&[(NOW, 0)], 0);
    assert!(record_observation(&o, 10, NOW + min_interval(), WINDOW).is_some());
}

#[test]
fn sampling_guard_does_not_advance_the_baseline_tick() {
    let o = observations(&[(NOW, 0)], 3);
    assert!(record_observation(&o, 99, NOW, WINDOW).is_none());
    assert_eq!(o.last_recorded_tick, 3, "the skipped call changes nothing");
}

#[test]
fn min_interval_floors_to_one_so_same_timestamp_writes_are_blocked() {
    let o = observations(&[(NOW, 0)], 0);
    // A degenerate window would otherwise floor the interval to zero.
    assert!(record_observation(&o, 10, NOW, 1).is_none());
}

#[test]
fn large_positive_delta_is_clamped_to_max_tick_delta() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, MAX_TICK_DELTA * 10, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        i64::from(MAX_TICK_DELTA) * i64::try_from(min_interval()).unwrap(),
        "the integrated tick is capped"
    );
    assert_eq!(
        next.last_recorded_tick,
        MAX_TICK_DELTA * 10,
        "the baseline keeps the raw tick so the next delta is true"
    );
}

#[test]
fn large_negative_delta_is_clamped_to_min_tick_delta() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, -MAX_TICK_DELTA * 10, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        -i64::from(MAX_TICK_DELTA) * i64::try_from(min_interval()).unwrap()
    );
}

#[test]
fn delta_exactly_at_the_bound_passes_through_untruncated() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, MAX_TICK_DELTA, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        i64::from(MAX_TICK_DELTA) * i64::try_from(min_interval()).unwrap()
    );
}

#[test]
fn small_delta_passes_through_untruncated() {
    let o = observations(&[(NOW, 0)], 0);
    let next = record_observation(&o, 5, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[1].tick_cumulative,
        5 * i64::try_from(min_interval()).unwrap()
    );
}

#[test]
fn write_index_wraps_at_capacity() {
    let mut o = observations(&[(NOW, 0)], 0);
    o.write_index = u32::try_from(capacity() - 1).unwrap();
    o.total_entries = u64::try_from(capacity() - 1).unwrap();
    o.entries[capacity() - 2] = ObservationEntry {
        timestamp: NOW,
        tick_cumulative: 0,
    };
    let next = record_observation(&o, 1, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(next.write_index, 0, "the ring wraps");
}

#[test]
fn write_index_zero_reads_the_last_slot_as_previous() {
    let mut o = observations(&[], 0);
    o.write_index = 0;
    o.total_entries = u64::from(OBSERVATIONS_CAPACITY);
    o.entries[capacity() - 1] = ObservationEntry {
        timestamp: NOW,
        tick_cumulative: 500,
    };
    let next = record_observation(&o, 2, NOW + min_interval(), WINDOW).unwrap();
    assert_eq!(
        next.entries[0].tick_cumulative,
        500 + 2 * i64::try_from(min_interval()).unwrap(),
        "the previous entry is the wrapped-around last slot"
    );
}

#[test]
fn append_observation_round_trips_through_the_shard() {
    let o = observations(&[(NOW, 0)], 0);
    let written = append_observation(&ShardData::from(&o), 10, NOW + min_interval(), WINDOW)
        .expect("interval elapsed");
    assert_eq!(as_observations(&written).total_entries, 2);
}

// ── PublishPrice ────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "price observations account ID does not match expected PDA")]
fn publish_rejects_a_foreign_observations_account() {
    plan_publish_price(
        &ours(AccountId::new([0xE9; 32])),
        &price_account_slot(),
        &current_tick_slot(),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "oracle price account ID does not match expected PDA")]
fn publish_rejects_a_foreign_price_account() {
    plan_publish_price(
        &observations_slot(),
        &ours(AccountId::new([0xE8; 32])),
        &current_tick_slot(),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "current tick account ID does not match expected PDA")]
fn publish_rejects_a_foreign_current_tick_account() {
    plan_publish_price(
        &observations_slot(),
        &price_account_slot(),
        &ours(AccountId::new([0xE7; 32])),
        price_source_id(),
        WINDOW,
        ORACLE_PROGRAM_ID,
    );
}

/// Two observations one second apart, a flat tick of 10 and no tail: the average
/// is exactly 10.
#[test]
fn twap_over_the_stored_window() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    assert_eq!(
        compute_twap_price(&o, 10, NOW + 1_000, NOW + 1_000),
        Some(tick_to_oracle_price(10))
    );
}

#[test]
fn twap_is_none_with_fewer_than_two_observations() {
    let o = observations(&[(NOW, 0)], 0);
    assert_eq!(compute_twap_price(&o, 0, NOW, NOW + 1_000), None);
}

/// A price that has simply not changed yields a *fresh* timestamp and the
/// unchanged value, rather than a stale window stamped with a current time.
#[test]
fn constant_price_extrapolates_to_now() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    assert_eq!(
        compute_twap_price(&o, 10, NOW + 1_000, NOW + 5_000),
        Some(tick_to_oracle_price(10)),
        "a flat tick averages to itself however long the tail"
    );
}

/// The tail is split at `last_updated`: `[t2, last_updated]` carries the tick
/// stored at t2, and only `[last_updated, now]` carries the live tick.
#[test]
fn tail_segment_is_split_at_the_current_ticks_timestamp() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    let split = compute_twap_price(&o, 20, NOW + 3_000, NOW + 4_000);
    let whole_tail = compute_twap_price(&o, 20, NOW + 1_000, NOW + 4_000);
    assert_ne!(
        split, whole_tail,
        "a late tick must not be smeared across the whole gap"
    );
}

#[test]
fn tail_extrapolation_clamps_a_large_tick_jump() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    let huge = compute_twap_price(&o, MAX_TICK_DELTA * 100, NOW + 1_000, NOW + 2_000);
    let bound = compute_twap_price(&o, 10 + MAX_TICK_DELTA, NOW + 1_000, NOW + 2_000);
    assert_eq!(huge, bound, "the live-tick segment is bounded");
}

#[test]
fn negative_twap_floors_toward_negative_infinity() {
    // cumulative_diff = -1 over 2 ms floors to -1, not 0.
    let o = observations(&[(NOW, 0), (NOW + 2, -1)], 0);
    assert_eq!(
        compute_twap_price(&o, 0, NOW + 2, NOW + 2),
        Some(tick_to_oracle_price(-1)),
        "floor division, not truncation toward zero"
    );
}

#[test]
fn zero_twap_tick_is_published_as_the_unit_price() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 0)], 0);
    assert_eq!(
        compute_twap_price(&o, 0, NOW + 1_000, NOW + 1_000),
        Some(tick_to_oracle_price(0))
    );
}

// ── PublishPrice: the pin chain ─────────────────────────────────────────────

#[test]
fn check_twap_accepts_a_truthful_computation() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    let price = compute_twap_price(&o, 10, NOW + 1_000, NOW + 1_000);
    check_twap(&ShardData::from(&o), 10, NOW + 1_000, NOW + 1_000, price);
}

/// The whole instruction rests on this: a caller who inflates the published
/// figure fails here, because the guard recomputes it from the buffer.
#[test]
#[should_panic(expected = "Declared TWAP does not match the price observations")]
fn check_twap_rejects_an_inflated_price() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    check_twap(
        &ShardData::from(&o),
        10,
        NOW + 1_000,
        NOW + 1_000,
        Some(tick_to_oracle_price(10_000)),
    );
}

/// And a caller cannot suppress a publish that is due by claiming there is
/// nothing to average.
#[test]
#[should_panic(expected = "Declared TWAP does not match the price observations")]
fn check_twap_rejects_a_false_nothing_to_publish_claim() {
    let o = observations(&[(NOW, 0), (NOW + 1_000, 10_000)], 10);
    check_twap(&ShardData::from(&o), 10, NOW + 1_000, NOW + 1_000, None);
}

/// Nor force one that is not due.
#[test]
#[should_panic(expected = "Declared TWAP does not match the price observations")]
fn check_twap_rejects_a_publish_with_too_few_observations() {
    let o = observations(&[(NOW, 0)], 0);
    check_twap(
        &ShardData::from(&o),
        0,
        NOW,
        NOW + 1_000,
        Some(tick_to_oracle_price(0)),
    );
}

#[test]
fn check_twap_accepts_a_truthful_nothing_to_publish_claim() {
    let o = observations(&[(NOW, 0)], 0);
    check_twap(&ShardData::from(&o), 0, NOW, NOW + 1_000, None);
}

#[test]
#[should_panic(expected = "price observations account must be initialized")]
fn check_twap_rejects_an_empty_observations_shard() {
    check_twap(&empty(), 0, NOW, NOW, None);
}

#[test]
fn publish_twap_writes_price_and_timestamp_and_preserves_the_rest() {
    let written = publish_twap(
        &price_account_data(UNIT_PRICE, NOW - 10_000),
        tick_to_oracle_price(10),
        NOW,
    );
    let a = as_price_account(&written);
    assert_eq!(a.price, tick_to_oracle_price(10));
    assert_eq!(a.timestamp, NOW);
    assert_eq!(a.base_asset, base_asset(), "asset pair is preserved");
    assert_eq!(a.quote_asset, quote_asset());
    assert_eq!(a.source_id, price_source_id());
    assert_eq!(a.confidence_interval, 7, "confidence interval is preserved");
}

#[test]
#[should_panic(expected = "oracle price account must be initialized")]
fn publish_twap_on_an_uninitialized_price_account_panics() {
    let _ = publish_twap(&empty(), UNIT_PRICE, NOW);
}
