//! `CreateCurrentTickAccount`: a price source creates its current-tick actor. A program source
//! sends with its current-tick seed, which admits the account (it's the program's own PDA); a
//! key source signs a root message to its own account.

use lee_core::program::{ReceiveInput, Response};
use twap_oracle_core::{
    event::{CurrentTickAccountCreated, OracleEvent as _},
    price_to_tick, CurrentTickState, OracleState, PriceSource,
};

use crate::write;

/// At the empty current-tick actor. `last_updated` starts at 0: `PublishPrice` clamps it into
/// the tail, where 0 means the tick held for the whole tail.
pub(crate) fn create_current_tick_account(input: &ReceiveInput, initial_price: u128) -> Response {
    assert!(
        input.pre_state.is_empty(),
        "current-tick actor already exists"
    );
    let source = match input.from {
        Some(from) => {
            let source = PriceSource::program(from);
            assert!(
                input.receiver.account_id == source.current_tick_account_id(),
                "receiver is not the sender's current-tick account"
            );
            source
        }
        None => {
            assert!(
                input.is_authorized,
                "a key source must sign for its own account"
            );
            PriceSource::Key(input.receiver.account_id)
        }
    };
    let tick = price_to_tick(initial_price);
    write(OracleState::CurrentTick(CurrentTickState {
        source,
        tick,
        last_updated: 0,
        pending: None,
    }))
    .event(
        CurrentTickAccountCreated {
            current_tick: input.receiver.account_id,
            source,
            tick,
        }
        .to_program_event(),
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn message(initial_price: u128) -> Message {
        Message::CreateCurrentTickAccount { initial_price }
    }

    fn created(source: PriceSource, tick: i32) -> Response {
        write(&current_tick_of(source, tick, 0, None)).event(
            CurrentTickAccountCreated {
                current_tick: source.current_tick_account_id(),
                source,
                tick,
            }
            .to_program_event(),
        )
    }

    #[test]
    fn a_program_source_creates_its_current_tick_actor() {
        let response = run(current_tick_id(), Some(pool()), false, None, &message(ONE));
        assert_eq!(response, created(source(), 0));
    }

    #[test]
    fn the_initial_price_is_converted_to_a_tick() {
        for price in [ONE << 1, ONE >> 1, ONE << 20, 1] {
            let response = run(
                current_tick_id(),
                Some(pool()),
                false,
                None,
                &message(price),
            );
            assert_eq!(response, created(source(), price_to_tick(price)));
        }
    }

    #[test]
    fn a_key_source_creates_its_current_tick_actor_on_its_own_account() {
        let response = run(KEY, None, true, None, &message(ONE));
        assert_eq!(response, created(PriceSource::Key(KEY), 0));
    }

    #[test]
    #[should_panic(expected = "a key source must sign for its own account")]
    fn an_unsigned_root_creation_panics() {
        let _ = run(KEY, None, false, None, &message(ONE));
    }

    #[test]
    #[should_panic(expected = "receiver is not the sender's current-tick account")]
    fn a_program_cannot_create_another_sources_current_tick_actor() {
        let other_pool = Actor::new(AccountId::new([9; 32]), AMM);
        let _ = run(
            current_tick_id(),
            Some(other_pool),
            false,
            None,
            &message(ONE),
        );
    }

    #[test]
    #[should_panic(expected = "receiver is not the sender's current-tick account")]
    fn another_programs_actor_on_the_pool_cannot_create_it() {
        let _ = run(
            current_tick_id(),
            Some(outsider(POOL)),
            false,
            None,
            &message(ONE),
        );
    }

    #[test]
    #[should_panic(expected = "current-tick actor already exists")]
    fn an_existing_current_tick_actor_panics() {
        let existing = current_tick(5, 0, None);
        let _ = run(
            current_tick_id(),
            Some(pool()),
            false,
            Some(&existing),
            &message(ONE),
        );
    }
}
