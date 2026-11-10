//! Constant-product AMM on LEZ's actor interface.
//!
//! Config and pool actors own their state. A pool reads the config and token holdings
//! through synchronous, sender-authenticated `StateReply` conversations. It then emits
//! token messages with narrowly scoped PDA grants. Declaring each vault/destination via
//! a synchronous read or initialization ensures token credits settle atomically.
//!
//! Pending reads and settlement lock the actor against reentrancy. A refusal anywhere
//! rolls back the transaction, including the lock, transfers and oracle notifications.

pub use amm_core as core;
use amm_core::{AmmState, Internal, Message};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply},
};

mod add;
mod config;
mod new_definition;
mod reads;
mod remove;
mod settlement;
mod swap;
mod sync;
#[cfg(test)]
mod tests;

/// Dispatch one actor message, authenticating internal messages and outstanding replies.
pub fn handle_message(input: &ReceiveInput) -> Response {
    let state = AmmState::decode(&input.pre_state);
    if let Some(AmmState::Pending(pending)) = state {
        return reads::reply(input, *pending);
    }
    if let Some(AmmState::Settling(next)) = state {
        assert_eq!(
            input.from,
            Some(input.receiver),
            "AMM operation is settling"
        );
        let message: Internal =
            borsh::from_slice(&input.message).expect("AMM internal message must decode");
        assert_eq!(
            message,
            Internal::Complete,
            "expected settlement completion"
        );
        return Response::set_state(next.encode());
    }
    if input.from_own_program() {
        let message: Internal =
            borsh::from_slice(&input.message).expect("AMM internal message must decode");
        return match message {
            Internal::ReadState => read_state(input),
            Internal::Complete => panic!("no settlement in progress"),
            Internal::NewPool { request } => {
                let Message::NewDefinition { config, .. } = *request else {
                    panic!("expected pool creation")
                };
                assert_eq!(
                    input.from,
                    Some(actor(input, config)),
                    "pool creation must come from its config"
                );
                reads::start(input, state, *request)
            }
            message @ (Internal::InitializeConfig { .. }
            | Internal::UpdateConfig { .. }
            | Internal::WithdrawProtocolFees { .. }) => config::internal(input, state, message),
        };
    }
    let message: Message = borsh::from_slice(&input.message).expect("AMM message must decode");
    match message {
        Message::ReadState => read_state(input),
        message @ Message::NewDefinition { .. } => new_definition::forward(input, state, message),
        Message::Initialize { .. }
        | Message::UpdateConfig { .. }
        | Message::WithdrawProtocolFees { .. } => config::forward(input, message),
        message @ (Message::AddLiquidity { .. }
        | Message::RemoveLiquidity { .. }
        | Message::SwapExactInput { .. }
        | Message::SwapExactOutput { .. }
        | Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. }) => reads::start(input, state, message),
    }
}

fn actor(input: &ReceiveInput, account_id: AccountId) -> Actor {
    Actor::new(account_id, input.receiver.program_account_id)
}

fn read_state(input: &ReceiveInput) -> Response {
    let reader = input.from.expect("ReadState needs a sender to reply to");
    Response::keep_state().send_sync(reader, &StateReply::from(input))
}
