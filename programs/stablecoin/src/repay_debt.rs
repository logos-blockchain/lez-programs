//! `RepayDebt`: the owner burns stablecoin to reduce the position's debt. The position asks the
//! engine, which accrues fees at the clock's time, caps the repayment at the debt, enforces the
//! debt floor, burns the repayment and tells the position how much normalized debt to remove.
//! Repaying is allowed while the protocol is frozen.
//!
//! The engine cannot see what the caller-named holding holds: a `Burn` sent straight to it would
//! burn whatever token it holds, so a holding of any other token under the same token program
//! would erase stablecoin debt. The repayment is therefore first transferred from that holding
//! into the surplus holding, whose credit the token refuses for any token but the stablecoin
//! ("recipient holds another token"), and then burned from the surplus holding under its seed.
//! Messages run depth first, so the transfer's credit lands before the burn.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_surplus_holding_pda, compute_surplus_holding_pda_seed,
    debt_limits::meets_debt_floor,
    event::{DebtRepaid, StablecoinEvent as _},
    math::compute_repayment,
    Engine, Internal, Pending,
};

use crate::{
    accrue_stability_fee::accrue, checks, clock, engine_state, position_state, program_id,
    save_engine, save_position, stablecoin_actor,
};

/// At the owner's actor, signed as a root message: ask the engine to burn up to `maximum_amount`
/// from `stablecoin_holding` against the position's debt. The position is not changed in this run.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
/// - `"this account has no position"` when the receiver holds no position.
pub(crate) fn repay_debt(
    input: &ReceiveInput,
    stablecoin_holding: AccountId,
    maximum_amount: u128,
) -> Response {
    checks::assert_owner(input);
    let position = position_state(input);
    Response::keep_state().send_sync(
        crate::engine_actor(input),
        &Internal::RepayDebtRequest {
            maximum_amount,
            from_holding: stablecoin_holding,
            normalized_debt_amount: position.normalized_debt_amount,
        },
    )
}

/// At the engine, from the position's actor: park the request and ask the clock for the time.
/// There is no freeze check: repaying only improves the protocol's position. Only the program's
/// own code, after the owner check, sends `RepayDebtRequest` from the position's state.
///
/// # Panics
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn request(
    input: &ReceiveInput,
    from: Actor,
    maximum_amount: u128,
    from_holding: AccountId,
    normalized_debt_amount: u128,
) -> Response {
    clock::read_clock(
        engine_state(input),
        Pending::RepayDebt {
            position: from.account_id,
            maximum_amount,
            from_holding,
            normalized_debt_amount,
        },
    )
}

/// At the engine, on the clock's reply: accrue to `now`, cap the repayment at the debt at the
/// fresh accumulator, check the floor, then move the repayment from `from_holding` into the
/// surplus holding and burn it there (even a zero amount), and tell the position its debt delta.
/// The surplus mint (if any) goes first. The ceiling is never checked.
///
/// The transfer (authorized by `from_holding`'s signature, no seed) is what ties the repayment
/// to the stablecoin: the token refuses to credit the surplus holding with another token. The
/// burn runs at the surplus holding under the surplus seed.
///
/// # Panics
/// - `"Repayment decrement is capped at outstanding normalized debt"`: defensive here, since
///   `compute_repayment` already caps the decrement at the debt. It is reachable only in
///   [`repaid`], from a forged or mismatched reply.
/// - `"Total normalized debt underflow"` when the decrement exceeds the total debt.
/// - `"Position debt is below minimum_debt_amount"` when a non-zero decrement leaves debt under the
///   floor.
pub(crate) fn finish(
    input: &ReceiveInput,
    mut engine: Engine,
    position: AccountId,
    maximum_amount: u128,
    from_holding: AccountId,
    normalized_debt_amount: u128,
    now: u64,
) -> Response {
    let accrued = accrue(input, &mut engine, now);
    let current_accumulator = engine.fee.accumulated_rate_at_last_accrual;
    let repayment = compute_repayment(maximum_amount, normalized_debt_amount, current_accumulator);
    let new_debt = normalized_debt_amount
        .checked_sub(repayment.normalized_debt_decrement)
        .expect("Repayment decrement is capped at outstanding normalized debt");
    engine.fee.total_normalized_debt = engine
        .fee
        .total_normalized_debt
        .checked_sub(repayment.normalized_debt_decrement)
        .expect("Total normalized debt underflow");
    if repayment.normalized_debt_decrement != 0 {
        assert!(
            meets_debt_floor(
                new_debt,
                current_accumulator,
                engine.parameters.minimum_debt_amount
            ),
            "Position debt is below minimum_debt_amount"
        );
    }
    let token_program_id = engine.parameters.token_program_id;
    let surplus_holding = compute_surplus_holding_pda(program_id(input));
    accrued
        .attach(save_engine(engine))
        .send_sync(
            Actor::new(from_holding, token_program_id),
            &token_core::Message::Transfer {
                to: surplus_holding,
                amount: repayment.amount_to_burn,
            },
        )
        .send(
            SyncMessage::new(
                Actor::new(surplus_holding, token_program_id),
                &token_core::Message::Burn {
                    amount: repayment.amount_to_burn,
                },
            )
            .with_pda_seeds([compute_surplus_holding_pda_seed()]),
        )
        .send_sync(
            stablecoin_actor(input, position),
            &Internal::DebtRepaid {
                normalized_debt_delta: repayment.normalized_debt_decrement,
            },
        )
        .event(
            DebtRepaid {
                owner: position,
                amount_burned: repayment.amount_to_burn,
                normalized_debt_delta: repayment.normalized_debt_decrement,
            }
            .to_program_event(),
        )
}

/// At the owner's actor, from the engine: remove the normalized debt the engine repaid.
///
/// # Panics
/// - `"only the engine may send this"` when the sender is anyone else.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Repayment decrement is capped at outstanding normalized debt"` when it exceeds the debt.
pub(crate) fn repaid(input: &ReceiveInput, normalized_debt_delta: u128) -> Response {
    checks::assert_from_engine(input);
    let mut position = position_state(input);
    position.normalized_debt_amount = position
        .normalized_debt_amount
        .checked_sub(normalized_debt_delta)
        .expect("Repayment decrement is capped at outstanding normalized debt");
    save_position(position)
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        event::{DebtRepaid, StabilityFeeAccrued, StablecoinEvent as _},
        Internal,
    };

    use super::*;
    use crate::test_utils::*;

    const NOW: u64 = 1_000_000_000;

    fn position(normalized_debt_amount: u128) -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_amount: 5_000,
            normalized_debt_amount,
        })
    }

    fn repay(from: Option<Actor>, is_authorized: bool, pre: Option<&StablecoinState>) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::RepayDebt {
                stablecoin_holding: HOLDING,
                maximum_amount: 400,
            },
        )
    }

    fn request_message(frozen: bool) -> Response {
        let mut engine = engine_at(NOW);
        engine.parameters.is_frozen = frozen;
        run(
            self::engine(),
            Some(stablecoin(OWNER)),
            false,
            Some(&StablecoinState::Engine(engine)),
            &Internal::RepayDebtRequest {
                maximum_amount: 400,
                from_holding: HOLDING,
                normalized_debt_amount: 1_000,
            },
        )
    }

    /// A parked engine at accumulator `rate`, with `total_normalized_debt` equal to the debt.
    fn parked(maximum_amount: u128, normalized_debt_amount: u128, rate: u128) -> Engine {
        let mut engine = engine_at(NOW);
        engine.fee.accumulated_rate_at_last_accrual = rate;
        engine.fee.total_normalized_debt = normalized_debt_amount;
        engine.pending = Some(Pending::RepayDebt {
            position: OWNER,
            maximum_amount,
            from_holding: HOLDING,
            normalized_debt_amount,
        });
        engine
    }

    fn finish(engine: &Engine) -> Response {
        run(
            self::engine(),
            Some(clock()),
            false,
            Some(&StablecoinState::Engine(engine.clone())),
            &clock_reply(NOW),
        )
    }

    /// The repayment's two token messages: the transfer from the caller's holding into the
    /// surplus holding, then the burn there under the surplus seed.
    fn pay_in(amount: u128) -> SyncMessage {
        SyncMessage::new(
            token(HOLDING),
            &token_core::Message::Transfer {
                to: surplus_id(),
                amount,
            },
        )
    }

    fn burn(amount: u128) -> SyncMessage {
        SyncMessage::new(token(surplus_id()), &token_core::Message::Burn { amount })
            .with_pda_seeds([stablecoin_core::compute_surplus_holding_pda_seed()])
    }

    fn repaid_message(delta: u128) -> SyncMessage {
        SyncMessage::new(
            stablecoin(OWNER),
            &Internal::DebtRepaid {
                normalized_debt_delta: delta,
            },
        )
    }

    fn accrued_event(rate: u128, fees: u128) -> lee_core::program::ProgramEvent {
        StabilityFeeAccrued {
            accumulated_rate: rate,
            accrued_fees: fees,
            now: NOW,
        }
        .to_program_event()
    }

    fn repaid_event(burned: u128, delta: u128) -> lee_core::program::ProgramEvent {
        DebtRepaid {
            owner: OWNER,
            amount_burned: burned,
            normalized_debt_delta: delta,
        }
        .to_program_event()
    }

    // --- position, root message ---

    #[test]
    fn position_sends_the_request_and_keeps_its_state() {
        let response = repay(None, true, Some(&position(1_000)));
        assert_eq!(response.post_state, None);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                engine(),
                &Internal::RepayDebtRequest {
                    maximum_amount: 400,
                    from_holding: HOLDING,
                    normalized_debt_amount: 1_000,
                },
            )]
        );
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_an_unsigned_request() {
        let _ = repay(None, false, Some(&position(1_000)));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_a_nested_request() {
        let _ = repay(Some(outsider(OWNER)), true, Some(&position(1_000)));
    }

    #[test]
    #[should_panic(expected = "this account has no position")]
    fn position_refuses_a_request_without_a_position() {
        let _ = repay(None, true, None);
    }

    // --- engine, request ---

    #[test]
    fn engine_parks_the_request_and_asks_the_clock() {
        let response = request_message(false);
        let mut expected = engine_at(NOW);
        expected.pending = Some(Pending::RepayDebt {
            position: OWNER,
            maximum_amount: 400,
            from_holding: HOLDING,
            normalized_debt_amount: 1_000,
        });
        assert_eq!(saved_engine(&response), expected);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 },
            )]
        );
    }

    #[test]
    fn engine_parks_the_request_while_frozen() {
        let response = request_message(true);
        let mut expected = engine_at(NOW);
        expected.parameters.is_frozen = true;
        expected.pending = Some(Pending::RepayDebt {
            position: OWNER,
            maximum_amount: 400,
            from_holding: HOLDING,
            normalized_debt_amount: 1_000,
        });
        assert_eq!(saved_engine(&response), expected);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                clock(),
                &clock_core::Message::ReadTimestamp { granularity: 1 },
            )]
        );
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn request_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(stablecoin(OWNER)),
            false,
            Some(&position(1_000)),
            &Internal::RepayDebtRequest {
                maximum_amount: 1,
                from_holding: HOLDING,
                normalized_debt_amount: 1,
            },
        );
    }

    // --- engine, clock reply ---

    #[test]
    fn engine_pays_the_repayment_into_the_surplus_holding_burns_it_and_tells_the_position() {
        let before = parked(400, 1_000, FIXED_POINT_ONE);
        let response = finish(&before);
        let mut expected = before.clone();
        expected.pending = None;
        expected.fee.total_normalized_debt = 600;
        assert_eq!(saved_engine(&response), expected);
        assert_eq!(
            response.sync_messages,
            vec![pay_in(400), burn(400), repaid_message(400)]
        );
        assert_eq!(
            response.events,
            vec![accrued_event(FIXED_POINT_ONE, 0), repaid_event(400, 400)]
        );
    }

    #[test]
    fn engine_mints_the_surplus_before_the_burn_when_fees_accrued() {
        let mut before = parked(400, 1_000, FIXED_POINT_ONE);
        before.parameters.stability_fee_per_millisecond = 2 * FIXED_POINT_ONE;
        before.fee.last_accrued_at = NOW - 1;
        let response = finish(&before);
        // Accumulator 2.0, so 400 burned removes 200 of normalized debt; 1_000 of fees accrued.
        let surplus_mint = SyncMessage::new(
            token(definition_id()),
            &token_core::Message::Mint {
                definition: definition_id(),
                to: surplus_id(),
                amount: 1_000,
            },
        )
        .with_pda_seeds([stablecoin_core::compute_stablecoin_definition_pda_seed()]);
        assert_eq!(
            response.sync_messages,
            vec![surplus_mint, pay_in(400), burn(400), repaid_message(200)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 800);
    }

    #[test]
    fn engine_caps_a_repayment_at_the_debt() {
        // Debt 100 at 1.5 costs 150 to clear.
        let response = finish(&parked(151, 100, FIXED_POINT_ONE * 3 / 2));
        assert_eq!(
            response.sync_messages,
            vec![pay_in(150), burn(150), repaid_message(100)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 0);
    }

    #[test]
    fn engine_rounds_a_partial_decrement_down() {
        let response = finish(&parked(100, 100, FIXED_POINT_ONE * 3 / 2));
        assert_eq!(
            response.sync_messages,
            vec![pay_in(100), burn(100), repaid_message(66)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 34);
    }

    #[test]
    #[should_panic(expected = "Position debt is below minimum_debt_amount")]
    fn engine_refuses_to_leave_a_debt_below_the_floor() {
        let _ = finish(&parked(995, 1_000, FIXED_POINT_ONE));
    }

    #[test]
    fn engine_clears_the_debt_despite_the_floor() {
        let response = finish(&parked(1_000, 1_000, FIXED_POINT_ONE));
        assert_eq!(
            response.sync_messages,
            vec![pay_in(1_000), burn(1_000), repaid_message(1_000)]
        );
    }

    #[test]
    fn engine_still_burns_zero_for_a_zero_repayment() {
        // The debt is under the floor already; a zero repayment does not check it.
        let response = finish(&parked(0, 5, FIXED_POINT_ONE));
        assert_eq!(
            response.sync_messages,
            vec![pay_in(0), burn(0), repaid_message(0)]
        );
    }

    #[test]
    fn engine_still_burns_zero_for_a_debt_free_position() {
        let response = finish(&parked(50, 0, FIXED_POINT_ONE));
        assert_eq!(
            response.sync_messages,
            vec![pay_in(0), burn(0), repaid_message(0)]
        );
    }

    #[test]
    #[should_panic(expected = "Total normalized debt underflow")]
    fn engine_refuses_a_decrement_beyond_the_total_debt() {
        let mut before = parked(400, 1_000, FIXED_POINT_ONE);
        before.fee.total_normalized_debt = 100;
        let _ = finish(&before);
    }

    // --- floor boundaries, ceiling and parked position ---

    fn with_floor(mut engine: Engine, minimum_debt_amount: u128) -> Engine {
        engine.parameters.minimum_debt_amount = minimum_debt_amount;
        engine
    }

    #[test]
    fn engine_leaves_a_debt_above_the_floor() {
        let response = finish(&with_floor(parked(9, 20, FIXED_POINT_ONE), 10));
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 11);
    }

    #[test]
    fn engine_leaves_a_debt_exactly_at_the_floor() {
        let response = finish(&with_floor(parked(10, 20, FIXED_POINT_ONE), 10));
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 10);
    }

    #[test]
    #[should_panic(expected = "Position debt is below minimum_debt_amount")]
    fn engine_refuses_a_remainder_one_below_the_floor() {
        let _ = finish(&with_floor(parked(11, 20, FIXED_POINT_ONE), 10));
    }

    #[test]
    fn engine_clears_a_debt_of_twenty_despite_the_floor() {
        let response = finish(&with_floor(parked(20, 20, FIXED_POINT_ONE), 10));
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 0);
    }

    #[test]
    #[should_panic(expected = "Position debt is below minimum_debt_amount")]
    fn engine_refuses_a_fractional_remainder_below_the_floor() {
        // Debt 2 at 1.5, repaying 2 removes 1 and leaves 1 (worth 1.5) under the floor 2.
        let _ = finish(&with_floor(parked(2, 2, FIXED_POINT_ONE * 3 / 2), 2));
    }

    #[test]
    fn engine_clears_a_fractional_debt_with_enough_budget() {
        let response = finish(&with_floor(parked(3, 2, FIXED_POINT_ONE * 3 / 2), 2));
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 0);
    }

    #[test]
    fn engine_allows_a_partial_repayment_while_the_total_is_over_the_ceiling() {
        // Total debt 100 at accumulator 2.0 is worth 200, above the ceiling 100: minting is
        // refused, repaying is not.
        let mut before = with_floor(parked(20, 100, 2 * FIXED_POINT_ONE), 1);
        before.parameters.debt_ceiling = 100;
        let response = finish(&before);
        assert_eq!(response.sync_messages[..2], [pay_in(20), burn(20)]);
        assert_eq!(response.sync_messages.len(), 3);
        assert_eq!(response.sync_messages[2], repaid_message(10));
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 90);
    }

    #[test]
    fn engine_replies_to_the_parked_position() {
        let mut before = parked(400, 1_000, FIXED_POINT_ONE);
        before.pending = Some(Pending::RepayDebt {
            position: ADMIN,
            maximum_amount: 400,
            from_holding: HOLDING,
            normalized_debt_amount: 1_000,
        });
        let response = finish(&before);
        assert_eq!(
            response.sync_messages.last(),
            Some(&SyncMessage::new(
                stablecoin(ADMIN),
                &Internal::DebtRepaid {
                    normalized_debt_delta: 400,
                },
            ))
        );
    }

    // --- position, repaid ---

    fn repaid(from: Actor, pre: Option<&StablecoinState>, delta: u128) -> Response {
        run(
            stablecoin(OWNER),
            Some(from),
            false,
            pre,
            &Internal::DebtRepaid {
                normalized_debt_delta: delta,
            },
        )
    }

    #[test]
    fn position_removes_the_debt_delta() {
        let response = repaid(engine(), Some(&position(1_000)), 400);
        assert_eq!(response.post_state, Some(position(600).encode().into()));
        assert!(response.sync_messages.is_empty());
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn position_refuses_a_delta_from_a_non_engine_actor() {
        let _ = repaid(stablecoin(OWNER), Some(&position(1_000)), 1);
    }

    #[test]
    #[should_panic(expected = "Repayment decrement is capped at outstanding normalized debt")]
    fn position_refuses_a_delta_beyond_its_debt() {
        let _ = repaid(engine(), Some(&position(10)), 11);
    }
}
