//! `RecordTick`: at root, the current-tick actor reads the clock and pushes its tick to the
//! window's observations actor, which appends it if a sampling interval has passed.
//!
//! Each observation moves the recorded tick by at most [`MAX_TICK_DELTA`], and the clamped tick
//! is what the next observation is measured from, so a manipulated price moves the average only
//! for as long as it is held.

use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response},
};
use twap_oracle_core::{
    event::{OracleEvent as _, Recorded},
    CurrentTickState, Internal, ObservationEntry, OracleState, Pending, PriceObservations,
    MAX_TICK_DELTA,
};

use crate::{
    assert_root, capacity, clock::read_clock, current_tick_state, min_interval, newest_index,
    observations_actor, observations_state, write,
};

/// At the current-tick actor, as the root message.
pub(crate) fn record_tick(input: &ReceiveInput, window_duration: u64) -> Response {
    assert_root(input, "RecordTick");
    read_clock(
        current_tick_state(input),
        Pending::RecordTick { window_duration },
    )
}

/// After the clock's reply.
pub(crate) fn finish(
    input: &ReceiveInput,
    state: CurrentTickState,
    window_duration: u64,
    now: u64,
) -> Response {
    let tick = state.tick;
    write(OracleState::CurrentTick(state)).send_sync(
        observations_actor(input, window_duration),
        &Internal::SampleTick { tick, now },
    )
}

/// At the observations actor, from its current-tick actor. Below the sampling interval nothing
/// changes; every record after the first in a block lands here.
pub(crate) fn sample_tick(input: &ReceiveInput, from: Actor, tick: i32, now: u64) -> Response {
    let mut observations = observations_state(input);
    assert!(
        from.account_id == observations.current_tick,
        "sender is not this window's current-tick actor"
    );
    let Some(recorded) = append(&mut observations, tick, now) else {
        return Response::keep_state();
    };
    let event = Recorded {
        price_observations: input.receiver.account_id,
        window_duration: observations.window_duration,
        timestamp: now,
        tick: recorded,
    };
    write(OracleState::PriceObservations(observations)).event(event.to_program_event())
}

/// Appends `(now, cumulative + clamped_tick × elapsed)` and returns the clamped tick, or `None`
/// when less than a sampling interval has passed since the newest entry.
pub(crate) fn append(observations: &mut PriceObservations, tick: i32, now: u64) -> Option<i32> {
    let newest = observations
        .entries
        .get(newest_index(observations))
        .expect("newest index is within bounds");
    let elapsed_ms = now.saturating_sub(newest.timestamp);
    if elapsed_ms < min_interval(observations.window_duration) {
        return None;
    }

    // Integrate the clamped tick itself (not the delta), so a constant tick still accumulates.
    let delta = tick.saturating_sub(observations.last_recorded_tick);
    let clamped_tick = observations
        .last_recorded_tick
        .saturating_add(delta.clamp(-MAX_TICK_DELTA, MAX_TICK_DELTA));
    let elapsed_ms = i64::try_from(elapsed_ms).expect("elapsed_ms fits in i64");
    let tick_cumulative = i64::from(clamped_tick)
        .checked_mul(elapsed_ms)
        .and_then(|product| newest.tick_cumulative.checked_add(product))
        .expect("tick_cumulative fits in i64");

    let write_index = usize::try_from(observations.write_index).expect("write_index fits in usize");
    *observations
        .entries
        .get_mut(write_index)
        .expect("write_index is within bounds") = ObservationEntry {
        timestamp: now,
        tick_cumulative,
    };
    let next_index = write_index
        .checked_add(1)
        .expect("write_index + 1 fits in usize")
        .checked_rem(capacity())
        .expect("capacity is non-zero");
    observations.write_index = u32::try_from(next_index).expect("write_index fits in u32");
    observations.total_entries = observations
        .total_entries
        .checked_add(1)
        .expect("total_entries does not overflow");
    observations.last_recorded_tick = clamped_tick;
    Some(clamped_tick)
}

#[cfg(test)]
mod tests {
    mod record_tick {
        use crate::test_utils::*;

        fn message() -> Message {
            Message::RecordTick {
                window_duration: WINDOW,
            }
        }

        fn parked() -> Option<Pending> {
            Some(Pending::RecordTick {
                window_duration: WINDOW,
            })
        }

        #[test]
        fn anyone_at_root_parks_the_record_and_reads_the_clock() {
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
        #[should_panic(expected = "RecordTick must be the transaction's root message")]
        fn a_program_cannot_record() {
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
        #[should_panic(expected = "RecordTick must be the transaction's root message")]
        fn not_even_the_source_can_record_from_a_program() {
            let pre = current_tick(5, 0, None);
            let _ = run(
                current_tick_id(),
                Some(pool()),
                false,
                Some(&pre),
                &message(),
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not a current-tick actor")]
        fn recording_at_the_observations_actor_panics() {
            let pre = observations_at(&[(1_000, 0)], 0);
            let _ = run(observations_id(), None, false, Some(&pre), &message());
        }

        #[test]
        #[should_panic(expected = "receiver is not a current-tick actor")]
        fn recording_at_an_empty_actor_panics() {
            let _ = run(current_tick_id(), None, false, None, &message());
        }

        #[test]
        fn the_reply_pushes_the_tick_to_the_observations_actor() {
            let response = clock_reply(current_tick_id(), &current_tick(-42, 0, parked()), 7_000);
            assert_eq!(
                response,
                write(&current_tick(-42, 0, None)).send_sync(
                    oracle(observations_id()),
                    &Internal::SampleTick {
                        tick: -42,
                        now: 7_000
                    }
                )
            );
        }
    }

    mod sample_tick {
        use crate::test_utils::*;

        fn sample_tick(pre: &OracleState, tick: i32, now: u64) -> Response {
            run(
                observations_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(pre),
                &Internal::SampleTick { tick, now },
            )
        }

        #[test]
        fn an_entry_is_appended_and_announced() {
            let response = sample_tick(&observations_at(&[(1_000, 0)], 0), 100, 3_000);
            let expected = write(&observations_at(&[(1_000, 0), (3_000, 200_000)], 100)).event(
                Recorded {
                    price_observations: observations_id(),
                    window_duration: WINDOW,
                    timestamp: 3_000,
                    tick: 100,
                }
                .to_program_event(),
            );
            assert_eq!(response, expected);
        }

        #[test]
        fn below_the_sampling_interval_nothing_changes() {
            let pre = observations_at(&[(1_000, 0)], 0);
            assert_eq!(sample_tick(&pre, 100, 1_999), Response::keep_state());
        }

        #[test]
        #[should_panic(expected = "sender is not this window's current-tick actor")]
        fn another_current_tick_actor_cannot_inject_a_tick() {
            let _ = run(
                observations_id(),
                Some(oracle(KEY)),
                false,
                Some(&observations_at(&[(1_000, 0)], 0)),
                &Internal::SampleTick {
                    tick: 100,
                    now: 3_000,
                },
            );
        }

        #[test]
        #[should_panic(expected = "oracle message must decode")]
        fn an_outsider_cannot_send_an_observation() {
            let _ = run(
                observations_id(),
                Some(outsider(current_tick_id())),
                false,
                Some(&observations_at(&[(1_000, 0)], 0)),
                &Internal::SampleTick {
                    tick: 100,
                    now: 3_000,
                },
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an observations actor")]
        fn an_observation_at_a_price_actor_panics() {
            let _ = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(&price(ONE, 1_000)),
                &Internal::SampleTick {
                    tick: 100,
                    now: 3_000,
                },
            );
        }

        #[test]
        #[should_panic(expected = "receiver is not an observations actor")]
        fn an_observation_at_an_empty_actor_panics() {
            let _ = run(
                observations_id(),
                Some(oracle(current_tick_id())),
                false,
                None,
                &Internal::SampleTick {
                    tick: 100,
                    now: 3_000,
                },
            );
        }
    }

    mod append {
        use crate::{record_tick::append, test_utils::*};

        fn newest(observations: &PriceObservations) -> ObservationEntry {
            observations.entries[crate::newest_index(observations)].clone()
        }

        #[test]
        fn the_cumulative_advances_by_tick_times_elapsed() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            assert_eq!(append(&mut observations, 100, 3_000), Some(100));
            assert_eq!(
                newest(&observations),
                ObservationEntry {
                    timestamp: 3_000,
                    tick_cumulative: 200_000
                }
            );
        }

        #[test]
        fn the_cumulative_advances_from_a_non_zero_base() {
            let mut observations = buffer(&[(1_000, 0), (2_000, 50_000)], 50);
            append(&mut observations, 60, 4_000);
            assert_eq!(newest(&observations).tick_cumulative, 50_000 + 60 * 2_000);
        }

        #[test]
        fn a_negative_tick_decrements_the_cumulative() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            append(&mut observations, -100, 2_000);
            assert_eq!(newest(&observations).tick_cumulative, -100_000);
        }

        #[test]
        fn a_constant_non_zero_tick_still_accumulates() {
            let mut observations = buffer(&[(1_000, 0)], 50);
            append(&mut observations, 50, 2_000);
            append(&mut observations, 50, 3_000);
            assert_eq!(newest(&observations).tick_cumulative, 100_000);
        }

        #[test]
        fn the_write_index_and_total_advance() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            append(&mut observations, 0, 2_000);
            assert_eq!(observations.write_index, 2);
            assert_eq!(observations.total_entries, 2);
        }

        #[test]
        fn the_write_index_wraps_at_capacity_and_reads_the_last_slot() {
            let capacity = usize::try_from(OBSERVATIONS_CAPACITY).unwrap();
            let entries: Vec<(u64, i64)> = (0..capacity)
                .map(|i| (1_000 * (u64::try_from(i).unwrap() + 1), 0))
                .collect();
            let mut observations = buffer(&entries, 0);
            assert_eq!(
                observations.write_index, 0,
                "a full buffer writes slot 0 next"
            );
            let last = entries.last().unwrap().0;
            assert_eq!(append(&mut observations, 7, last + 1_000), Some(7));
            assert_eq!(observations.write_index, 1);
            assert_eq!(
                observations.entries[0],
                ObservationEntry {
                    timestamp: last + 1_000,
                    tick_cumulative: 7_000
                },
                "measured from the newest entry in the last slot"
            );
        }

        #[test]
        fn exactly_one_interval_is_enough() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            assert_eq!(append(&mut observations, 1, 1_000 + MIN_INTERVAL), Some(1));
        }

        #[test]
        fn below_the_interval_nothing_is_written() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            let before = observations.clone();
            assert_eq!(
                append(&mut observations, 5_000, 1_000 + MIN_INTERVAL - 1),
                None
            );
            assert_eq!(observations, before, "last_recorded_tick unchanged too");
        }

        #[test]
        fn the_interval_is_floored_to_one_ms() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            observations.window_duration = 1;
            assert_eq!(append(&mut observations, 1, 1_000), None, "same timestamp");
            assert_eq!(append(&mut observations, 1, 1_001), Some(1));
        }

        #[test]
        fn a_small_delta_passes_through() {
            let mut observations = buffer(&[(1_000, 0)], 100);
            assert_eq!(append(&mut observations, 150, 2_000), Some(150));
        }

        #[test]
        fn a_delta_of_exactly_max_tick_delta_passes_through() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            assert_eq!(
                append(&mut observations, MAX_TICK_DELTA, 2_000),
                Some(MAX_TICK_DELTA)
            );
        }

        #[test]
        fn large_deltas_are_clamped_both_ways() {
            let mut up = buffer(&[(1_000, 0)], 100);
            assert_eq!(append(&mut up, 100_000, 2_000), Some(100 + MAX_TICK_DELTA));
            assert_eq!(up.last_recorded_tick, 100 + MAX_TICK_DELTA);
            assert_eq!(
                newest(&up).tick_cumulative,
                i64::from(100 + MAX_TICK_DELTA) * 1_000,
                "integrates the baseline plus the clamped delta"
            );

            let mut down = buffer(&[(1_000, 0)], -100);
            assert_eq!(
                append(&mut down, -100_000, 2_000),
                Some(-100 - MAX_TICK_DELTA)
            );
        }

        #[test]
        fn manipulate_then_revert_is_bounded_by_max_tick_delta() {
            let mut observations = buffer(&[(1_000, 0)], 0);
            assert_eq!(
                append(&mut observations, 500_000, 2_000),
                Some(MAX_TICK_DELTA)
            );
            assert_eq!(append(&mut observations, 0, 3_000), Some(0));
            assert_eq!(
                newest(&observations).tick_cumulative,
                i64::from(MAX_TICK_DELTA) * 1_000,
                "one interval at MAX_TICK_DELTA, then back to 0"
            );
        }
    }
}
