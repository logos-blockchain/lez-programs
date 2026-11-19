//! `CreatePriceAccounts`: the source opens a window. The current-tick actor reads the clock, then
//! creates the window's observations and price actors with the oracle's seeds, both seeded from
//! its current tick.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_oracle_price_account_pda_seed,
    compute_price_observations_pda, compute_price_observations_pda_seed,
    event::{OracleEvent as _, PriceAccountsCreated},
    tick_to_oracle_price, CurrentTickState, Internal, ObservationEntry, OraclePrice, OracleState,
    Pending, PriceObservations, OBSERVATIONS_CAPACITY,
};

use crate::{assert_source, capacity, clock::read_clock, current_tick_state, oracle_actor, write};

/// At the current-tick actor, from its source.
pub(crate) fn create_price_accounts(
    input: &ReceiveInput,
    window_duration: u64,
    base_asset: AccountId,
    quote_asset: AccountId,
) -> Response {
    let state = current_tick_state(input);
    assert_source(input, &state.source);
    assert!(
        window_duration >= u64::from(OBSERVATIONS_CAPACITY),
        "window_duration must be at least OBSERVATIONS_CAPACITY, for a sampling interval of 1 ms or more"
    );
    read_clock(
        state,
        Pending::CreatePriceAccounts {
            window_duration,
            base_asset,
            quote_asset,
        },
    )
}

/// After the clock's reply: create both actors, each admitted by its oracle seed.
pub(crate) fn finish(
    input: &ReceiveInput,
    state: CurrentTickState,
    window_duration: u64,
    base_asset: AccountId,
    quote_asset: AccountId,
    now: u64,
) -> Response {
    let oracle = input.receiver.program_account_id;
    let current_tick = input.receiver.account_id;
    let price_observations = compute_price_observations_pda(oracle, current_tick, window_duration);
    let oracle_price = compute_oracle_price_account_pda(oracle, current_tick, window_duration);
    let price_source_id = state.source.account_id();
    let tick = state.tick;

    write(OracleState::CurrentTick(state))
        .send(
            SyncMessage::new(
                oracle_actor(input, price_observations),
                &Internal::InitObservations {
                    window_duration,
                    price_source_id,
                    tick,
                    now,
                },
            )
            .with_pda_seeds([compute_price_observations_pda_seed(
                current_tick,
                window_duration,
            )]),
        )
        .send(
            SyncMessage::new(
                oracle_actor(input, oracle_price),
                &Internal::InitPrice {
                    window_duration,
                    price_source_id,
                    base_asset,
                    quote_asset,
                    price: tick_to_oracle_price(tick),
                    now,
                },
            )
            .with_pda_seeds([compute_oracle_price_account_pda_seed(
                current_tick,
                window_duration,
            )]),
        )
        .event(
            PriceAccountsCreated {
                current_tick,
                window_duration,
                price_observations,
                oracle_price,
            }
            .to_program_event(),
        )
}

/// At the empty observations actor, from its current-tick actor: entry 0 at `now`.
pub(crate) fn init_observations(
    input: &ReceiveInput,
    from: Actor,
    window_duration: u64,
    price_source_id: AccountId,
    tick: i32,
    now: u64,
) -> Response {
    assert!(
        input.pre_state.is_empty(),
        "observations actor already exists"
    );
    assert!(
        input.receiver.account_id
            == compute_price_observations_pda(
                input.receiver.program_account_id,
                from.account_id,
                window_duration,
            ),
        "receiver is not the sender's observations account"
    );
    let mut entries = vec![ObservationEntry::default(); capacity()];
    *entries.first_mut().expect("capacity is non-zero") = ObservationEntry {
        timestamp: now,
        tick_cumulative: 0,
    };
    write(OracleState::PriceObservations(PriceObservations {
        price_source_id,
        current_tick: from.account_id,
        window_duration,
        write_index: 1,
        total_entries: 1,
        last_recorded_tick: tick,
        entries,
    }))
}

/// [`Internal::InitPrice`]'s fields.
pub(crate) struct InitPrice {
    pub window_duration: u64,
    pub price_source_id: AccountId,
    pub base_asset: AccountId,
    pub quote_asset: AccountId,
    pub price: u128,
    pub now: u64,
}

/// At the empty price actor, from its current-tick actor: the first price. Zero price and zero
/// timestamp are what consumers reject as "no price", so the actor never starts in that state.
pub(crate) fn init_price(input: &ReceiveInput, from: Actor, init: InitPrice) -> Response {
    assert!(input.pre_state.is_empty(), "price actor already exists");
    assert!(
        input.receiver.account_id
            == compute_oracle_price_account_pda(
                input.receiver.program_account_id,
                from.account_id,
                init.window_duration,
            ),
        "receiver is not the sender's price account"
    );
    assert!(init.price != 0, "initial price must be non-zero");
    assert!(init.now != 0, "clock timestamp must be non-zero");
    write(OracleState::Price(OraclePrice {
        base_asset: init.base_asset,
        quote_asset: init.quote_asset,
        price: init.price,
        timestamp: init.now,
        source_id: init.price_source_id,
        confidence_interval: 0,
    }))
}

#[cfg(test)]
mod tests {
    mod create_price_accounts {
        use crate::test_utils::*;

        fn message(window_duration: u64) -> Message {
            Message::CreatePriceAccounts {
                window_duration,
                base_asset: BASE,
                quote_asset: QUOTE,
            }
        }

        fn parked(window_duration: u64) -> Option<Pending> {
            Some(Pending::CreatePriceAccounts {
                window_duration,
                base_asset: BASE,
                quote_asset: QUOTE,
            })
        }

        #[test]
        fn the_source_parks_the_creation_and_reads_the_clock() {
            let response = run(
                current_tick_id(),
                Some(pool()),
                false,
                Some(&current_tick(5, 0, None)),
                &message(WINDOW),
            );
            assert_eq!(response, reads_clock(&current_tick(5, 0, parked(WINDOW))));
        }

        #[test]
        fn a_window_equal_to_the_capacity_is_accepted() {
            let window = u64::from(OBSERVATIONS_CAPACITY);
            let response = run(
                current_tick_id(),
                Some(pool()),
                false,
                Some(&current_tick(5, 0, None)),
                &message(window),
            );
            assert_eq!(response, reads_clock(&current_tick(5, 0, parked(window))));
        }

        #[test]
        #[should_panic(expected = "window_duration must be at least OBSERVATIONS_CAPACITY")]
        fn a_window_below_the_capacity_panics() {
            let window = u64::from(OBSERVATIONS_CAPACITY) - 1;
            let pre = current_tick(5, 0, None);
            let _ = run(
                current_tick_id(),
                Some(pool()),
                false,
                Some(&pre),
                &message(window),
            );
        }

        #[test]
        #[should_panic(expected = "sender is not the price source")]
        fn another_programs_actor_on_the_pool_is_refused() {
            let pre = current_tick(5, 0, None);
            let _ = run(
                current_tick_id(),
                Some(outsider(POOL)),
                false,
                Some(&pre),
                &message(WINDOW),
            );
        }

        #[test]
        fn a_key_source_opens_a_window_at_root() {
            let key = PriceSource::Key(KEY);
            let response = run(
                KEY,
                None,
                true,
                Some(&current_tick_of(key, 5, 0, None)),
                &message(WINDOW),
            );
            assert_eq!(
                response,
                reads_clock(&current_tick_of(key, 5, 0, parked(WINDOW)))
            );
        }

        #[test]
        fn the_reply_creates_both_actors_with_their_seeds() {
            let tick = -300;
            let response = clock_reply(
                current_tick_id(),
                &current_tick(tick, 0, parked(WINDOW)),
                7_000,
            );
            let expected =
                write(&current_tick(tick, 0, None))
                    .send(
                        SyncMessage::new(
                            oracle(observations_id()),
                            &Internal::InitObservations {
                                window_duration: WINDOW,
                                price_source_id: POOL,
                                tick,
                                now: 7_000,
                            },
                        )
                        .with_pda_seeds([
                            compute_price_observations_pda_seed(current_tick_id(), WINDOW),
                        ]),
                    )
                    .send(
                        SyncMessage::new(
                            oracle(price_id()),
                            &Internal::InitPrice {
                                window_duration: WINDOW,
                                price_source_id: POOL,
                                base_asset: BASE,
                                quote_asset: QUOTE,
                                price: tick_to_oracle_price(tick),
                                now: 7_000,
                            },
                        )
                        .with_pda_seeds([
                            compute_oracle_price_account_pda_seed(current_tick_id(), WINDOW),
                        ]),
                    )
                    .event(
                        PriceAccountsCreated {
                            current_tick: current_tick_id(),
                            window_duration: WINDOW,
                            price_observations: observations_id(),
                            oracle_price: price_id(),
                        }
                        .to_program_event(),
                    );
            assert_eq!(response, expected);
        }

        #[test]
        fn a_key_sources_window_actors_name_the_key() {
            let key = PriceSource::Key(KEY);
            let pre = current_tick_of(key, 0, 0, parked(WINDOW));
            let response = clock_reply(KEY, &pre, 7_000);
            let init = response.sync_messages.first().expect("InitObservations");
            assert_eq!(
                init.to,
                oracle(compute_price_observations_pda(ORACLE, KEY, WINDOW))
            );
            let message: Internal = borsh::from_slice(&init.message).unwrap();
            assert!(matches!(
                message,
                Internal::InitObservations { price_source_id, .. } if price_source_id == KEY
            ));
        }
    }

    mod init_observations {
        use crate::test_utils::*;

        fn message(tick: i32, now: u64) -> Internal {
            Internal::InitObservations {
                window_duration: WINDOW,
                price_source_id: POOL,
                tick,
                now,
            }
        }

        #[test]
        fn entry_zero_is_at_now_and_the_tick_is_the_last_recorded() {
            for tick in [0, 4_000, -4_000] {
                let response = run(
                    observations_id(),
                    Some(oracle(current_tick_id())),
                    false,
                    None,
                    &message(tick, 7_000),
                );
                assert_eq!(response, write(&observations_at(&[(7_000, 0)], tick)));
            }
        }

        #[test]
        fn the_buffer_starts_with_one_entry_and_default_slots() {
            let response = run(
                observations_id(),
                Some(oracle(current_tick_id())),
                false,
                None,
                &message(0, 7_000),
            );
            let OracleState::PriceObservations(observations) = written(&response) else {
                panic!("not observations");
            };
            assert_eq!(observations.write_index, 1);
            assert_eq!(observations.total_entries, 1);
            assert_eq!(observations.price_source_id, POOL);
            assert_eq!(observations.current_tick, current_tick_id());
            assert_eq!(observations.window_duration, WINDOW);
            assert_eq!(
                observations.entries.len(),
                usize::try_from(OBSERVATIONS_CAPACITY).unwrap()
            );
            assert!(observations
                .entries
                .iter()
                .skip(1)
                .all(|e| *e == ObservationEntry::default()));
        }

        #[test]
        #[should_panic(expected = "receiver is not the sender's observations account")]
        fn another_current_tick_actor_cannot_create_it() {
            let _ = run(
                observations_id(),
                Some(oracle(KEY)),
                false,
                None,
                &message(0, 7_000),
            );
        }

        #[test]
        #[should_panic(expected = "observations actor already exists")]
        fn an_existing_observations_actor_panics() {
            let _ = run(
                observations_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(&observations_at(&[(1_000, 0)], 0)),
                &message(0, 7_000),
            );
        }

        #[test]
        #[should_panic(expected = "oracle message must decode")]
        fn an_outsider_cannot_send_it() {
            let _ = run(
                observations_id(),
                Some(outsider(current_tick_id())),
                false,
                None,
                &message(0, 7_000),
            );
        }
    }

    mod init_price {
        use crate::test_utils::*;

        fn message(price: u128, now: u64) -> Internal {
            Internal::InitPrice {
                window_duration: WINDOW,
                price_source_id: POOL,
                base_asset: BASE,
                quote_asset: QUOTE,
                price,
                now,
            }
        }

        #[test]
        fn the_price_names_its_assets_and_source() {
            let response = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                None,
                &message(ONE, 7_000),
            );
            assert_eq!(response, write(&price(ONE, 7_000)));
        }

        #[test]
        #[should_panic(expected = "receiver is not the sender's price account")]
        fn another_current_tick_actor_cannot_create_it() {
            let _ = run(
                price_id(),
                Some(oracle(KEY)),
                false,
                None,
                &message(ONE, 7_000),
            );
        }

        #[test]
        #[should_panic(expected = "price actor already exists")]
        fn an_existing_price_actor_panics() {
            let _ = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                Some(&price(ONE, 1_000)),
                &message(ONE, 7_000),
            );
        }

        #[test]
        #[should_panic(expected = "initial price must be non-zero")]
        fn a_zero_price_panics() {
            let _ = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                None,
                &message(0, 7_000),
            );
        }

        #[test]
        #[should_panic(expected = "clock timestamp must be non-zero")]
        fn a_zero_timestamp_panics() {
            let _ = run(
                price_id(),
                Some(oracle(current_tick_id())),
                false,
                None,
                &message(ONE, 0),
            );
        }
    }
}
