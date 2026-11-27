//! `WithdrawCollateral`: the owner takes collateral out of the vault. The position asks the
//! engine, which approves after checking the freeze flag and, when the position has debt, the
//! collateralization ratio at the clock's time; the position then decrements its collateral and
//! transfers from the vault.
//!
//! The destination may not be the position's own vault: the token would debit and credit the
//! same holding, so the position would record a withdrawal that moved nothing out.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_position_vault_pda_seed,
    event::{CollateralWithdrawn, StablecoinEvent as _},
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    Engine, Internal, Pending,
};

use crate::{
    checks, clock, engine_actor, engine_state, position_state, save_engine, save_position,
};

/// At the owner's actor, signed as a root message: work out the collateral left after the
/// withdrawal and ask the engine to approve. The position is not changed in this run.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Withdrawal amount exceeds position collateral"` when `amount` exceeds the collateral.
/// - `"Collateral cannot be withdrawn to the position's own vault"` when a non-zero withdrawal
///   names the vault as its destination.
pub(crate) fn withdraw_collateral(input: &ReceiveInput, to: AccountId, amount: u128) -> Response {
    checks::assert_owner(input);
    let position = position_state(input);
    assert_leaves_the_vault(input, to, amount);
    let collateral_after = position
        .collateral_amount
        .checked_sub(amount)
        .expect("Withdrawal amount exceeds position collateral");
    Response::keep_state().send_sync(
        engine_actor(input),
        &Internal::WithdrawRequest {
            amount,
            to,
            collateral_after,
            normalized_debt_amount: position.normalized_debt_amount,
        },
    )
}

/// At the engine, from the position's actor: while frozen only a debt-free position may leave.
/// A zero withdrawal, or one from a debt-free position, is approved at once; otherwise the
/// request is parked and the clock asked for the time. Only the program's own code, after the
/// owner check, sends `WithdrawRequest` from the position's state.
///
/// # Panics
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"Protocol is frozen"` while frozen and the position has debt.
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn request(
    input: &ReceiveInput,
    from: Actor,
    amount: u128,
    to: AccountId,
    collateral_after: u128,
    normalized_debt_amount: u128,
) -> Response {
    let engine = engine_state(input);
    assert!(
        !engine.parameters.is_frozen || normalized_debt_amount == 0,
        "Protocol is frozen"
    );
    if amount == 0 || normalized_debt_amount == 0 {
        return Response::keep_state().send_sync(from, &Internal::WithdrawApproved { amount, to });
    }
    clock::read_clock(
        engine,
        Pending::Withdraw {
            position: from.account_id,
            amount,
            to,
            collateral_after,
            normalized_debt_amount,
        },
    )
}

/// At the engine, on the clock's reply: check the collateralization ratio of what is left, with
/// the fee accumulator and the redemption price projected to `now`, and approve. Nothing
/// accrues, so `fee` and `redemption` are saved as they were.
///
/// # Panics
/// - `"Redemption price projected to zero"` when the projected price is zero.
/// - `"Position is undercollateralized"` when the position would fall under the ratio.
#[expect(
    clippy::too_many_arguments,
    reason = "the parked `Pending::Withdraw` fields plus the engine, the clock's time and the input"
)]
pub(crate) fn finish(
    input: &ReceiveInput,
    engine: Engine,
    position: AccountId,
    amount: u128,
    to: AccountId,
    collateral_after: u128,
    normalized_debt_amount: u128,
    now: u64,
) -> Response {
    let redemption_price = compute_current_redemption_price_wide(
        engine.redemption.redemption_price_at_last_update,
        engine.redemption.redemption_rate_per_millisecond,
        engine.redemption.last_updated_at,
        now,
    );
    // A rate below 1.0 can decay the projected price to zero, which would zero the required
    // collateral and let an indebted position be drained.
    assert!(
        !redemption_price.is_zero(),
        "Redemption price projected to zero"
    );
    checks::assert_position_is_collateralized(
        collateral_after,
        normalized_debt_amount,
        compute_current_accumulated_rate_wide(
            engine.fee.accumulated_rate_at_last_accrual,
            engine.parameters.stability_fee_per_millisecond,
            engine.fee.last_accrued_at,
            now,
        ),
        redemption_price,
        engine.parameters.minimum_collateralization_ratio,
    );
    save_engine(engine).send_sync(
        crate::stablecoin_actor(input, position),
        &Internal::WithdrawApproved { amount, to },
    )
}

/// At the owner's actor, from the engine: decrement the collateral and transfer `amount` from
/// the vault to `to` (no transfer when it is zero).
///
/// # Panics
/// - `"only the engine may send this"` when the sender is anyone else.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Withdrawal amount exceeds position collateral"` when `amount` exceeds the collateral.
/// - `"Collateral cannot be withdrawn to the position's own vault"` when a non-zero withdrawal
///   names the vault as its destination.
pub(crate) fn approved(input: &ReceiveInput, amount: u128, to: AccountId) -> Response {
    checks::assert_from_engine(input);
    let mut position = position_state(input);
    assert_leaves_the_vault(input, to, amount);
    position.collateral_amount = position
        .collateral_amount
        .checked_sub(amount)
        .expect("Withdrawal amount exceeds position collateral");
    if amount == 0 {
        return Response::keep_state();
    }
    let owner = input.receiver.account_id;
    let vault = stablecoin_core::compute_position_vault_pda(crate::program_id(input), owner);
    let token_program_id = position.token_program_id;
    save_position(position)
        .send(
            SyncMessage::new(
                Actor::new(vault, token_program_id),
                &token_core::Message::Transfer { to, amount },
            )
            .with_pda_seeds([compute_position_vault_pda_seed(owner)]),
        )
        .event(CollateralWithdrawn { owner, to, amount }.to_program_event())
}

/// A non-zero withdrawal must name a destination other than the receiver's own vault.
fn assert_leaves_the_vault(input: &ReceiveInput, to: AccountId, amount: u128) {
    let vault = stablecoin_core::compute_position_vault_pda(
        crate::program_id(input),
        input.receiver.account_id,
    );
    assert!(
        amount == 0 || to != vault,
        "Collateral cannot be withdrawn to the position's own vault"
    );
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        compute_position_vault_pda_seed,
        event::{CollateralWithdrawn, StablecoinEvent as _},
        Internal,
    };

    use super::*;
    use crate::test_utils::*;

    /// A rate one legacy clamp away from 1.0.
    const RATE_DELTA: u128 = FIXED_POINT_ONE / 100_000;
    const NOW: u64 = 1_000_000_000;

    fn position(collateral_amount: u128, normalized_debt_amount: u128) -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_definition_id: COLLATERAL_DEFINITION,
            collateral_amount,
            normalized_debt_amount,
        })
    }

    fn withdraw_to(to: AccountId, amount: u128) -> Response {
        run(
            stablecoin(OWNER),
            None,
            true,
            Some(&position(10, 0)),
            &Message::WithdrawCollateral { to, amount },
        )
    }

    #[test]
    #[should_panic(expected = "Collateral cannot be withdrawn to the position's own vault")]
    fn position_refuses_a_withdrawal_to_its_own_vault() {
        let _ = withdraw_to(vault_id(OWNER), 10);
    }

    #[test]
    fn position_allows_a_zero_withdrawal_that_names_its_own_vault() {
        assert_eq!(
            withdraw_to(vault_id(OWNER), 0),
            Response::keep_state().send_sync(
                engine(),
                &Internal::WithdrawRequest {
                    amount: 0,
                    to: vault_id(OWNER),
                    collateral_after: 10,
                    normalized_debt_amount: 0,
                },
            ),
        );
    }

    #[test]
    #[should_panic(expected = "Collateral cannot be withdrawn to the position's own vault")]
    fn position_refuses_an_approval_that_pays_its_own_vault() {
        let _ = run(
            stablecoin(OWNER),
            Some(engine()),
            false,
            Some(&position(10, 0)),
            &Internal::WithdrawApproved {
                amount: 10,
                to: vault_id(OWNER),
            },
        );
    }

    fn withdraw(
        from: Option<Actor>,
        is_authorized: bool,
        pre: Option<&StablecoinState>,
        amount: u128,
    ) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::WithdrawCollateral {
                to: RECIPIENT,
                amount,
            },
        )
    }

    fn engine_with(frozen: bool) -> Engine {
        let mut engine = engine_at(NOW);
        engine.parameters.is_frozen = frozen;
        engine
    }

    fn request(engine: Engine, amount: u128, debt: u128) -> Response {
        run(
            self::engine(),
            Some(stablecoin(OWNER)),
            false,
            Some(&StablecoinState::Engine(engine)),
            &Internal::WithdrawRequest {
                amount,
                to: RECIPIENT,
                collateral_after: 300,
                normalized_debt_amount: debt,
            },
        )
    }

    fn approved_message(amount: u128) -> SyncMessage {
        SyncMessage::new(
            stablecoin(OWNER),
            &Internal::WithdrawApproved {
                amount,
                to: RECIPIENT,
            },
        )
    }

    /// Redemption price 0.5, fee accumulator 1.0, ratio 1.5, as in the v0.2.5 fixtures: the
    /// required collateral is `debt * 0.75`.
    fn parked_engine(
        collateral_after: u128,
        normalized_debt_amount: u128,
        rate: u128,
        updated_at: u64,
        ratio: u128,
    ) -> Engine {
        let mut engine = engine_at(NOW);
        engine.redemption.redemption_price_at_last_update = FIXED_POINT_ONE / 2;
        engine.redemption.redemption_rate_per_millisecond = rate;
        engine.redemption.last_updated_at = updated_at;
        engine.parameters.minimum_collateralization_ratio = ratio;
        engine.pending = Some(Pending::Withdraw {
            position: OWNER,
            amount: 1,
            to: RECIPIENT,
            collateral_after,
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

    fn finish_default(collateral_after: u128, debt: u128) -> Response {
        finish(&parked_engine(
            collateral_after,
            debt,
            FIXED_POINT_ONE,
            NOW,
            FIXED_POINT_ONE * 3 / 2,
        ))
    }

    // --- position, root message ---

    #[test]
    fn position_sends_the_request_and_keeps_its_state() {
        let response = withdraw(None, true, Some(&position(500, 20)), 200);
        assert_eq!(response.post_state, None);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                engine(),
                &Internal::WithdrawRequest {
                    amount: 200,
                    to: RECIPIENT,
                    collateral_after: 300,
                    normalized_debt_amount: 20,
                },
            )]
        );
    }

    #[test]
    #[should_panic(expected = "Withdrawal amount exceeds position collateral")]
    fn position_refuses_more_than_the_collateral() {
        let _ = withdraw(None, true, Some(&position(500, 20)), 501);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_an_unsigned_withdrawal() {
        let _ = withdraw(None, false, Some(&position(500, 20)), 1);
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_a_nested_withdrawal() {
        let _ = withdraw(Some(outsider(OWNER)), true, Some(&position(500, 20)), 1);
    }

    #[test]
    #[should_panic(expected = "this account has no position")]
    fn position_refuses_a_withdrawal_without_a_position() {
        let _ = withdraw(None, true, None, 1);
    }

    // --- engine, request ---

    #[test]
    fn engine_approves_a_debt_free_request_at_once() {
        let response = request(engine_with(false), 200, 0);
        assert_eq!(response.post_state, None);
        assert_eq!(response.sync_messages, vec![approved_message(200)]);
    }

    #[test]
    fn engine_approves_a_debt_free_request_while_frozen() {
        let response = request(engine_with(true), 200, 0);
        assert_eq!(response.sync_messages, vec![approved_message(200)]);
    }

    #[test]
    fn engine_approves_a_zero_request_with_debt_at_once() {
        let response = request(engine_with(false), 0, 20);
        assert_eq!(response.post_state, None);
        assert_eq!(response.sync_messages, vec![approved_message(0)]);
    }

    #[test]
    fn engine_parks_a_request_with_debt_and_asks_the_clock() {
        let response = request(engine_with(false), 200, 20);
        let mut expected = engine_with(false);
        expected.pending = Some(Pending::Withdraw {
            position: OWNER,
            amount: 200,
            to: RECIPIENT,
            collateral_after: 300,
            normalized_debt_amount: 20,
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
    #[should_panic(expected = "Protocol is frozen")]
    fn engine_refuses_a_request_with_debt_while_frozen() {
        let _ = request(engine_with(true), 200, 20);
    }

    #[test]
    #[should_panic(expected = "Protocol is frozen")]
    fn engine_refuses_a_zero_request_with_debt_while_frozen() {
        let _ = request(engine_with(true), 0, 20);
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn request_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(stablecoin(OWNER)),
            false,
            Some(&position(500, 20)),
            &Internal::WithdrawRequest {
                amount: 1,
                to: RECIPIENT,
                collateral_after: 1,
                normalized_debt_amount: 1,
            },
        );
    }

    // --- engine, clock reply ---

    #[test]
    fn engine_approves_a_healthy_withdrawal_and_changes_nothing_else() {
        // debt 100 needs 75 collateral; 900 remain.
        let before = parked_engine(900, 100, FIXED_POINT_ONE, NOW, FIXED_POINT_ONE * 3 / 2);
        let response = finish(&before);
        let mut expected = before.clone();
        expected.pending = None;
        assert_eq!(saved_engine(&response), expected);
        assert_eq!(response.sync_messages, vec![approved_message(1)]);
    }

    #[test]
    fn engine_approves_at_the_exact_ratio_boundary() {
        // debt 100 needs exactly 75.
        let _ = finish_default(75, 100);
    }

    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn engine_refuses_one_unit_below_the_ratio() {
        let _ = finish_default(74, 100);
    }

    #[test]
    #[should_panic(expected = "Redemption price projected to zero")]
    fn engine_refuses_a_redemption_price_that_projects_to_zero() {
        let _ = finish(&parked_engine(
            500_000,
            300,
            FIXED_POINT_ONE - RATE_DELTA,
            NOW - 7_000_000,
            FIXED_POINT_ONE * 3 / 2,
        ));
    }

    #[test]
    fn engine_compares_a_projected_price_beyond_u128_instead_of_overflowing() {
        // Required about 585_174_061_358 against 900_000_000_000 left.
        let mut engine = parked_engine(
            900_000_000_000,
            1,
            FIXED_POINT_ONE + RATE_DELTA,
            NOW - 2_700_000,
            FIXED_POINT_ONE * 11 / 10,
        );
        engine.redemption.redemption_price_at_last_update = FIXED_POINT_ONE;
        let _ = finish(&engine);
    }

    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn engine_refuses_when_the_projected_price_exceeds_the_collateral() {
        let mut engine = parked_engine(
            999,
            1,
            FIXED_POINT_ONE + RATE_DELTA,
            NOW - 2_700_000,
            FIXED_POINT_ONE * 11 / 10,
        );
        engine.redemption.redemption_price_at_last_update = FIXED_POINT_ONE;
        let _ = finish(&engine);
    }

    // --- position, approved ---

    fn approve(from: Actor, pre: Option<&StablecoinState>, amount: u128) -> Response {
        run(
            stablecoin(OWNER),
            Some(from),
            false,
            pre,
            &Internal::WithdrawApproved {
                amount,
                to: RECIPIENT,
            },
        )
    }

    #[test]
    fn position_transfers_from_the_vault_and_decrements_its_collateral() {
        let response = approve(engine(), Some(&position(500, 20)), 200);
        assert_eq!(response.post_state, Some(position(300, 20).encode().into()));
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                token(vault_id(OWNER)),
                &token_core::Message::Transfer {
                    to: RECIPIENT,
                    amount: 200,
                },
            )
            .with_pda_seeds([compute_position_vault_pda_seed(OWNER)])]
        );
        assert_eq!(
            response.events,
            vec![CollateralWithdrawn {
                owner: OWNER,
                to: RECIPIENT,
                amount: 200,
            }
            .to_program_event()]
        );
    }

    #[test]
    fn position_does_nothing_for_a_zero_withdrawal() {
        let response = approve(engine(), Some(&position(500, 20)), 0);
        assert_eq!(response.post_state, None);
        assert!(response.sync_messages.is_empty());
        assert!(response.events.is_empty());
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn position_refuses_an_approval_from_a_non_engine_actor() {
        let _ = approve(stablecoin(OWNER), Some(&position(500, 20)), 200);
    }

    #[test]
    #[should_panic(expected = "Withdrawal amount exceeds position collateral")]
    fn position_refuses_an_approval_beyond_its_collateral() {
        let _ = approve(engine(), Some(&position(100, 20)), 200);
    }
}
