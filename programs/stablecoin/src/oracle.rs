//! The engine reads the market price from the oracle's reply.
//!
//! The engine parks [`Pending::OraclePrice`] (the clock time is already known), asks the oracle
//! with `ReadPrice`, and applies the price when the reply arrives. `UpdateRedemptionRate` is
//! strict (`hard`: an unusable price is a panic); `RefreshGlobals` is best-effort (an unusable
//! price leaves the redemption state untouched).

use lee_core::{
    account::Actor,
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    event::{RedemptionRateUpdated, StablecoinEvent as _},
    Engine, Internal, Pending, ProtocolParameters,
};
use twap_oracle_core::{OraclePrice, PriceReply, ReadPrice};

use crate::{
    checks, clock, engine_actor, engine_state, save_engine,
    update_redemption_rate::advance_redemption_price,
};

/// Parks the price read in `engine` and returns the messages to send, in this order: `ReadPrice`
/// to the oracle, then [`Internal::AssertOracleReplied`] to the engine itself. LEZ delivers
/// depth-first, so the oracle's reply (which clears `pending`) runs before the assertion; an
/// oracle that swallows `ReadPrice` fails the whole transaction instead of leaving the engine
/// waiting forever. Callers attach other messages (the surplus mint) before these.
///
/// # Panics
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn request_price(
    input: &ReceiveInput,
    engine: &mut Engine,
    now: u64,
    hard: bool,
) -> [SyncMessage; 2] {
    clock::park(engine, Pending::OraclePrice { now, hard });
    [
        SyncMessage::new(
            Actor::new(
                engine.parameters.market_price_oracle_id,
                engine.parameters.market_price_oracle_program_id,
            ),
            &ReadPrice,
        ),
        SyncMessage::new(engine_actor(input), &Internal::AssertOracleReplied),
    ]
}

/// Parks `Pending::OraclePrice { now, hard }` and asks the oracle for its price, followed by
/// the engine's own [`Internal::AssertOracleReplied`] self-check (see [`request_price`]): an
/// oracle that does not reply fails the transaction.
///
/// # Panics
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn read_price(
    input: &ReceiveInput,
    mut engine: Engine,
    now: u64,
    hard: bool,
) -> Response {
    let requests = request_price(input, &mut engine, now, hard);
    requests
        .into_iter()
        .fold(save_engine(engine), Response::send)
}

/// At the engine, from the engine itself: the oracle must have replied by now.
///
/// # Panics
/// - `"only the engine may send this"` when the sender is anyone else.
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"the market price oracle did not reply"` when the price read is still parked.
pub(crate) fn assert_replied(input: &ReceiveInput) -> Response {
    checks::assert_from_engine(input);
    assert!(
        engine_state(input).pending.is_none(),
        "the market price oracle did not reply"
    );
    Response::keep_state()
}

/// At the engine, from the configured oracle: apply the price it replied with.
///
/// # Panics
/// - `"the engine is not waiting for the oracle"` when no price read is parked.
/// - `"market price oracle reply must decode"` when the bytes are not a `PriceReply`.
/// - For a `hard` read, in this order: `"Market price oracle does not quote the stablecoin in
///   collateral"`, `"Market price oracle observation is dated in the future"`, `"Market price
///   oracle observation is stale"`, `"Market price oracle reports zero price"`.
pub(crate) fn on_reply(input: &ReceiveInput) -> Response {
    let mut engine = engine_state(input);
    let Some(Pending::OraclePrice { now, hard }) = engine.pending.take() else {
        panic!("the engine is not waiting for the oracle");
    };
    let reply: PriceReply =
        borsh::from_slice(&input.message).expect("market price oracle reply must decode");
    match refusal(&engine.parameters, &reply.price, now) {
        Some(reason) => assert!(!hard, "{reason}"),
        None => {
            engine.redemption = advance_redemption_price(
                &engine.parameters,
                &engine.redemption,
                reply.price.price,
                now,
            );
            let event = RedemptionRateUpdated {
                redemption_price: engine.redemption.redemption_price_at_last_update,
                redemption_rate_per_millisecond: engine.redemption.redemption_rate_per_millisecond,
                now,
            }
            .to_program_event();
            return save_engine(engine).event(event);
        }
    }
    save_engine(engine)
}

/// Why `price` cannot drive the controller at `now`, if it cannot. Checked in order.
fn refusal(parameters: &ProtocolParameters, price: &OraclePrice, now: u64) -> Option<&'static str> {
    if price.base_asset != parameters.stablecoin_definition_id
        || price.quote_asset != parameters.collateral_definition_id
    {
        return Some("Market price oracle does not quote the stablecoin in collateral");
    }
    let Some(age) = now.checked_sub(price.timestamp) else {
        return Some("Market price oracle observation is dated in the future");
    };
    if age > parameters.maximum_oracle_price_age_milliseconds {
        return Some("Market price oracle observation is stale");
    }
    if price.price == 0 {
        return Some("Market price oracle reports zero price");
    }
    None
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        bound_redemption_price, event::StablecoinEvent as _,
        math::compute_current_redemption_price_wide,
    };

    use super::*;
    use crate::test_utils::*;

    const NOW: u64 = 100_000_000;
    /// `1 << 62` is 0.25 in Q64.64, below the fixture's redemption price of 1.0.
    const PRICE: u128 = 1 << 62;

    fn waiting(hard: bool) -> StablecoinState {
        let mut engine = engine_at(1_000);
        engine.parameters.controller_proportional_gain = FIXED_POINT_ONE.cast_signed();
        engine.pending = Some(Pending::OraclePrice { now: NOW, hard });
        StablecoinState::Engine(engine)
    }

    fn reply(hard: bool, price: &PriceReply) -> Response {
        run(engine(), Some(oracle()), false, Some(&waiting(hard)), price)
    }

    #[test]
    fn a_usable_price_re_anchors_the_redemption_price_and_emits_the_event() {
        for hard in [true, false] {
            let response = reply(hard, &price_reply(PRICE, NOW));
            let after = saved_engine(&response);
            let StablecoinState::Engine(before) = waiting(hard) else {
                unreachable!("`waiting` builds an engine");
            };
            let expected =
                advance_redemption_price(&before.parameters, &before.redemption, PRICE, NOW);
            assert_eq!(after.redemption, expected);
            assert!(after.redemption.redemption_rate_per_millisecond > FIXED_POINT_ONE);
            assert_eq!(after.pending, None);
            assert!(response.sync_messages.is_empty());
            assert_eq!(
                response.events,
                vec![RedemptionRateUpdated {
                    redemption_price: expected.redemption_price_at_last_update,
                    redemption_rate_per_millisecond: expected.redemption_rate_per_millisecond,
                    now: NOW,
                }
                .to_program_event()],
            );
        }
    }

    #[test]
    #[should_panic(expected = "the engine is not waiting for the oracle")]
    fn a_price_reply_nobody_asked_for_panics() {
        let _ = run(
            engine(),
            Some(oracle()),
            false,
            Some(&StablecoinState::Engine(engine_at(1_000))),
            &price_reply(PRICE, NOW),
        );
    }

    #[test]
    #[should_panic(expected = "the engine is not waiting for the oracle")]
    fn a_price_reply_while_waiting_for_the_clock_panics() {
        let mut engine = engine_at(1_000);
        engine.pending = Some(Pending::AccrueStabilityFee);
        let _ = run(
            self::engine(),
            Some(oracle()),
            false,
            Some(&StablecoinState::Engine(engine)),
            &price_reply(PRICE, NOW),
        );
    }

    #[test]
    #[should_panic(expected = "stablecoin message must decode")]
    fn a_price_reply_from_another_actor_is_not_treated_as_a_reply() {
        let _ = run(
            engine(),
            Some(Actor::new(ORACLE_PRICE, OTHER_PROGRAM)),
            false,
            Some(&waiting(true)),
            &price_reply(PRICE, NOW),
        );
    }

    #[test]
    fn a_price_from_the_oracle_the_engine_was_reconfigured_to_is_accepted() {
        let mut engine = engine_at(1_000);
        engine.parameters.market_price_oracle_id = OWNER;
        engine.parameters.market_price_oracle_program_id = OTHER_PROGRAM;
        engine.pending = Some(Pending::OraclePrice {
            now: NOW,
            hard: true,
        });
        let response = run(
            self::engine(),
            Some(Actor::new(OWNER, OTHER_PROGRAM)),
            false,
            Some(&StablecoinState::Engine(engine)),
            &price_reply(PRICE, NOW),
        );
        assert_eq!(saved_engine(&response).pending, None);
    }

    fn unusable_prices() -> Vec<(&'static str, PriceReply)> {
        let mut other_pair = price_reply(PRICE, NOW);
        other_pair.price.quote_asset = OWNER;
        let mut other_base = price_reply(PRICE, NOW);
        other_base.price.base_asset = OWNER;
        vec![
            (
                "Market price oracle does not quote the stablecoin in collateral",
                other_pair,
            ),
            (
                "Market price oracle does not quote the stablecoin in collateral",
                other_base,
            ),
            (
                "Market price oracle observation is dated in the future",
                price_reply(PRICE, NOW + 1),
            ),
            (
                "Market price oracle observation is dated in the future",
                price_reply(PRICE, u64::MAX),
            ),
            (
                "Market price oracle observation is stale",
                price_reply(PRICE, NOW - 10_001),
            ),
            (
                "Market price oracle reports zero price",
                price_reply(0, NOW),
            ),
        ]
    }

    #[test]
    fn the_hard_read_refuses_each_unusable_price_with_its_own_message() {
        for (message, price) in unusable_prices() {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reply(true, &price)));
            let payload = outcome.expect_err("an unusable price must panic");
            let text = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
                .expect("the panic carries a message");
            assert_eq!(text, message);
        }
    }

    #[test]
    fn the_soft_read_skips_each_unusable_price_and_only_clears_pending() {
        for (_, price) in unusable_prices() {
            let response = reply(false, &price);
            let after = saved_engine(&response);
            assert_eq!(after.redemption, engine_at(1_000).redemption);
            assert_eq!(after.pending, None);
            assert!(response.events.is_empty());
            assert!(response.sync_messages.is_empty());
        }
    }

    #[test]
    fn the_checks_run_in_order() {
        // Wrong pair beats future-dated beats stale beats zero.
        let mut price = price_reply(0, u64::MAX);
        price.price.quote_asset = OWNER;
        assert_eq!(
            refusal(&parameters(), &price.price, NOW),
            Some("Market price oracle does not quote the stablecoin in collateral"),
        );
        let price = price_reply(0, u64::MAX);
        assert_eq!(
            refusal(&parameters(), &price.price, NOW),
            Some("Market price oracle observation is dated in the future"),
        );
        let price = price_reply(0, 0);
        assert_eq!(
            refusal(&parameters(), &price.price, NOW),
            Some("Market price oracle observation is stale"),
        );
    }

    #[test]
    fn the_age_boundary_is_inclusive() {
        let at_the_limit = price_reply(PRICE, NOW - 10_000);
        assert_eq!(refusal(&parameters(), &at_the_limit.price, NOW), None);
        let response = reply(true, &at_the_limit);
        assert_eq!(saved_engine(&response).redemption.last_updated_at, NOW);

        let one_older = price_reply(PRICE, NOW - 10_001);
        assert_eq!(
            refusal(&parameters(), &one_older.price, NOW),
            Some("Market price oracle observation is stale"),
        );
        assert_eq!(
            refusal(&parameters(), &price_reply(PRICE, NOW).price, NOW),
            None
        );
    }

    #[test]
    #[should_panic(expected = "the engine is already waiting for a reply")]
    fn reading_the_price_while_another_operation_is_parked_panics() {
        let mut engine = engine_at(1_000);
        engine.pending = Some(Pending::AccrueStabilityFee);
        let input = ReceiveInput {
            receiver: self::engine(),
            from: None,
            is_authorized: false,
            pre_state: lee_core::account::ActorState::default(),
            message: Vec::new(),
        };
        let _ = read_price(&input, engine, NOW, true);
    }

    // --- the post-condition self-message ---

    fn assert_replied_at(from: Option<Actor>, pending: Option<Pending>) -> Response {
        let mut engine = engine_at(1_000);
        engine.pending = pending;
        run(
            self::engine(),
            from,
            false,
            Some(&StablecoinState::Engine(engine)),
            &Internal::AssertOracleReplied,
        )
    }

    #[test]
    fn the_engine_asks_the_oracle_then_asserts_the_reply_to_itself() {
        let input = ReceiveInput {
            receiver: engine(),
            from: None,
            is_authorized: false,
            pre_state: lee_core::account::ActorState::default(),
            message: Vec::new(),
        };
        let response = read_price(&input, engine_at(1_000), NOW, true);
        assert_eq!(
            response.sync_messages,
            vec![
                SyncMessage::new(oracle(), &twap_oracle_core::ReadPrice),
                SyncMessage::new(engine(), &Internal::AssertOracleReplied),
            ],
        );
    }

    #[test]
    fn assert_replied_passes_once_the_reply_cleared_pending() {
        assert_eq!(
            assert_replied_at(Some(engine()), None),
            Response::keep_state()
        );
    }

    #[test]
    #[should_panic(expected = "the market price oracle did not reply")]
    fn assert_replied_fails_while_the_price_read_is_parked() {
        let _ = assert_replied_at(
            Some(engine()),
            Some(Pending::OraclePrice {
                now: NOW,
                hard: true,
            }),
        );
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn assert_replied_from_another_stablecoin_actor_panics() {
        let _ = assert_replied_at(Some(stablecoin(OWNER)), None);
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn assert_replied_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(engine()),
            false,
            None,
            &Internal::AssertOracleReplied,
        );
    }

    // The controller's numeric behaviour, through `advance_redemption_price`.

    fn advance(
        price: u128,
        redemption: RedemptionPriceState,
        gains: (i128, i128),
    ) -> RedemptionPriceState {
        let mut engine = engine_at(1_000);
        engine.parameters.controller_proportional_gain = gains.0;
        engine.parameters.controller_integral_gain = gains.1;
        advance_redemption_price(&engine.parameters, &redemption, price, NOW)
    }

    fn redemption(rate: u128, last_updated_at: u64) -> RedemptionPriceState {
        RedemptionPriceState {
            redemption_price_at_last_update: FIXED_POINT_ONE,
            redemption_rate_per_millisecond: rate,
            controller_integral_term: 0,
            last_updated_at,
        }
    }

    #[test]
    fn a_market_below_the_redemption_price_drives_the_rate_above_one() {
        let after = advance(
            1 << 62,
            redemption(FIXED_POINT_ONE, 1_000),
            (FIXED_POINT_ONE.cast_signed(), 0),
        );
        assert!(after.redemption_rate_per_millisecond > FIXED_POINT_ONE);
    }

    #[test]
    fn a_market_above_the_redemption_price_drives_the_rate_below_one() {
        let after = advance(
            1 << 65,
            redemption(FIXED_POINT_ONE, 1_000),
            (FIXED_POINT_ONE.cast_signed(), 0),
        );
        assert!(after.redemption_rate_per_millisecond < FIXED_POINT_ONE);
    }

    #[test]
    fn a_market_at_the_redemption_price_leaves_the_rate_at_one() {
        let after = advance(
            1 << 64,
            redemption(FIXED_POINT_ONE, 1_000),
            (FIXED_POINT_ONE.cast_signed(), 1),
        );
        assert_eq!(after.redemption_rate_per_millisecond, FIXED_POINT_ONE);
        assert_eq!(after.controller_integral_term, 0);
        assert_eq!(after.last_updated_at, NOW);
    }

    #[test]
    fn the_projection_uses_the_old_rate() {
        let old_rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000_000;
        let after = advance(
            1 << 62,
            redemption(old_rate, 1_000),
            (FIXED_POINT_ONE.cast_signed(), 0),
        );
        assert_eq!(
            after.redemption_price_at_last_update,
            bound_redemption_price(compute_current_redemption_price_wide(
                FIXED_POINT_ONE,
                old_rate,
                1_000,
                NOW
            ))
        );
    }

    #[test]
    fn a_projection_that_overflows_re_anchors_at_the_ceiling() {
        let rate = FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000;
        let after = advance(
            1 << 62,
            redemption(rate, NOW - 86_400_000),
            (FIXED_POINT_ONE.cast_signed(), 0),
        );
        assert_eq!(
            after.redemption_price_at_last_update,
            stablecoin_core::MAXIMUM_REDEMPTION_PRICE
        );
        assert!(after.redemption_rate_per_millisecond <= FIXED_POINT_ONE);
    }

    #[test]
    fn a_projection_that_decays_to_zero_re_anchors_at_the_floor() {
        let rate = FIXED_POINT_ONE - FIXED_POINT_ONE / 100_000;
        let after = advance(
            1 << 62,
            redemption(rate, NOW - 86_400_000),
            (FIXED_POINT_ONE.cast_signed(), 0),
        );
        assert_eq!(
            after.redemption_price_at_last_update,
            stablecoin_core::MINIMUM_REDEMPTION_PRICE
        );
        assert!(after.redemption_rate_per_millisecond >= FIXED_POINT_ONE);
    }

    #[test]
    fn the_standing_integral_term_is_carried_when_the_gain_is_zero() {
        let mut state = redemption(FIXED_POINT_ONE, 1_000);
        state.controller_integral_term = 12_345_678;
        let after = advance(1 << 62, state, (0, 0));
        assert_eq!(after.controller_integral_term, 12_345_678);
    }
}
