//! `UpdateCurrentTick`: the source reports its spot price; the current-tick actor stamps the new
//! tick with the clock, which bounds the live tick's share of `PublishPrice`'s tail.

use lee_core::program::{ReceiveInput, Response};
use twap_oracle_core::{price_to_tick, CurrentTickState, OracleState, Pending};

use crate::{assert_source, clock::read_clock, current_tick_state, write};

/// At the current-tick actor, from its source.
pub(crate) fn update_current_tick(input: &ReceiveInput, price: u128) -> Response {
    let state = current_tick_state(input);
    assert_source(input, &state.source);
    read_clock(state, Pending::UpdateCurrentTick { price })
}

/// After the clock's reply.
pub(crate) fn finish(mut state: CurrentTickState, price: u128, now: u64) -> Response {
    assert!(
        now >= state.last_updated,
        "clock is behind the current tick's last update"
    );
    state.tick = price_to_tick(price);
    state.last_updated = now;
    write(OracleState::CurrentTick(state))
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn message(price: u128) -> Message {
        Message::UpdateCurrentTick { price }
    }

    fn parked(price: u128) -> Option<Pending> {
        Some(Pending::UpdateCurrentTick { price })
    }

    #[test]
    fn the_source_parks_the_update_and_reads_the_clock() {
        let response = run(
            current_tick_id(),
            Some(pool()),
            false,
            Some(&current_tick(5, 1_000, None)),
            &message(ONE),
        );
        assert_eq!(response, reads_clock(&current_tick(5, 1_000, parked(ONE))));
    }

    #[test]
    fn the_reply_sets_the_tick_and_stamps_it() {
        let response = clock_reply(
            current_tick_id(),
            &current_tick(5, 1_000, parked(ONE)),
            7_000,
        );
        assert_eq!(response, write(&current_tick(0, 7_000, None)));
    }

    #[test]
    fn prices_convert_via_price_to_tick() {
        for price in [ONE << 1, ONE >> 1, ONE << 30, 1] {
            let response =
                clock_reply(current_tick_id(), &current_tick(0, 0, parked(price)), 7_000);
            assert_eq!(
                response,
                write(&current_tick(price_to_tick(price), 7_000, None))
            );
        }
    }

    #[test]
    fn the_same_block_may_update_again() {
        let response = clock_reply(
            current_tick_id(),
            &current_tick(5, 7_000, parked(ONE)),
            7_000,
        );
        assert_eq!(response, write(&current_tick(0, 7_000, None)));
    }

    #[test]
    #[should_panic(expected = "clock is behind the current tick's last update")]
    fn a_clock_behind_the_last_update_panics() {
        let _ = clock_reply(
            current_tick_id(),
            &current_tick(5, 7_000, parked(ONE)),
            6_999,
        );
    }

    #[test]
    fn a_key_source_updates_at_root() {
        let pre = current_tick_of(PriceSource::Key(KEY), 5, 0, None);
        let response = run(KEY, None, true, Some(&pre), &message(ONE));
        assert_eq!(
            response,
            reads_clock(&current_tick_of(PriceSource::Key(KEY), 5, 0, parked(ONE)))
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
            &message(ONE),
        );
    }

    #[test]
    #[should_panic(expected = "sender is not the price source")]
    fn another_pool_is_refused() {
        let pre = current_tick(5, 0, None);
        let other_pool = Actor::new(AccountId::new([9; 32]), AMM);
        let _ = run(
            current_tick_id(),
            Some(other_pool),
            false,
            Some(&pre),
            &message(ONE),
        );
    }

    #[test]
    #[should_panic(expected = "sender is not the price source")]
    fn a_program_source_at_root_is_refused() {
        let pre = current_tick(5, 0, None);
        let _ = run(current_tick_id(), None, true, Some(&pre), &message(ONE));
    }

    #[test]
    #[should_panic(expected = "the price source must sign the transaction's root message")]
    fn an_unsigned_key_update_is_refused() {
        let pre = current_tick_of(PriceSource::Key(KEY), 5, 0, None);
        let _ = run(KEY, None, false, Some(&pre), &message(ONE));
    }

    #[test]
    #[should_panic(expected = "the price source must sign the transaction's root message")]
    fn a_key_signature_reaching_a_nested_call_is_refused() {
        let pre = current_tick_of(PriceSource::Key(KEY), 5, 0, None);
        let _ = run(KEY, Some(outsider(READER)), true, Some(&pre), &message(ONE));
    }

    #[test]
    #[should_panic(expected = "receiver is not a current-tick actor")]
    fn an_empty_actor_panics() {
        let _ = run(current_tick_id(), Some(pool()), false, None, &message(ONE));
    }
}
