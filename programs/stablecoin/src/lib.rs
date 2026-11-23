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
use stablecoin_core::{
    compute_engine_pda, AdminAction, Engine, InitializeArgs, Internal, Message, Position,
    StablecoinState,
};

mod accrue_stability_fee;
mod admin;
mod checks;
mod clock;
mod close_position;
mod deposit_collateral;
mod generate_debt;
mod initialize;
mod open_position;
mod oracle;
mod refresh_globals;
mod repay_debt;
#[cfg(test)]
mod test_utils;
mod update_redemption_rate;
mod withdraw_collateral;

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
        return clock::on_reply(input);
    }
    if input.from_own_program() {
        return handle_internal(input);
    }
    if sent_by_configured_oracle(input) {
        return oracle::on_reply(input);
    }
    handle_public(input)
}

/// An [`Internal`] message from a stablecoin actor.
fn handle_internal(input: &ReceiveInput) -> Response {
    let message: Internal =
        borsh::from_slice(&input.message).expect("internal stablecoin message must decode");
    match message {
        Internal::CreateStablecoinToken {
            name,
            token_program_id,
        } => initialize::create_stablecoin_token(input, sender(input), name, token_program_id),
        Internal::InitializeEngine(args) => {
            initialize::initialize_engine(input, sender(input), args)
        }
        Internal::Admin(action) => admin::apply(input, sender(input), action),
        Internal::OpenRequest {
            collateral_holding,
            initial_collateral_amount,
        } => open_position::approve(
            input,
            sender(input),
            collateral_holding,
            initial_collateral_amount,
        ),
        Internal::GenerateDebtRequest {
            amount,
            to,
            collateral_amount,
            normalized_debt_amount,
        } => generate_debt::request(
            input,
            sender(input),
            amount,
            to,
            collateral_amount,
            normalized_debt_amount,
        ),
        Internal::RepayDebtRequest {
            maximum_amount,
            from_holding,
            normalized_debt_amount,
        } => repay_debt::request(
            input,
            sender(input),
            maximum_amount,
            from_holding,
            normalized_debt_amount,
        ),
        Internal::WithdrawRequest {
            amount,
            to,
            collateral_after,
            normalized_debt_amount,
        } => withdraw_collateral::request(
            input,
            sender(input),
            amount,
            to,
            collateral_after,
            normalized_debt_amount,
        ),
        Internal::OpenApproved {
            token_program_id,
            collateral_definition_id,
            collateral_holding,
            initial_collateral_amount,
        } => open_position::opened(
            input,
            token_program_id,
            collateral_definition_id,
            collateral_holding,
            initial_collateral_amount,
        ),
        Internal::DebtGenerated {
            normalized_debt_delta,
        } => generate_debt::generated(input, normalized_debt_delta),
        Internal::DebtRepaid {
            normalized_debt_delta,
        } => repay_debt::repaid(input, normalized_debt_delta),
        Internal::AssertOracleReplied => oracle::assert_replied(input),
        Internal::WithdrawApproved { amount, to } => {
            withdraw_collateral::approved(input, amount, to)
        }
    }
}

/// A [`Message`] from anyone else.
fn handle_public(input: &ReceiveInput) -> Response {
    let message: Message =
        borsh::from_slice(&input.message).expect("stablecoin message must decode");
    match message {
        Message::Initialize {
            initial_admin_account_id,
            freeze_authority_account_id,
            token_program_id,
            collateral_definition_id,
            market_price_oracle_id,
            market_price_oracle_program_id,
            initial_stability_fee_per_millisecond,
            initial_controller_proportional_gain,
            initial_controller_integral_gain,
            initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
            initial_redemption_price,
            stablecoin_name,
            minimum_debt_amount,
            debt_ceiling,
        } => initialize::initialize(
            input,
            InitializeArgs {
                initial_admin_account_id,
                freeze_authority_account_id,
                token_program_id,
                collateral_definition_id,
                market_price_oracle_id,
                market_price_oracle_program_id,
                initial_stability_fee_per_millisecond,
                initial_controller_proportional_gain,
                initial_controller_integral_gain,
                initial_minimum_collateralization_ratio,
                minimum_milliseconds_between_rate_updates,
                maximum_oracle_price_age_milliseconds,
                initial_redemption_price,
                stablecoin_name,
                minimum_debt_amount,
                debt_ceiling,
            },
        ),
        Message::AccrueStabilityFee => accrue_stability_fee::accrue_stability_fee(input),
        Message::UpdateRedemptionRate => update_redemption_rate::update_redemption_rate(input),
        Message::RefreshGlobals { consult_oracle } => {
            refresh_globals::refresh_globals(input, consult_oracle)
        }
        Message::OpenPosition {
            collateral_holding,
            initial_collateral_amount,
        } => open_position::open_position(input, collateral_holding, initial_collateral_amount),
        Message::DepositCollateral {
            collateral_holding,
            amount,
        } => deposit_collateral::deposit_collateral(input, collateral_holding, amount),
        Message::WithdrawCollateral { to, amount } => {
            withdraw_collateral::withdraw_collateral(input, to, amount)
        }
        Message::GenerateDebt { to, amount } => generate_debt::generate_debt(input, to, amount),
        Message::RepayDebt {
            stablecoin_holding,
            maximum_amount,
        } => repay_debt::repay_debt(input, stablecoin_holding, maximum_amount),
        Message::ClosePosition => close_position::close_position(input),
        Message::SetMinimumCollateralizationRatio { new_ratio } => admin::forward(
            input,
            AdminAction::SetMinimumCollateralizationRatio { new_ratio },
        ),
        Message::SetControllerGains {
            new_proportional_gain,
            new_integral_gain,
        } => admin::forward(
            input,
            AdminAction::SetControllerGains {
                new_proportional_gain,
                new_integral_gain,
            },
        ),
        Message::SetMarketPriceOracle {
            oracle_account_id,
            oracle_program_id,
        } => admin::forward(
            input,
            AdminAction::SetMarketPriceOracle {
                oracle_account_id,
                oracle_program_id,
            },
        ),
        Message::SetTimingParameters {
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
        } => admin::forward(
            input,
            AdminAction::SetTimingParameters {
                minimum_milliseconds_between_rate_updates,
                maximum_oracle_price_age_milliseconds,
            },
        ),
        Message::SetAdmin {
            new_admin_account_id,
        } => admin::forward(
            input,
            AdminAction::SetAdmin {
                new_admin_account_id,
            },
        ),
        Message::SetFreezeAuthority {
            new_freeze_authority_account_id,
        } => admin::forward(
            input,
            AdminAction::SetFreezeAuthority {
                new_freeze_authority_account_id,
            },
        ),
        Message::SetStabilityFeePerMillisecond { new_rate } => admin::forward(
            input,
            AdminAction::SetStabilityFeePerMillisecond { new_rate },
        ),
        Message::Freeze => admin::forward(input, AdminAction::Freeze),
        Message::Unfreeze => admin::forward(input, AdminAction::Unfreeze),
        Message::ResetRedemptionRateController => {
            admin::forward(input, AdminAction::ResetRedemptionRateController)
        }
        Message::SetDebtLimits {
            minimum_debt_amount,
            debt_ceiling,
        } => admin::forward(
            input,
            AdminAction::SetDebtLimits {
                minimum_debt_amount,
                debt_ceiling,
            },
        ),
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

/// The sender of an [`Internal`] message; the router has checked it exists.
fn sender(input: &ReceiveInput) -> Actor {
    input
        .from
        .expect("an internal stablecoin message has a sender")
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
fn position_state(input: &ReceiveInput) -> Position {
    match StablecoinState::decode(&input.pre_state) {
        Some(StablecoinState::Position(position)) => position,
        Some(StablecoinState::Engine(_)) | None => panic!("this account has no position"),
    }
}

fn save_engine(engine: Engine) -> Response {
    Response::set_state(StablecoinState::Engine(engine).encode())
}

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
