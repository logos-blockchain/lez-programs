//! The engine reads the clock: it parks the operation, asks the clock, and finishes the
//! operation when the reply arrives. The reply carries no context, so the parked operation is
//! what says which one asked.

use clock_core::Reply;
use lee_core::program::{ReceiveInput, Response};
use stablecoin_core::{Engine, Pending};

use crate::{accrue_stability_fee, engine_state, save_engine};

/// Each read returns the previous block's timestamp in milliseconds, unrounded.
const GRANULARITY: u64 = 1;

/// Parks `operation` in the engine's state and asks the clock for the time.
///
/// # Panics
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn read_clock(mut engine: Engine, operation: Pending) -> Response {
    assert!(
        engine.pending.is_none(),
        "the engine is already waiting for a reply"
    );
    engine.pending = Some(operation);
    save_engine(engine).send_sync(
        clock_core::clock_actor(),
        &clock_core::Message::ReadTimestamp {
            granularity: GRANULARITY,
        },
    )
}

/// At the engine, from the clock: finish the parked operation at `now`.
///
/// # Panics
/// - `"the stablecoin reads only the clock's timestamp"` for any other reply.
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"the engine is not waiting for the clock"` when nothing, or the oracle's price, is parked.
pub(crate) fn on_reply(input: &ReceiveInput) -> Response {
    let Reply::Timestamp(now) = Reply::from_message(&input.message) else {
        panic!("the stablecoin reads only the clock's timestamp");
    };
    let mut engine = engine_state(input);
    let operation = engine
        .pending
        .take()
        .expect("the engine is not waiting for the clock");
    match operation {
        Pending::AccrueStabilityFee => accrue_stability_fee::finish(input, engine, now),
        Pending::Initialize => panic!("not ported yet"),
        Pending::UpdateRedemptionRate => panic!("not ported yet"),
        Pending::RefreshGlobals { consult_oracle: _ } => panic!("not ported yet"),
        Pending::SetStabilityFee { new_rate: _ } => panic!("not ported yet"),
        Pending::ResetController => panic!("not ported yet"),
        Pending::GenerateDebt {
            position: _,
            amount: _,
            to: _,
            collateral_amount: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Pending::RepayDebt {
            position: _,
            maximum_amount: _,
            from_holding: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Pending::Withdraw {
            position: _,
            amount: _,
            to: _,
            collateral_after: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Pending::OraclePrice { now: _, hard: _ } => {
            panic!("the engine is not waiting for the clock")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    fn parked(operation: Pending) -> StablecoinState {
        let mut engine = engine_at(1_000);
        engine.pending = Some(operation);
        StablecoinState::Engine(engine)
    }

    #[test]
    #[should_panic(expected = "the engine is already waiting for a reply")]
    fn a_second_poke_while_one_is_pending_panics() {
        let _ = run(
            engine(),
            None,
            false,
            Some(&parked(Pending::AccrueStabilityFee)),
            &Message::AccrueStabilityFee,
        );
    }

    #[test]
    #[should_panic(expected = "the engine is not waiting for the clock")]
    fn a_clock_reply_nobody_asked_for_panics() {
        let _ = run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(engine_at(1_000))),
            &clock_reply(5_000),
        );
    }

    #[test]
    #[should_panic(expected = "the engine is not waiting for the clock")]
    fn a_clock_reply_while_waiting_for_the_oracle_panics() {
        let _ = run(
            engine(),
            Some(clock()),
            false,
            Some(&parked(Pending::OraclePrice { now: 1, hard: true })),
            &clock_reply(5_000),
        );
    }

    #[test]
    #[should_panic(expected = "the stablecoin reads only the clock's timestamp")]
    fn a_block_id_reply_panics() {
        let _ = run(
            engine(),
            Some(clock()),
            false,
            Some(&parked(Pending::AccrueStabilityFee)),
            &Reply::BlockId(7),
        );
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn a_clock_reply_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(clock()),
            false,
            None,
            &clock_reply(5_000),
        );
    }
}
