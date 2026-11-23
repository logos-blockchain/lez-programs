//! The stablecoin program on LEZ's actor interface.
//!
//! A handler sees one actor: the engine (global parameters, fee accumulator, redemption price),
//! a position on its owner's own account, or a seat (an admin or freeze-authority account that
//! only forwards a message). Every operation is a conversation between them; the engine reads
//! the clock first, parking the operation in its state until the reply arrives.
//!
//! Trust rests on the sender, checked in this order by [`handle_message`]:
//!
//! - the clock's reply is accepted only from `clock_actor()`;
//! - [`Internal`] messages only from stablecoin actors, and each receiver checks that the sender is
//!   the exact actor it expects;
//! - the oracle's reply only from the market price oracle the engine was configured with;
//! - a [`Message`] from anyone else, authorized by the receiving account's signature.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
pub use stablecoin_core as core;
use stablecoin_core::{compute_engine_pda, Engine, Internal, Message, Position, StablecoinState};

mod checks;
#[cfg(test)]
mod test_utils;

/// The guest entry point. Each kind of sender has one format: the clock's reply, [`Internal`]
/// from stablecoin actors, the oracle's reply from the configured oracle, then a [`Message`]
/// from anyone else.
///
/// # Panics
/// - `"internal stablecoin message must decode"` when a stablecoin actor sends bytes that are not
///   an [`Internal`].
/// - `"stablecoin message must decode"` when anyone else sends bytes that are not a [`Message`].
pub fn handle_message(input: &ReceiveInput) -> Response {
    if input.from == Some(clock_core::clock_actor()) {
        panic!("not ported yet");
    }
    if input.from_own_program() {
        return handle_internal(input);
    }
    if sent_by_configured_oracle(input) {
        panic!("not ported yet");
    }
    handle_public(input)
}

/// An [`Internal`] message from a stablecoin actor.
fn handle_internal(input: &ReceiveInput) -> Response {
    let message: Internal =
        borsh::from_slice(&input.message).expect("internal stablecoin message must decode");
    match message {
        Internal::CreateStablecoinToken {
            name: _,
            token_program_id: _,
        } => panic!("not ported yet"),
        Internal::InitializeEngine(_args) => panic!("not ported yet"),
        Internal::Admin(_action) => panic!("not ported yet"),
        Internal::OpenRequest {
            collateral_holding: _,
            initial_collateral_amount: _,
        } => panic!("not ported yet"),
        Internal::GenerateDebtRequest {
            amount: _,
            to: _,
            collateral_amount: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Internal::RepayDebtRequest {
            maximum_amount: _,
            from_holding: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Internal::WithdrawRequest {
            amount: _,
            to: _,
            collateral_after: _,
            normalized_debt_amount: _,
        } => panic!("not ported yet"),
        Internal::OpenApproved {
            token_program_id: _,
            collateral_definition_id: _,
            collateral_holding: _,
            initial_collateral_amount: _,
        } => panic!("not ported yet"),
        Internal::DebtGenerated {
            normalized_debt_delta: _,
        } => panic!("not ported yet"),
        Internal::DebtRepaid {
            normalized_debt_delta: _,
        } => panic!("not ported yet"),
        Internal::WithdrawApproved { amount: _, to: _ } => panic!("not ported yet"),
    }
}

/// A [`Message`] from anyone else.
fn handle_public(input: &ReceiveInput) -> Response {
    let message: Message =
        borsh::from_slice(&input.message).expect("stablecoin message must decode");
    match message {
        Message::Initialize(_args) => panic!("not ported yet"),
        Message::AccrueStabilityFee => {
            let _engine = engine_state(input);
            panic!("not ported yet")
        }
        Message::UpdateRedemptionRate => panic!("not ported yet"),
        Message::RefreshGlobals { consult_oracle: _ } => panic!("not ported yet"),
        Message::OpenPosition {
            collateral_holding: _,
            initial_collateral_amount: _,
        } => panic!("not ported yet"),
        Message::DepositCollateral {
            collateral_holding: _,
            amount: _,
        } => panic!("not ported yet"),
        Message::WithdrawCollateral { to: _, amount: _ } => panic!("not ported yet"),
        Message::GenerateDebt { to: _, amount: _ } => panic!("not ported yet"),
        Message::RepayDebt {
            stablecoin_holding: _,
            maximum_amount: _,
        } => panic!("not ported yet"),
        Message::ClosePosition => panic!("not ported yet"),
        Message::SetMinimumCollateralizationRatio { new_ratio: _ } => panic!("not ported yet"),
        Message::SetControllerGains {
            new_proportional_gain: _,
            new_integral_gain: _,
        } => panic!("not ported yet"),
        Message::SetMarketPriceOracle {
            oracle_account_id: _,
            oracle_program_id: _,
        } => panic!("not ported yet"),
        Message::SetTimingParameters {
            minimum_milliseconds_between_rate_updates: _,
            maximum_oracle_price_age_milliseconds: _,
        } => panic!("not ported yet"),
        Message::SetAdmin {
            new_admin_account_id: _,
        } => panic!("not ported yet"),
        Message::SetFreezeAuthority {
            new_freeze_authority_account_id: _,
        } => panic!("not ported yet"),
        Message::SetStabilityFeePerMillisecond { new_rate: _ } => panic!("not ported yet"),
        Message::Freeze => panic!("not ported yet"),
        Message::Unfreeze => panic!("not ported yet"),
        Message::ResetRedemptionRateController => panic!("not ported yet"),
        Message::SetDebtLimits {
            minimum_debt_amount: _,
            debt_ceiling: _,
        } => panic!("not ported yet"),
    }
}

/// Whether the receiver is an engine and the sender is the market price oracle it was
/// configured with.
fn sent_by_configured_oracle(input: &ReceiveInput) -> bool {
    match StablecoinState::decode(&input.pre_state) {
        Some(StablecoinState::Engine(engine)) => {
            input.from
                == Some(Actor::new(
                    engine.parameters.market_price_oracle_id,
                    engine.parameters.market_price_oracle_program_id,
                ))
        }
        Some(StablecoinState::Position(_)) | None => false,
    }
}

/// The program's own account id: the header account every PDA derives from.
fn program_id(input: &ReceiveInput) -> AccountId {
    input.receiver.program_account_id
}

/// The stablecoin program's actor on `account_id`.
fn stablecoin_actor(input: &ReceiveInput, account_id: AccountId) -> Actor {
    Actor::new(account_id, program_id(input))
}

/// The engine's actor.
fn engine_actor(input: &ReceiveInput) -> Actor {
    stablecoin_actor(input, compute_engine_pda(program_id(input)))
}

/// The receiver's engine state.
///
/// # Panics
/// - `"this actor is not the engine"` when the receiver holds no engine state.
fn engine_state(input: &ReceiveInput) -> Engine {
    match StablecoinState::decode(&input.pre_state) {
        Some(StablecoinState::Engine(engine)) => engine,
        Some(StablecoinState::Position(_)) | None => panic!("this actor is not the engine"),
    }
}

/// The receiver's position state.
///
/// # Panics
/// - `"this account has no position"` when the receiver holds no position state.
#[expect(dead_code, reason = "used from Task 4: open_position")]
fn position_state(input: &ReceiveInput) -> Position {
    match StablecoinState::decode(&input.pre_state) {
        Some(StablecoinState::Position(position)) => position,
        Some(StablecoinState::Engine(_)) | None => panic!("this account has no position"),
    }
}

#[expect(dead_code, reason = "used from Task 3: initialize")]
fn save_engine(engine: Engine) -> Response {
    Response::set_state(StablecoinState::Engine(engine).encode())
}

#[expect(dead_code, reason = "used from Task 4: open_position")]
fn save_position(position: Position) -> Response {
    Response::set_state(StablecoinState::Position(position).encode())
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;

    #[test]
    #[should_panic(expected = "this actor is not the engine")]
    fn a_poke_at_an_account_that_is_not_the_engine_panics() {
        let _ = run(
            stablecoin(OWNER),
            None,
            false,
            None,
            &Message::AccrueStabilityFee,
        );
    }

    #[test]
    #[should_panic(expected = "internal stablecoin message must decode")]
    fn an_internal_sender_with_a_public_message_panics() {
        let _ = run(
            engine(),
            Some(stablecoin(OWNER)),
            false,
            Some(&StablecoinState::Engine(engine_at(1_000))),
            &Message::Freeze,
        );
    }
}
