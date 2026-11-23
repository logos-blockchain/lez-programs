//! `AccrueStabilityFee`: the permissionless poke that advances the fee accumulator and mints the
//! accrued fees to the surplus holding.
//!
//! Also the shared "accrue" step of every operation that reads the clock and touches debt:
//! [`accrue`] applies it to the engine and hands back the messages to attach to the response.

use lee_core::{
    account::Actor,
    program::{ProgramEvent, ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_stablecoin_definition_pda_seed, compute_surplus_holding_pda,
    event::{StabilityFeeAccrued, StablecoinEvent as _},
    math::{compute_current_accumulated_rate, mul_div, FIXED_POINT_ONE},
    Engine, Pending, ProtocolParameters, StabilityFeeAccumulator,
};

use crate::{clock, engine_state, program_id, save_engine};

/// At the engine, from anyone: park the poke and ask the clock for the time.
pub(crate) fn accrue_stability_fee(input: &ReceiveInput) -> Response {
    clock::read_clock(engine_state(input), Pending::AccrueStabilityFee)
}

/// At the engine, on the clock's reply: accrue and save.
pub(crate) fn finish(input: &ReceiveInput, mut engine: Engine, now: u64) -> Response {
    let accrued = accrue(input, &mut engine, now);
    accrued.attach(save_engine(engine))
}

/// What [`accrue`] produced besides the engine's new fee accumulator.
pub(crate) struct Accrued {
    /// Mints the accrued fees to the surplus holding; `None` when nothing accrued.
    pub(crate) surplus_mint: Option<SyncMessage>,
    /// The engine's `StabilityFeeAccrued` event.
    pub(crate) event: ProgramEvent,
}

impl Accrued {
    /// Appends the surplus mint (if any) and the event to `response`.
    ///
    /// Call it before the response gets any other message so the mint is delivered first.
    pub(crate) fn attach(self, mut response: Response) -> Response {
        if let Some(mint) = self.surplus_mint {
            response = response.send(mint);
        }
        response.event(self.event)
    }
}

/// Advances `engine.fee` to `now` and returns the surplus mint and event to attach to the
/// response. The caller saves the engine.
pub(crate) fn accrue(input: &ReceiveInput, engine: &mut Engine, now: u64) -> Accrued {
    let accrual = accrue_fees(&engine.parameters, &engine.fee, now);
    engine.fee = accrual.accumulator;
    let parameters = &engine.parameters;
    let surplus_mint = (accrual.accrued_fees > 0).then(|| {
        SyncMessage::new(
            Actor::new(
                parameters.stablecoin_definition_id,
                parameters.token_program_id,
            ),
            &token_core::Message::Mint {
                definition: parameters.stablecoin_definition_id,
                to: compute_surplus_holding_pda(program_id(input)),
                amount: accrual.accrued_fees,
            },
        )
        .with_pda_seeds([compute_stablecoin_definition_pda_seed()])
    });
    Accrued {
        surplus_mint,
        event: StabilityFeeAccrued {
            accumulated_rate: engine.fee.accumulated_rate_at_last_accrual,
            accrued_fees: accrual.accrued_fees,
            now,
        }
        .to_program_event(),
    }
}

/// The accumulator advanced to `now`, and the fees that accrued on the
/// outstanding debt since the last accrual.
pub(crate) struct FeeAccrual {
    /// The accumulator to persist. `total_normalized_debt` is carried over.
    pub(crate) accumulator: StabilityFeeAccumulator,
    /// `total_normalized_debt × Δaccumulator / FIXED_POINT_ONE`, rounded down:
    /// the stablecoin to mint to the surplus holding.
    pub(crate) accrued_fees: u128,
}

/// Project the accumulator to `now` and compute the fees accrued on the
/// outstanding debt — RAI's `SAFEEngine.updateAccumulatedRate`.
///
/// Shared by every operation that accrues, so they can never disagree about how a gap
/// accrues or how much surplus it mints.
pub(crate) fn accrue_fees(
    params: &ProtocolParameters,
    accumulator: &StabilityFeeAccumulator,
    now: u64,
) -> FeeAccrual {
    let current = compute_current_accumulated_rate(
        accumulator.accumulated_rate_at_last_accrual,
        params.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    );
    // The fee is bounded below by 1.0 (§8), so the accumulator never decreases.
    let rate_delta = current
        .checked_sub(accumulator.accumulated_rate_at_last_accrual)
        .expect("Stability fee accumulator must not decrease");
    let accrued_fees = mul_div(
        accumulator.total_normalized_debt,
        rate_delta,
        FIXED_POINT_ONE,
    );

    FeeAccrual {
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: current,
            last_accrued_at: now,
            ..accumulator.clone()
        },
        accrued_fees,
    }
}

#[cfg(test)]
mod tests {
    use stablecoin_core::event::StablecoinEvent as _;

    use super::*;
    use crate::test_utils::*;

    #[test]
    fn the_poke_parks_and_asks_the_clock() {
        let before = engine_at(1_000);
        let mut parked = before.clone();
        parked.pending = Some(Pending::AccrueStabilityFee);
        assert_eq!(
            run(
                engine(),
                None,
                false,
                Some(&StablecoinState::Engine(before)),
                &Message::AccrueStabilityFee
            ),
            Response::set_state(StablecoinState::Engine(parked).encode()).send_sync(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 }
            ),
        );
    }

    fn reply_to_poke(parked: Engine, now: u64) -> (Response, Engine) {
        let response = run(
            engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(parked)),
            &clock_reply(now),
        );
        let state = StablecoinState::decode(&response.post_state.clone().expect("state is set"))
            .expect("state decodes");
        let StablecoinState::Engine(after) = state else {
            panic!("the engine stays an engine");
        };
        (response, after)
    }

    #[test]
    fn the_clock_reply_with_no_debt_only_restamps() {
        let mut parked = engine_at(1_000);
        parked.pending = Some(Pending::AccrueStabilityFee);
        let (response, after) = reply_to_poke(parked, 5_000);
        assert_eq!(after.fee.last_accrued_at, 5_000);
        assert_eq!(after.pending, None);
        assert!(response.sync_messages.is_empty());
    }

    #[test]
    fn accrued_fees_are_minted_to_the_surplus_holding() {
        // fee 2.0 per ms over 1 ms on 100 normalized debt at rate 1.0 -> accumulator 2.0, fees 100
        let mut parked = engine_at(1_000);
        parked.parameters.stability_fee_per_millisecond = 2 * FIXED_POINT_ONE;
        parked.fee.total_normalized_debt = 100;
        parked.pending = Some(Pending::AccrueStabilityFee);
        let (response, after) = reply_to_poke(parked, 1_001);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                token(definition_id()),
                &token_core::Message::Mint {
                    definition: definition_id(),
                    to: surplus_id(),
                    amount: 100
                }
            )
            .with_pda_seeds([compute_stablecoin_definition_pda_seed()])],
        );
        assert_eq!(
            response.events,
            vec![StabilityFeeAccrued {
                accumulated_rate: after.fee.accumulated_rate_at_last_accrual,
                accrued_fees: 100,
                now: 1_001,
            }
            .to_program_event()],
        );
    }

    // The pure fee-math tests below are carried over from v0.2.5 unchanged.

    fn accumulator(rate: u128, last_accrued_at: u64) -> StabilityFeeAccumulator {
        StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: rate,
            last_accrued_at,
            total_normalized_debt: 0,
        }
    }

    #[test]
    fn zero_elapsed_time_is_a_noop_restamp() {
        let mut parameters = parameters();
        parameters.stability_fee_per_millisecond = 2 * FIXED_POINT_ONE;
        let accrual = accrue_fees(&parameters, &accumulator(FIXED_POINT_ONE, 5_000), 5_000);
        assert_eq!(accrual.accumulator, accumulator(FIXED_POINT_ONE, 5_000));
        assert_eq!(accrual.accrued_fees, 0);
    }

    #[test]
    fn a_fee_of_one_leaves_the_anchor_untouched() {
        let accrual = accrue_fees(&parameters(), &accumulator(FIXED_POINT_ONE, 1_000), 9_000);
        assert_eq!(
            accrual.accumulator.accumulated_rate_at_last_accrual,
            FIXED_POINT_ONE
        );
        assert_eq!(accrual.accumulator.last_accrued_at, 9_000);
        assert_eq!(accrual.accrued_fees, 0);
    }

    #[test]
    fn the_accumulator_advances_by_the_projection() {
        let mut parameters = parameters();
        parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000;
        let before = accumulator(FIXED_POINT_ONE, 1_000);
        let accrual = accrue_fees(&parameters, &before, 4_000);
        assert_eq!(
            accrual.accumulator.accumulated_rate_at_last_accrual,
            compute_current_accumulated_rate(
                FIXED_POINT_ONE,
                parameters.stability_fee_per_millisecond,
                1_000,
                4_000
            )
        );
        assert!(accrual.accumulator.accumulated_rate_at_last_accrual > FIXED_POINT_ONE);
    }

    #[test]
    fn the_fees_are_the_debt_times_the_rate_growth() {
        let mut parameters = parameters();
        parameters.stability_fee_per_millisecond = FIXED_POINT_ONE + FIXED_POINT_ONE / 1_000;
        let mut before = accumulator(FIXED_POINT_ONE, 1_000);
        before.total_normalized_debt = 1_000_000_000_000_000_000;
        let accrual = accrue_fees(&parameters, &before, 4_000);
        let growth = accrual.accumulator.accumulated_rate_at_last_accrual - FIXED_POINT_ONE;
        assert_eq!(
            accrual.accrued_fees,
            mul_div(before.total_normalized_debt, growth, FIXED_POINT_ONE)
        );
        assert!(accrual.accrued_fees > 0);
        assert_eq!(
            accrual.accumulator.total_normalized_debt,
            before.total_normalized_debt
        );
    }
}
