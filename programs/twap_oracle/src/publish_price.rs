//! `PublishPrice`: at root, the current-tick actor reads the clock and pushes its tick to the
//! window's observations actor, which computes the TWAP up to `now` and sends it to the window's
//! price actor.
//!
//! The average runs from the oldest stored entry to `now`. The tail past the newest entry is
//! extrapolated: up to the current tick's `last_updated` with the last recorded tick, after it
//! with the live tick clamped by [`MAX_TICK_DELTA`], so a tick written just before a publish
//! can't dominate a long tail. A tail longer than [`MAX_TAIL_INTERVALS`] sampling intervals is
//! refused: `now` becomes the price's timestamp, the only freshness signal consumers have.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_price_observations_pda,
    event::{OracleEvent as _, Published},
    tick_to_oracle_price, CurrentTickState, Internal, OracleState, Pending, PriceObservations,
    MAX_TAIL_INTERVALS, MAX_TICK_DELTA, OBSERVATIONS_CAPACITY,
};

use crate::{
    assert_root, clock::read_clock, current_tick_state, min_interval, newest_index,
    observations_actor, observations_state, oracle_actor, price_state, write,
};

/// At the current-tick actor, as the root message.
pub(crate) fn publish_price(input: &ReceiveInput, window_duration: u64) -> Response {
    assert_root(input, "PublishPrice");
    read_clock(
        current_tick_state(input),
        Pending::PublishPrice { window_duration },
    )
}

/// After the clock's reply.
pub(crate) fn finish(
    input: &ReceiveInput,
    state: CurrentTickState,
    window_duration: u64,
    now: u64,
) -> Response {
    let message = Internal::ComputePrice {
        tick: state.tick,
        last_updated: state.last_updated,
        now,
    };
    write(OracleState::CurrentTick(state))
        .send_sync(observations_actor(input, window_duration), &message)
}

/// At the observations actor, from its current-tick actor. With fewer than two entries there is
/// no average yet, and nothing is sent.
pub(crate) fn compute_price(
    input: &ReceiveInput,
    from: Actor,
    tick: i32,
    last_updated: u64,
    now: u64,
) -> Response {
    let observations = observations_state(input);
    assert!(
        from.account_id == observations.current_tick,
        "sender is not this window's current-tick actor"
    );
    let Some(twap_tick) = twap(&observations, tick, last_updated, now) else {
        return Response::keep_state();
    };
    let price_actor = oracle_actor(
        input,
        compute_oracle_price_account_pda(
            input.receiver.program_account_id,
            observations.current_tick,
            observations.window_duration,
        ),
    );
    Response::keep_state().send_sync(
        price_actor,
        &Internal::SetPrice {
            current_tick: observations.current_tick,
            window_duration: observations.window_duration,
            price: tick_to_oracle_price(twap_tick),
            now,
        },
    )
}

/// The TWAP tick over `[oldest entry, now]`, or `None` with fewer than two entries.
pub(crate) fn twap(
    observations: &PriceObservations,
    tick: i32,
    last_updated: u64,
    now: u64,
) -> Option<i32> {
    if observations.total_entries < 2 {
        return None;
    }
    // Each buffer is calibrated to its window by the sampling interval, so the oldest entry is
    // the window's start: slot 0 until the buffer fills, then the slot about to be overwritten.
    let oldest_index = if observations.total_entries >= u64::from(OBSERVATIONS_CAPACITY) {
        usize::try_from(observations.write_index).expect("write_index fits in usize")
    } else {
        0
    };
    let t1 = observations
        .entries
        .get(oldest_index)
        .expect("oldest index is within bounds");
    let t2 = observations
        .entries
        .get(newest_index(observations))
        .expect("newest index is within bounds");

    assert!(
        now >= t2.timestamp,
        "clock is behind the newest observation"
    );
    let max_tail_ms = min_interval(observations.window_duration).saturating_mul(MAX_TAIL_INTERVALS);
    assert!(
        now.saturating_sub(t2.timestamp) <= max_tail_ms,
        "observations are stale; call RecordTick before publishing"
    );

    // Split the tail at `last_updated`, clamped into [t2, now]: before it the last recorded tick
    // held, after it the live tick, clamped as RecordTick would.
    let live_tick = observations.last_recorded_tick.saturating_add(
        tick.saturating_sub(observations.last_recorded_tick)
            .clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA),
    );
    let boundary = last_updated.clamp(t2.timestamp, now);
    let before_ms = i64::try_from(boundary.saturating_sub(t2.timestamp)).expect("tail fits in i64");
    let after_ms = i64::try_from(now.saturating_sub(boundary)).expect("tail fits in i64");
    let cumulative_now = i64::from(observations.last_recorded_tick)
        .checked_mul(before_ms)
        .zip(i64::from(live_tick).checked_mul(after_ms))
        .and_then(|(before, after)| t2.tick_cumulative.checked_add(before)?.checked_add(after))
        .expect("extrapolated tick_cumulative fits in i64");

    // Stored timestamps strictly increase, so now >= t2 > t1 and the divisor is positive. Floor
    // division (toward −∞), as Uniswap's `OracleLibrary.consult`.
    let elapsed_ms = i64::try_from(now.checked_sub(t1.timestamp).expect("now >= t1"))
        .expect("elapsed_ms fits in i64");
    let twap_tick = cumulative_now
        .checked_sub(t1.tick_cumulative)
        .expect("cumulative difference fits in i64")
        .checked_div_euclid(elapsed_ms)
        .expect("elapsed_ms is non-zero");
    Some(i32::try_from(twap_tick).expect("TWAP tick fits in i32"))
}

/// At the price actor, from its observations actor. `current_tick` and `window_duration` derive
/// both actors' addresses, so the price actor holds only the price.
pub(crate) fn set_price(
    input: &ReceiveInput,
    from: Actor,
    current_tick: AccountId,
    window_duration: u64,
    price: u128,
    now: u64,
) -> Response {
    let mut state = price_state(input);
    let oracle = input.receiver.program_account_id;
    assert!(
        input.receiver.account_id
            == compute_oracle_price_account_pda(oracle, current_tick, window_duration),
        "receiver is not this window's price actor"
    );
    assert!(
        from.account_id == compute_price_observations_pda(oracle, current_tick, window_duration),
        "sender is not this window's observations actor"
    );
    assert!(
        now >= state.timestamp,
        "a price can't be older than the one it replaces"
    );
    state.price = price;
    state.timestamp = now;
    let event = Published {
        oracle_price: input.receiver.account_id,
        window_duration,
        price,
        timestamp: now,
    };
    write(OracleState::Price(state)).event(event.to_program_event())
}

#[cfg(test)]
mod tests {
    mod publish_price {
        use crate::test_utils::*;

        fn message() -> Message {
            Message::PublishPrice {
                window_duration: WINDOW,
            }
        }

        fn parked() -> Option<Pending> {
            Some(Pending::PublishPrice {
                window_duration: WINDOW,
            })
        }

        #[test]
        fn anyone_at_root_parks_the_publish_and_reads_the_clock() {
            let response = run(
                current_tick_id(),
                None,
                false,
                Some(&current_tick(5, 0, None)),
                &message(),
            );
            assert_eq!(response, reads_clock(&current_tick(5, 0, parked())));
        }

        #[test]
        #[should_panic(expected = "PublishPrice must be the transaction's root message")]
        fn a_program_cannot_publish() {
            let pre = current_tick(5, 0, None);
            let _ = run(
                current_tick_id(),
                Some(outsider(POOL)),
                false,
                Some(&pre),
                &message(),
            );
        }

        #[test]
        fn the_reply_pushes_the_tick_and_its_last_update() {
            let response = clock_reply(
                current_tick_id(),
                &current_tick(-42, 6_500, parked()),
                7_000,
            );
            assert_eq!(
                response,
                write(&current_tick(-42, 6_500, None)).send_sync(
                    oracle(observations_id()),
                    &Internal::ComputePrice {
                        tick: -42,
                        last_updated: 6_500,
                        now: 7_000
                    }
                )
            );
        }
    }

    mod compute_price {
        use crate::test_utils::*;

        fn compute_price(pre: &OracleState, tick: i32, last_updated: u64, now: u64) -> Response {
            run(
                observations_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(pre),
                &Internal::ComputePrice {
                    tick,
                    last_updated,
                    now,
                },
            )
        }

        #[test]
        fn the_twap_goes_to_the_price_actor() {
            // Tick 100 over [1000, 3000], no tail.
            let pre = observations_at(&[(1_000, 0), (3_000, 200_000)], 100);
            let response = compute_price(&pre, 100, 0, 3_000);
            assert_eq!(
                response,
                Response::keep_state().send_sync(
                    oracle(price_id()),
                    &set_price_message(tick_to_oracle_price(100), 3_000)
                )
            );
        }

        #[test]
        fn one_observation_sends_nothing() {
            let pre = observations_at(&[(1_000, 0)], 0);
            assert_eq!(compute_price(&pre, 0, 0, 1_500), Response::keep_state());
        }

        #[test]
        #[should_panic(expected = "sender is not this window's current-tick actor")]
        fn another_current_tick_actor_cannot_publish() {
            let _ = run(
                observations_id(),
                Some(oracle(KEY)),
                false,
                Some(&observations_at(&[(1_000, 0), (2_000, 0)], 0)),
                &Internal::ComputePrice {
                    tick: 0,
                    last_updated: 0,
                    now: 2_000,
                },
            );
        }
    }

    mod twap {
        use crate::{publish_price::twap, test_utils::*};

        #[test]
        fn the_stored_average_without_a_tail() {
            let observations = buffer(&[(1_000, 0), (3_000, 200_000)], 100);
            assert_eq!(twap(&observations, 100, 0, 3_000), Some(100));
        }

        #[test]
        fn negative_and_zero_averages() {
            let negative = buffer(&[(1_000, 0), (3_000, -200_000)], -100);
            assert_eq!(twap(&negative, -100, 0, 3_000), Some(-100));
            let zero = buffer(&[(1_000, 0), (3_000, 0)], 0);
            assert_eq!(twap(&zero, 0, 0, 3_000), Some(0));
        }

        #[test]
        fn a_negative_remainder_floors_toward_negative_infinity() {
            // -1001 over 1000 ms is -1.001, which floors to -2 (truncation would give -1).
            let observations = buffer(&[(1_000, 0), (2_000, -1_001)], -1);
            assert_eq!(twap(&observations, -1, 0, 2_000), Some(-2));
        }

        #[test]
        fn the_average_spans_the_oldest_to_the_newest_entry() {
            // 0 for 1000 ms, then 300 for 1000 ms: 150.
            let observations = buffer(&[(1_000, 0), (2_000, 0), (3_000, 300_000)], 300);
            assert_eq!(twap(&observations, 300, 0, 3_000), Some(150));
        }

        #[test]
        fn a_full_buffer_starts_at_the_write_index() {
            let capacity = u64::from(OBSERVATIONS_CAPACITY);
            // Slot 0 is the newest after a wrap; slot 1 the oldest.
            let entries: Vec<(u64, i64)> = (0..capacity)
                .map(|i| (1_000 * (i + 1), 10 * 1_000 * i64::try_from(i).unwrap()))
                .collect();
            let mut observations = buffer(&entries, 10);
            let newest = 1_000 * (capacity + 1);
            observations.entries[0] = ObservationEntry {
                timestamp: newest,
                tick_cumulative: 10 * 1_000 * i64::try_from(capacity).unwrap(),
            };
            observations.write_index = 1;
            observations.total_entries = capacity + 1;
            assert_eq!(twap(&observations, 10, 0, newest), Some(10));
        }

        #[test]
        fn a_constant_price_extrapolates_to_now() {
            let observations = buffer(&[(1_000, 0), (2_000, 100_000)], 100);
            assert_eq!(twap(&observations, 100, 0, 4_000), Some(100));
        }

        #[test]
        fn the_tail_carries_the_live_tick_after_its_last_update() {
            // [1000, 2000] at 0, tail [2000, 4000]: 0 until 3000, then 300 → 300 × 1000 / 3000.
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            assert_eq!(twap(&observations, 300, 3_000, 4_000), Some(100));
        }

        #[test]
        fn a_live_tick_updated_before_the_newest_entry_covers_the_whole_tail() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            assert_eq!(twap(&observations, 300, 0, 4_000), Some(200));
        }

        #[test]
        fn a_negative_tail() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            assert_eq!(twap(&observations, -300, 0, 4_000), Some(-200));
        }

        #[test]
        fn a_large_tail_jump_is_clamped() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            let expected = MAX_TICK_DELTA * 2 / 3;
            assert_eq!(twap(&observations, 500_000, 0, 4_000), Some(expected));
        }

        #[test]
        fn a_zero_tail_ignores_the_live_tick() {
            let observations = buffer(&[(1_000, 0), (2_000, 100_000)], 100);
            assert_eq!(twap(&observations, 5_000, 2_000, 2_000), Some(100));
        }

        #[test]
        fn a_tail_of_exactly_the_bound_publishes() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            let now = 2_000 + MIN_INTERVAL * MAX_TAIL_INTERVALS;
            assert_eq!(twap(&observations, 0, 0, now), Some(0));
        }

        #[test]
        #[should_panic(expected = "observations are stale; call RecordTick before publishing")]
        fn a_tail_past_the_bound_is_refused() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            let now = 2_000 + MIN_INTERVAL * MAX_TAIL_INTERVALS + 1;
            let _ = twap(&observations, 0, 0, now);
        }

        #[test]
        #[should_panic(expected = "observations are stale; call RecordTick before publishing")]
        fn a_fresh_live_tick_does_not_excuse_a_stale_feed() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            let now = 2_000 + MIN_INTERVAL * MAX_TAIL_INTERVALS + 1;
            let _ = twap(&observations, 0, now, now);
        }

        #[test]
        #[should_panic(expected = "clock is behind the newest observation")]
        fn a_clock_behind_the_newest_entry_panics() {
            let observations = buffer(&[(1_000, 0), (2_000, 0)], 0);
            let _ = twap(&observations, 0, 0, 1_999);
        }

        #[test]
        fn fewer_than_two_entries_is_no_average() {
            assert_eq!(twap(&buffer(&[(1_000, 0)], 0), 0, 0, 1_000), None);
        }
    }

    mod set_price {
        use crate::test_utils::*;

        fn set_price(pre: &OracleState, price: u128, now: u64) -> Response {
            run(
                price_id(),
                Some(oracle(observations_id())),
                false,
                Some(pre),
                &set_price_message(price, now),
            )
        }

        #[test]
        fn the_price_and_timestamp_change_and_are_announced() {
            let response = set_price(&price(ONE, 1_000), ONE << 1, 3_000);
            let expected = write(&price(ONE << 1, 3_000)).event(
                Published {
                    oracle_price: price_id(),
                    window_duration: WINDOW,
                    price: ONE << 1,
                    timestamp: 3_000,
                }
                .to_program_event(),
            );
            assert_eq!(response, expected, "assets, source and confidence kept");
        }

        #[test]
        fn the_same_timestamp_may_publish_again() {
            let response = set_price(&price(ONE, 3_000), ONE << 1, 3_000);
            assert_eq!(written(&response), price(ONE << 1, 3_000));
        }

        #[test]
        #[should_panic(expected = "a price can't be older than the one it replaces")]
        fn an_older_price_panics() {
            let _ = set_price(&price(ONE, 3_000), ONE << 1, 2_999);
        }

        #[test]
        #[should_panic(expected = "receiver is not a price actor")]
        fn a_price_at_an_empty_actor_panics() {
            let _ = run(
                price_id(),
                Some(oracle(observations_id())),
                false,
                None,
                &set_price_message(1, 3_000),
            );
        }

        #[test]
        #[should_panic(expected = "sender is not this window's observations actor")]
        fn another_oracle_actor_cannot_set_the_price() {
            let _ = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(&price(ONE, 1_000)),
                &set_price_message(1, 3_000),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not this window's price actor")]
        fn another_windows_observations_actor_cannot_set_the_price() {
            let window = 2 * WINDOW;
            let _ = run(
                price_id(),
                Some(oracle(compute_price_observations_pda(
                    ORACLE,
                    current_tick_id(),
                    window,
                ))),
                false,
                Some(&price(ONE, 1_000)),
                &Internal::SetPrice {
                    current_tick: current_tick_id(),
                    window_duration: window,
                    price: 1,
                    now: 3_000,
                },
            );
        }

        #[test]
        #[should_panic(expected = "oracle message must decode")]
        fn an_outsider_cannot_set_the_price() {
            let _ = run(
                price_id(),
                Some(outsider(observations_id())),
                false,
                Some(&price(ONE, 1_000)),
                &set_price_message(1, 3_000),
            );
        }
    }
}
