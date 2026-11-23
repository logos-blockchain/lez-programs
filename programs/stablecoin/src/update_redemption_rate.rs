//! `UpdateRedemptionRate`: the permissionless, strict poke that runs one tick of the
//! redemption-rate controller.
//!
//! Every gate is hard: it panics when called before `minimum_milliseconds_between_rate_updates`
//! has elapsed, and when the oracle's price is for another pair, future-dated, stale or zero.
//! Never blocked by the frozen flag. The engine reads the clock, then the oracle.

use lee_core::program::{ReceiveInput, Response};
use stablecoin_core::{
    bound_redemption_price,
    controller::run_controller_tick,
    math::{compute_current_redemption_price_wide, oracle_price_to_fixed_point},
    ControllerOutput, Engine, Pending, ProtocolParameters, RedemptionPriceState,
};

use crate::{clock, engine_state, oracle};

/// At the engine, from anyone: park the poke and ask the clock for the time.
pub(crate) fn update_redemption_rate(input: &ReceiveInput) -> Response {
    clock::read_clock(engine_state(input), Pending::UpdateRedemptionRate)
}

/// At the engine, on the clock's reply: check the interval, then ask the oracle.
///
/// # Panics
/// - `"update_redemption_rate called too soon since last update"` before the interval elapsed.
pub(crate) fn finish(_input: &ReceiveInput, engine: Engine, now: u64) -> Response {
    assert!(
        interval_is_due(&engine, now),
        "update_redemption_rate called too soon since last update"
    );
    oracle::read_price(engine, now, true)
}

/// Whether `minimum_milliseconds_between_rate_updates` has elapsed since the last update.
pub(crate) fn interval_is_due(engine: &Engine, now: u64) -> bool {
    now.saturating_sub(engine.redemption.last_updated_at)
        >= engine.parameters.minimum_milliseconds_between_rate_updates
}

/// Project the redemption price to `now`, run one controller tick against the oracle's market
/// price, and return the re-anchored state.
///
/// `oracle_price` is the raw `Q64.64` price; it is converted to the redemption price's
/// fixed-point scale before the tick. The projection deliberately runs BEFORE the tick and uses
/// the OLD rate; the tick then produces the NEW rate that later reads compound against.
///
/// The projection is computed in `U512` and clamped into the redemption-price bounds, so
/// re-anchoring never panics on overflow.
pub(crate) fn advance_redemption_price(
    parameters: &ProtocolParameters,
    redemption: &RedemptionPriceState,
    oracle_price: u128,
    now: u64,
) -> RedemptionPriceState {
    let milliseconds_elapsed = now.saturating_sub(redemption.last_updated_at);
    let current_price = bound_redemption_price(compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    ));

    let ControllerOutput {
        redemption_rate_per_millisecond,
        controller_integral_term,
    } = run_controller_tick(
        current_price,
        oracle_price_to_fixed_point(oracle_price),
        redemption.controller_integral_term,
        parameters.controller_proportional_gain,
        parameters.controller_integral_gain,
        milliseconds_elapsed,
    );

    RedemptionPriceState {
        redemption_price_at_last_update: current_price,
        redemption_rate_per_millisecond,
        controller_integral_term,
        last_updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    fn parked() -> StablecoinState {
        let mut engine = engine_at(1_000);
        engine.pending = Some(Pending::UpdateRedemptionRate);
        StablecoinState::Engine(engine)
    }

    fn clock_replies(now: u64) -> Response {
        run(
            engine(),
            Some(clock()),
            false,
            Some(&parked()),
            &clock_reply(now),
        )
    }

    #[test]
    fn the_poke_parks_and_asks_the_clock() {
        let mut expected = engine_at(1_000);
        expected.pending = Some(Pending::UpdateRedemptionRate);
        assert_eq!(
            run(
                engine(),
                None,
                false,
                Some(&StablecoinState::Engine(engine_at(1_000))),
                &Message::UpdateRedemptionRate
            ),
            Response::set_state(StablecoinState::Engine(expected).encode()).send_sync(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 }
            ),
        );
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn the_poke_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            None,
            false,
            None,
            &Message::UpdateRedemptionRate,
        );
    }

    #[test]
    fn update_redemption_rate_asks_the_oracle_after_the_clock() {
        let mut parked = engine_at(1_000); // minimum interval 1_000 in `parameters()`
        parked.pending = Some(Pending::UpdateRedemptionRate);
        let response = run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(parked.clone())),
            &clock_reply(3_000),
        );
        parked.pending = Some(Pending::OraclePrice {
            now: 3_000,
            hard: true,
        });
        assert_eq!(
            response,
            Response::set_state(StablecoinState::Engine(parked).encode())
                .send_sync(oracle(), &twap_oracle_core::ReadPrice),
        );
    }

    #[test]
    fn the_interval_boundary_is_inclusive() {
        let response = clock_replies(2_000);
        assert_eq!(
            saved_engine(&response).pending,
            Some(Pending::OraclePrice {
                now: 2_000,
                hard: true
            })
        );
    }

    #[test]
    #[should_panic(expected = "update_redemption_rate called too soon since last update")]
    fn update_redemption_rate_too_soon_panics() {
        let _ = clock_replies(1_500);
    }

    #[test]
    fn the_whole_poke_updates_the_redemption_price() {
        // Clock reply, then the oracle's reply at the engine the first one saved.
        let after_clock = saved_engine(&clock_replies(3_000));
        let response = run(
            engine(),
            Some(oracle()),
            false,
            Some(&StablecoinState::Engine(after_clock)),
            &price_reply(1 << 62, 2_900),
        );
        let after = saved_engine(&response);
        assert_eq!(after.redemption.last_updated_at, 3_000);
        assert_eq!(after.pending, None);
    }

    #[test]
    fn a_frozen_protocol_still_updates() {
        let mut engine = engine_at(1_000);
        engine.parameters.is_frozen = true;
        engine.pending = Some(Pending::UpdateRedemptionRate);
        let response = run(
            self::engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(engine)),
            &clock_reply(3_000),
        );
        assert!(saved_engine(&response).pending.is_some());
    }

    #[test]
    #[should_panic(expected = "the engine is already waiting for a reply")]
    fn a_second_update_while_one_is_pending_panics() {
        let _ = run(
            engine(),
            None,
            false,
            Some(&parked()),
            &Message::UpdateRedemptionRate,
        );
    }
}
