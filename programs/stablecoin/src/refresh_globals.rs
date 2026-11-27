//! `RefreshGlobals`: the best-effort combined poke.
//!
//! Always advances the stability-fee accumulator. Advances the redemption price only when asked
//! to consult the oracle, its interval is due, and the oracle's price is usable; otherwise it
//! skips that half instead of panicking. `consult_oracle: false` is how a keeper pokes before
//! the oracle exists. Never blocked by the frozen flag.

use lee_core::program::{ReceiveInput, Response};
use stablecoin_core::{Engine, Pending};

use crate::{
    accrue_stability_fee::accrue, clock, engine_state, oracle, save_engine,
    update_redemption_rate::interval_is_due,
};

/// At the engine, from anyone: park the poke and ask the clock for the time.
pub(crate) fn refresh_globals(input: &ReceiveInput, consult_oracle: bool) -> Response {
    clock::read_clock(
        engine_state(input),
        Pending::RefreshGlobals { consult_oracle },
    )
}

/// At the engine, on the clock's reply: accrue, then ask the oracle if it was asked for and the
/// interval is due. The surplus mint stays the first message.
pub(crate) fn finish(
    input: &ReceiveInput,
    mut engine: Engine,
    consult_oracle: bool,
    now: u64,
) -> Response {
    let accrued = accrue(input, &mut engine, now);
    if consult_oracle && interval_is_due(&engine, now) {
        let requests = oracle::request_price(input, &mut engine, now, false);
        requests
            .into_iter()
            .fold(accrued.attach(save_engine(engine)), Response::send)
    } else {
        accrued.attach(save_engine(engine))
    }
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        compute_stablecoin_definition_pda_seed,
        event::{StabilityFeeAccrued, StablecoinEvent as _},
    };

    use super::*;
    use crate::{accrue_stability_fee::accrue_fees, test_utils::*};

    fn parked(consult_oracle: bool) -> Engine {
        let mut engine = engine_at(1_000);
        engine.pending = Some(Pending::RefreshGlobals { consult_oracle });
        engine
    }

    fn clock_replies(parked: Engine, now: u64) -> Response {
        run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(parked)),
            &clock_reply(now),
        )
    }

    #[test]
    fn the_poke_parks_with_the_flag_and_asks_the_clock() {
        for consult_oracle in [true, false] {
            let mut expected = engine_at(1_000);
            expected.pending = Some(Pending::RefreshGlobals { consult_oracle });
            assert_eq!(
                run(
                    engine(),
                    None,
                    false,
                    Some(&StablecoinState::Engine(engine_at(1_000))),
                    &Message::RefreshGlobals { consult_oracle }
                ),
                Response::set_state(StablecoinState::Engine(expected).encode()).send_sync(
                    clock(),
                    &clock_core::Message::ReadTimestamp { granularity: 1 }
                ),
            );
        }
    }

    #[test]
    fn refresh_without_the_oracle_only_accrues() {
        let response = clock_replies(parked(false), 5_000);
        let after = saved_engine(&response);
        let mut expected = engine_at(1_000);
        expected.fee.last_accrued_at = 5_000;
        assert_eq!(after, expected);
        assert_eq!(after.pending, None);
        assert!(response.sync_messages.is_empty());
        assert_eq!(
            response.events,
            vec![StabilityFeeAccrued {
                accumulated_rate: FIXED_POINT_ONE,
                accrued_fees: 0,
                now: 5_000,
            }
            .to_program_event()],
        );
    }

    #[test]
    fn refresh_before_the_interval_is_due_does_not_ask_the_oracle() {
        let response = clock_replies(parked(true), 1_500);
        let after = saved_engine(&response);
        assert_eq!(after.pending, None);
        assert_eq!(after.fee.last_accrued_at, 1_500);
        assert_eq!(after.redemption, engine_at(1_000).redemption);
        assert!(response.sync_messages.is_empty());
    }

    #[test]
    fn refresh_asks_the_oracle_when_it_is_due() {
        let response = clock_replies(parked(true), 3_000);
        let after = saved_engine(&response);
        assert_eq!(
            after.pending,
            Some(Pending::OraclePrice {
                now: 3_000,
                hard: false
            })
        );
        assert_eq!(after.fee.last_accrued_at, 3_000);
        assert_eq!(
            response.sync_messages,
            vec![
                SyncMessage::new(oracle(), &twap_oracle_core::ReadPrice),
                SyncMessage::new(engine(), &Internal::AssertOracleReplied),
            ],
        );
    }

    #[test]
    fn the_surplus_mint_comes_before_the_price_read() {
        let mut debtor = parked(true);
        debtor.parameters.stability_fee_per_millisecond =
            FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000_000;
        debtor.fee.total_normalized_debt = 1_000_000_000_000;
        let accrued_fees = accrue_fees(&debtor.parameters, &debtor.fee, 3_000).accrued_fees;
        assert!(accrued_fees > 0);
        let response = clock_replies(debtor, 3_000);
        let mint = SyncMessage::new(
            token(definition_id()),
            &token_core::Message::Mint {
                definition: definition_id(),
                to: surplus_id(),
                amount: accrued_fees,
            },
        )
        .with_pda_seeds([compute_stablecoin_definition_pda_seed()]);
        assert_eq!(
            response.sync_messages,
            vec![
                mint,
                SyncMessage::new(oracle(), &twap_oracle_core::ReadPrice),
                SyncMessage::new(engine(), &Internal::AssertOracleReplied),
            ],
        );
    }

    fn price_wait(hard: bool) -> StablecoinState {
        let mut waiting = engine_at(1_000);
        waiting.pending = Some(Pending::OraclePrice { now: 50_000, hard });
        StablecoinState::Engine(waiting)
    }

    #[test]
    fn a_stale_price_is_skipped_by_refresh() {
        let response = run(
            engine(),
            Some(oracle()),
            false,
            Some(&price_wait(false)),
            &price_reply(1 << 62, 50_000 - 10_001),
        );
        let after = saved_engine(&response);
        assert_eq!(after.redemption, engine_at(1_000).redemption);
        assert_eq!(after.pending, None);
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is stale")]
    fn the_same_stale_price_is_refused_by_update() {
        let _ = run(
            engine(),
            Some(oracle()),
            false,
            Some(&price_wait(true)),
            &price_reply(1 << 62, 50_000 - 10_001),
        );
    }

    #[test]
    fn a_future_dated_price_is_skipped_by_refresh() {
        for timestamp in [50_001, u64::MAX] {
            let response = run(
                engine(),
                Some(oracle()),
                false,
                Some(&price_wait(false)),
                &price_reply(1 << 62, timestamp),
            );
            assert_eq!(
                saved_engine(&response).redemption,
                engine_at(1_000).redemption
            );
        }
    }

    #[test]
    #[should_panic(expected = "Market price oracle observation is dated in the future")]
    fn a_future_dated_price_is_refused_by_update() {
        let _ = run(
            engine(),
            Some(oracle()),
            false,
            Some(&price_wait(true)),
            &price_reply(1 << 62, u64::MAX),
        );
    }

    #[test]
    fn a_frozen_protocol_still_refreshes() {
        let mut frozen = parked(true);
        frozen.parameters.is_frozen = true;
        let response = clock_replies(frozen, 3_000);
        assert_eq!(saved_engine(&response).fee.last_accrued_at, 3_000);
    }
}
