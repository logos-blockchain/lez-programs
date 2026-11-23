//! `GenerateDebt`: the owner mints stablecoin against the position's collateral. The position
//! asks the engine, which accrues fees at the clock's time, prices the new debt, enforces the
//! freeze flag, the debt floor, the debt ceiling and the collateralization ratio, mints the
//! stablecoin itself and tells the position how much normalized debt to add.

use alloy_primitives::U512;
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};
use stablecoin_core::{
    compute_stablecoin_definition_pda_seed,
    debt_limits::{meets_debt_floor, within_debt_ceiling},
    event::{DebtGenerated, StablecoinEvent as _},
    math::{compute_current_redemption_price_wide, mul_div_ceil, FIXED_POINT_ONE},
    Engine, Internal, Pending,
};

use crate::{
    accrue_stability_fee::accrue, checks, clock, engine_state, position_state, save_engine,
    save_position, stablecoin_actor,
};

/// At the owner's actor, signed as a root message: ask the engine to mint `amount` to `to`
/// against the position's current collateral and debt. The position is not changed in this run.
///
/// # Panics
/// - `"only the account's owner may send this"` when the message is nested or unsigned.
/// - `"this account has no position"` when the receiver holds no position.
pub(crate) fn generate_debt(input: &ReceiveInput, to: AccountId, amount: u128) -> Response {
    checks::assert_owner(input);
    let position = position_state(input);
    Response::keep_state().send_sync(
        crate::engine_actor(input),
        &Internal::GenerateDebtRequest {
            amount,
            to,
            collateral_amount: position.collateral_amount,
            normalized_debt_amount: position.normalized_debt_amount,
        },
    )
}

/// At the engine, from the position's actor: refuse while frozen, park the request and ask the
/// clock for the time. Only the program's own code, after the owner check, sends
/// `GenerateDebtRequest` from the position's state.
///
/// # Panics
/// - `"this actor is not the engine"` when the receiver holds no engine state.
/// - `"Protocol is frozen"` while frozen.
/// - `"the engine is already waiting for a reply"` when another operation is parked.
pub(crate) fn request(
    input: &ReceiveInput,
    from: Actor,
    amount: u128,
    to: AccountId,
    collateral_amount: u128,
    normalized_debt_amount: u128,
) -> Response {
    let engine = engine_state(input);
    assert!(!engine.parameters.is_frozen, "Protocol is frozen");
    clock::read_clock(
        engine,
        Pending::GenerateDebt {
            position: from.account_id,
            amount,
            to,
            collateral_amount,
            normalized_debt_amount,
        },
    )
}

/// At the engine, on the clock's reply: accrue to `now`, price the new debt at the fresh
/// accumulator (rounded up), check the floor, the ceiling and the collateralization ratio,
/// then mint to `to` and tell the position its debt delta. The surplus mint (if any) goes first.
///
/// # Panics
/// - `"Position normalized_debt_amount overflow"` / `"Total normalized debt overflow"`.
/// - `"Position debt is below minimum_debt_amount"` when a non-zero delta leaves debt under the
///   floor.
/// - `"Global debt ceiling exceeded"` when a non-zero mint takes the total over the ceiling.
/// - `"Redemption price projected to zero"` when the projected price is zero.
/// - `"Position is undercollateralized"` when the position would fall under the ratio.
#[expect(
    clippy::too_many_arguments,
    reason = "the parked `Pending::GenerateDebt` fields plus the engine, the clock's time and the input"
)]
pub(crate) fn finish(
    input: &ReceiveInput,
    mut engine: Engine,
    position: AccountId,
    amount: u128,
    to: AccountId,
    collateral_amount: u128,
    normalized_debt_amount: u128,
    now: u64,
) -> Response {
    let accrued = accrue(input, &mut engine, now);
    let current_accumulator = engine.fee.accumulated_rate_at_last_accrual;
    // Round up: the borrower receives exactly `amount`, so nominal debt grows by at least that.
    let delta = mul_div_ceil(amount, FIXED_POINT_ONE, current_accumulator);
    let new_debt = normalized_debt_amount
        .checked_add(delta)
        .expect("Position normalized_debt_amount overflow");
    engine.fee.total_normalized_debt = engine
        .fee
        .total_normalized_debt
        .checked_add(delta)
        .expect("Total normalized debt overflow");
    if delta != 0 {
        assert!(
            meets_debt_floor(
                new_debt,
                current_accumulator,
                engine.parameters.minimum_debt_amount
            ),
            "Position debt is below minimum_debt_amount"
        );
    }
    if amount != 0 {
        assert!(
            within_debt_ceiling(
                engine.fee.total_normalized_debt,
                current_accumulator,
                engine.parameters.debt_ceiling
            ),
            "Global debt ceiling exceeded"
        );
    }
    let redemption_price = compute_current_redemption_price_wide(
        engine.redemption.redemption_price_at_last_update,
        engine.redemption.redemption_rate_per_millisecond,
        engine.redemption.last_updated_at,
        now,
    );
    // A rate below 1.0 can decay the projected price to zero, which would zero the required
    // collateral and let any mint through.
    assert!(
        !redemption_price.is_zero(),
        "Redemption price projected to zero"
    );
    checks::assert_position_is_collateralized(
        collateral_amount,
        new_debt,
        U512::from(current_accumulator),
        redemption_price,
        engine.parameters.minimum_collateralization_ratio,
    );
    let definition = engine.parameters.stablecoin_definition_id;
    let token_program_id = engine.parameters.token_program_id;
    accrued
        .attach(save_engine(engine))
        .send(
            SyncMessage::new(
                Actor::new(definition, token_program_id),
                &token_core::Message::Mint {
                    definition,
                    to,
                    amount,
                },
            )
            .with_pda_seeds([compute_stablecoin_definition_pda_seed()]),
        )
        .send_sync(
            stablecoin_actor(input, position),
            &Internal::DebtGenerated {
                normalized_debt_delta: delta,
            },
        )
        .event(
            DebtGenerated {
                owner: position,
                to,
                amount,
                normalized_debt_delta: delta,
            }
            .to_program_event(),
        )
}

/// At the owner's actor, from the engine: add the normalized debt the engine priced.
///
/// # Panics
/// - `"only the engine may send this"` when the sender is anyone else.
/// - `"this account has no position"` when the receiver holds no position.
/// - `"Position normalized_debt_amount overflow"` when the sum overflows.
pub(crate) fn generated(input: &ReceiveInput, normalized_debt_delta: u128) -> Response {
    checks::assert_from_engine(input);
    let mut position = position_state(input);
    position.normalized_debt_amount = position
        .normalized_debt_amount
        .checked_add(normalized_debt_delta)
        .expect("Position normalized_debt_amount overflow");
    save_position(position)
}

#[cfg(test)]
mod tests {
    use stablecoin_core::{
        compute_stablecoin_definition_pda_seed,
        event::{DebtGenerated, StabilityFeeAccrued, StablecoinEvent as _},
        math::mul_div_ceil,
        Internal,
    };

    use super::*;
    use crate::test_utils::*;

    const NOW: u64 = 1_000_000_000;

    fn position(collateral_amount: u128, normalized_debt_amount: u128) -> StablecoinState {
        StablecoinState::Position(Position {
            token_program_id: TOKEN,
            collateral_amount,
            normalized_debt_amount,
        })
    }

    fn generate(
        from: Option<Actor>,
        is_authorized: bool,
        pre: Option<&StablecoinState>,
    ) -> Response {
        run(
            stablecoin(OWNER),
            from,
            is_authorized,
            pre,
            &Message::GenerateDebt {
                to: RECIPIENT,
                amount: 1_000,
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
            &Internal::GenerateDebtRequest {
                amount: 1_000,
                to: RECIPIENT,
                collateral_amount: 1_500,
                normalized_debt_amount: 20,
            },
        )
    }

    /// A parked engine at accumulator 1.0 and redemption price 1.0 with the default ratio 1.5x.
    fn parked(
        amount: u128,
        collateral_amount: u128,
        normalized_debt_amount: u128,
        total_normalized_debt: u128,
    ) -> Engine {
        let mut engine = engine_at(NOW);
        engine.fee.total_normalized_debt = total_normalized_debt;
        engine.pending = Some(Pending::GenerateDebt {
            position: OWNER,
            amount,
            to: RECIPIENT,
            collateral_amount,
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

    fn user_mint(amount: u128) -> SyncMessage {
        SyncMessage::new(
            token(definition_id()),
            &token_core::Message::Mint {
                definition: definition_id(),
                to: RECIPIENT,
                amount,
            },
        )
        .with_pda_seeds([compute_stablecoin_definition_pda_seed()])
    }

    fn generated_message(delta: u128) -> SyncMessage {
        SyncMessage::new(
            stablecoin(OWNER),
            &Internal::DebtGenerated {
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

    fn generated_event(amount: u128, delta: u128) -> lee_core::program::ProgramEvent {
        DebtGenerated {
            owner: OWNER,
            to: RECIPIENT,
            amount,
            normalized_debt_delta: delta,
        }
        .to_program_event()
    }

    // --- position, root message ---

    #[test]
    fn position_sends_the_request_and_keeps_its_state() {
        let response = generate(None, true, Some(&position(1_500, 20)));
        assert_eq!(response.post_state, None);
        assert_eq!(
            response.sync_messages,
            vec![SyncMessage::new(
                engine(),
                &Internal::GenerateDebtRequest {
                    amount: 1_000,
                    to: RECIPIENT,
                    collateral_amount: 1_500,
                    normalized_debt_amount: 20,
                },
            )]
        );
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_an_unsigned_request() {
        let _ = generate(None, false, Some(&position(1_500, 20)));
    }

    #[test]
    #[should_panic(expected = "only the account's owner may send this")]
    fn position_refuses_a_nested_request() {
        let _ = generate(Some(outsider(OWNER)), true, Some(&position(1_500, 20)));
    }

    #[test]
    #[should_panic(expected = "this account has no position")]
    fn position_refuses_a_request_without_a_position() {
        let _ = generate(None, true, None);
    }

    // --- engine, request ---

    #[test]
    fn engine_parks_the_request_and_asks_the_clock() {
        let response = request_message(false);
        let mut expected = engine_at(NOW);
        expected.pending = Some(Pending::GenerateDebt {
            position: OWNER,
            amount: 1_000,
            to: RECIPIENT,
            collateral_amount: 1_500,
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
    fn engine_refuses_a_request_while_frozen() {
        let _ = request_message(true);
    }

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn request_at_an_actor_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            Some(stablecoin(OWNER)),
            false,
            Some(&position(1_500, 0)),
            &Internal::GenerateDebtRequest {
                amount: 1,
                to: RECIPIENT,
                collateral_amount: 1,
                normalized_debt_amount: 0,
            },
        );
    }

    // --- engine, clock reply ---

    #[test]
    fn engine_mints_and_tells_the_position_its_debt_delta() {
        let before = parked(1_000, 1_500, 0, 0);
        let response = finish(&before);
        let mut expected = before.clone();
        expected.pending = None;
        expected.fee.total_normalized_debt = 1_000;
        assert_eq!(saved_engine(&response), expected);
        assert_eq!(
            response.sync_messages,
            vec![user_mint(1_000), generated_message(1_000)]
        );
        assert_eq!(
            response.events,
            vec![
                accrued_event(FIXED_POINT_ONE, 0),
                generated_event(1_000, 1_000)
            ]
        );
    }

    #[test]
    #[should_panic(expected = "Position is undercollateralized")]
    fn engine_refuses_one_unit_below_the_ratio() {
        let _ = finish(&parked(1_001, 1_500, 0, 0));
    }

    #[test]
    fn engine_mints_the_surplus_first_when_fees_accrued() {
        // Total debt 100 at a fee of 2.0 per millisecond for 1 ms: the accumulator doubles and
        // 100 of fees accrue.
        let mut before = parked(1_000, 10_000, 0, 100);
        before.parameters.stability_fee_per_millisecond = 2 * FIXED_POINT_ONE;
        before.fee.last_accrued_at = NOW - 1;
        let response = finish(&before);
        let delta = mul_div_ceil(1_000, FIXED_POINT_ONE, 2 * FIXED_POINT_ONE);
        assert_eq!(delta, 500);
        let saved = saved_engine(&response);
        assert_eq!(
            saved.fee.accumulated_rate_at_last_accrual,
            2 * FIXED_POINT_ONE
        );
        assert_eq!(saved.fee.last_accrued_at, NOW);
        assert_eq!(saved.fee.total_normalized_debt, 100 + delta);
        let surplus_mint = SyncMessage::new(
            token(definition_id()),
            &token_core::Message::Mint {
                definition: definition_id(),
                to: surplus_id(),
                amount: 100,
            },
        )
        .with_pda_seeds([compute_stablecoin_definition_pda_seed()]);
        assert_eq!(
            response.sync_messages,
            vec![surplus_mint, user_mint(1_000), generated_message(delta)]
        );
        assert_eq!(
            response.events,
            vec![
                accrued_event(2 * FIXED_POINT_ONE, 100),
                generated_event(1_000, delta)
            ]
        );
    }

    #[test]
    fn engine_rounds_the_debt_delta_up() {
        let mut before = parked(1, 10_000, 100, 100);
        before.fee.accumulated_rate_at_last_accrual = 2 * FIXED_POINT_ONE;
        let response = finish(&before);
        assert_eq!(
            response.sync_messages,
            vec![user_mint(1), generated_message(1)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 101);
    }

    #[test]
    fn engine_still_sends_a_zero_mint() {
        let response = finish(&parked(0, 1_500, 0, 0));
        assert_eq!(
            response.sync_messages,
            vec![user_mint(0), generated_message(0)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 0);
    }

    #[test]
    #[should_panic(expected = "Position debt is below minimum_debt_amount")]
    fn engine_refuses_a_debt_below_the_floor() {
        let _ = finish(&parked(9, 1_500, 0, 0));
    }

    #[test]
    fn engine_accepts_a_debt_at_the_floor() {
        let _ = finish(&parked(10, 1_500, 0, 0));
    }

    #[test]
    #[should_panic(expected = "Global debt ceiling exceeded")]
    fn engine_refuses_to_pass_the_ceiling() {
        let mut before = parked(401, 100_000, 0, 600);
        before.parameters.debt_ceiling = 1_000;
        let _ = finish(&before);
    }

    #[test]
    fn engine_accepts_to_reach_the_ceiling() {
        let mut before = parked(400, 100_000, 0, 600);
        before.parameters.debt_ceiling = 1_000;
        let _ = finish(&before);
    }

    #[test]
    #[should_panic(expected = "Redemption price projected to zero")]
    fn engine_refuses_a_redemption_price_that_projects_to_zero() {
        let mut before = parked(1_000, 1_500, 0, 0);
        before.redemption.redemption_rate_per_millisecond =
            FIXED_POINT_ONE - FIXED_POINT_ONE / 100_000;
        before.redemption.last_updated_at = NOW - 7_000_000;
        let _ = finish(&before);
    }

    #[test]
    #[should_panic(expected = "Position normalized_debt_amount overflow")]
    fn engine_refuses_a_position_debt_overflow() {
        let _ = finish(&parked(1_000, 1_500, u128::MAX, 0));
    }

    #[test]
    #[should_panic(expected = "Total normalized debt overflow")]
    fn engine_refuses_a_total_debt_overflow() {
        let _ = finish(&parked(1_000, 1_500, 0, u128::MAX));
    }

    // --- floor, ceiling and rounding boundaries ---

    /// Accumulator 1.5, so one minted unit rounds up to one unit of normalized debt worth 1.5.
    fn fractional(amount: u128, minimum_debt_amount: u128, debt_ceiling: u128) -> Engine {
        let mut engine = parked(amount, 10_000, 0, 0);
        engine.fee.accumulated_rate_at_last_accrual = FIXED_POINT_ONE * 3 / 2;
        engine.parameters.minimum_debt_amount = minimum_debt_amount;
        engine.parameters.debt_ceiling = debt_ceiling;
        engine
    }

    #[test]
    #[should_panic(expected = "Position debt is below minimum_debt_amount")]
    fn engine_refuses_a_fractional_debt_below_the_floor() {
        // Minting 1 adds normalized debt 1, whose exact accrued value 1.5 is under the floor 2.
        let _ = finish(&fractional(1, 2, 100));
    }

    #[test]
    #[should_panic(expected = "Global debt ceiling exceeded")]
    fn engine_refuses_a_fractional_debt_above_the_ceiling() {
        // The same 1.5 exceeds the ceiling 1 although only one token is minted.
        let _ = finish(&fractional(1, 1, 1));
    }

    #[test]
    fn engine_accepts_a_fractional_debt_at_the_floor_and_the_ceiling() {
        let response = finish(&fractional(2, 3, 3));
        assert_eq!(
            response.sync_messages,
            vec![user_mint(2), generated_message(2)]
        );
        assert_eq!(saved_engine(&response).fee.total_normalized_debt, 2);
    }

    #[test]
    #[should_panic(expected = "Global debt ceiling exceeded")]
    fn engine_refuses_a_mint_when_accrued_fees_put_the_total_over_the_ceiling() {
        // Total debt 100 at accumulator 2.0 is worth 200, above the ceiling 100.
        let mut before = parked(1, 10_000, 100, 100);
        before.fee.accumulated_rate_at_last_accrual = 2 * FIXED_POINT_ONE;
        before.parameters.minimum_debt_amount = 1;
        before.parameters.debt_ceiling = 100;
        let _ = finish(&before);
    }

    #[test]
    fn engine_compares_a_projected_price_beyond_u128_instead_of_overflowing() {
        let mut before = parked(1, 1_000_000_000_000, 0, 0);
        before.parameters.minimum_debt_amount = 1;
        before.parameters.minimum_collateralization_ratio = FIXED_POINT_ONE * 11 / 10;
        before.redemption.redemption_rate_per_millisecond =
            FIXED_POINT_ONE + FIXED_POINT_ONE / 100_000;
        before.redemption.last_updated_at = NOW - 2_700_000;
        let response = finish(&before);
        assert_eq!(
            response.sync_messages,
            vec![user_mint(1), generated_message(1)]
        );
    }

    #[test]
    fn engine_replies_to_the_parked_position() {
        let mut before = parked(1_000, 1_500, 0, 0);
        before.pending = Some(Pending::GenerateDebt {
            position: ADMIN,
            amount: 1_000,
            to: RECIPIENT,
            collateral_amount: 1_500,
            normalized_debt_amount: 0,
        });
        let response = finish(&before);
        assert_eq!(
            response.sync_messages,
            vec![
                user_mint(1_000),
                SyncMessage::new(
                    stablecoin(ADMIN),
                    &Internal::DebtGenerated {
                        normalized_debt_delta: 1_000,
                    },
                ),
            ]
        );
    }

    // --- position, generated ---

    fn generated(from: Actor, pre: Option<&StablecoinState>, delta: u128) -> Response {
        run(
            stablecoin(OWNER),
            Some(from),
            false,
            pre,
            &Internal::DebtGenerated {
                normalized_debt_delta: delta,
            },
        )
    }

    #[test]
    fn position_adds_the_debt_delta() {
        let response = generated(engine(), Some(&position(1_500, 20)), 1_000);
        assert_eq!(
            response.post_state,
            Some(position(1_500, 1_020).encode().into())
        );
        assert!(response.sync_messages.is_empty());
    }

    #[test]
    #[should_panic(expected = "only the engine may send this")]
    fn position_refuses_a_delta_from_a_non_engine_actor() {
        let _ = generated(stablecoin(OWNER), Some(&position(1_500, 20)), 1);
    }

    #[test]
    #[should_panic(expected = "Position normalized_debt_amount overflow")]
    fn position_refuses_a_delta_that_overflows() {
        let _ = generated(engine(), Some(&position(1_500, u128::MAX)), 1);
    }
}
