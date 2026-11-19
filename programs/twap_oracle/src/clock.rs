//! The current-tick actor reads the clock: it parks the operation, asks the clock, and finishes
//! the operation when the reply arrives. The reply carries no context, so the parked
//! operation is what says which one asked.

use clock_core::Reply;
use lee_core::program::{ReceiveInput, Response};
use twap_oracle_core::{CurrentTickState, OracleState, Pending};

use crate::{
    create_price_accounts, current_tick_state, publish_price, record_tick, update_current_tick,
};

/// Each read returns the previous block's timestamp in milliseconds, unrounded.
const GRANULARITY: u64 = 1;

/// Parks `operation` in the current-tick actor's state and asks the clock for the time.
pub(crate) fn read_clock(mut state: CurrentTickState, operation: Pending) -> Response {
    assert!(
        state.pending.is_none(),
        "an operation is already waiting for the clock"
    );
    state.pending = Some(operation);
    Response::set_state(OracleState::CurrentTick(state).encode()).send_sync(
        clock_core::clock_actor(),
        &clock_core::Message::ReadTimestamp {
            granularity: GRANULARITY,
        },
    )
}

/// At the current-tick actor, from the clock: finish the parked operation at `now`.
pub(crate) fn on_reply(input: &ReceiveInput) -> Response {
    let Reply::Timestamp(now) = Reply::from_message(&input.message) else {
        panic!("the oracle reads only the clock's timestamp");
    };
    let mut state = current_tick_state(input);
    let operation = state
        .pending
        .take()
        .expect("no operation is waiting for the clock");
    match operation {
        Pending::UpdateCurrentTick { price } => update_current_tick::finish(state, price, now),
        Pending::CreatePriceAccounts {
            window_duration,
            base_asset,
            quote_asset,
        } => create_price_accounts::finish(
            input,
            state,
            window_duration,
            base_asset,
            quote_asset,
            now,
        ),
        Pending::RecordTick { window_duration } => {
            record_tick::finish(input, state, window_duration, now)
        }
        Pending::PublishPrice { window_duration } => {
            publish_price::finish(input, state, window_duration, now)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    #[should_panic(expected = "an operation is already waiting for the clock")]
    fn an_operation_while_one_is_pending_panics() {
        let pending = Some(Pending::RecordTick {
            window_duration: WINDOW,
        });
        let _ = run(
            current_tick_id(),
            None,
            false,
            Some(&current_tick(5, 0, pending)),
            &Message::PublishPrice {
                window_duration: WINDOW,
            },
        );
    }

    #[test]
    #[should_panic(expected = "no operation is waiting for the clock")]
    fn a_reply_with_nothing_pending_panics() {
        let _ = clock_reply(current_tick_id(), &current_tick(5, 0, None), 1_000);
    }

    #[test]
    #[should_panic(expected = "the oracle reads only the clock's timestamp")]
    fn a_block_id_reply_panics() {
        let pending = Some(Pending::UpdateCurrentTick { price: ONE });
        let _ = run(
            current_tick_id(),
            Some(clock()),
            false,
            Some(&current_tick(5, 0, pending)),
            &Reply::BlockId(7),
        );
    }

    #[test]
    #[should_panic(expected = "oracle message must decode")]
    fn a_reply_forged_by_another_actor_is_not_a_reply() {
        let pending = Some(Pending::UpdateCurrentTick { price: ONE });
        let _ = run(
            current_tick_id(),
            Some(outsider(POOL)),
            false,
            Some(&current_tick(5, 0, pending)),
            &Reply::Timestamp(1_000),
        );
    }

    #[test]
    #[should_panic(expected = "receiver is not a current-tick actor")]
    fn a_reply_at_another_actor_panics() {
        let _ = clock_reply(observations_id(), &observations_at(&[(1_000, 0)], 0), 2_000);
    }
}
