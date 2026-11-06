//! The fungible Token Program on LEZ's actor interface.
//!
//! A handler sees one actor: a definition, a holding, or an empty one. Operations that
//! touch several (transfer, mint, burn, initialization) are short conversations between
//! token actors; each operation's module holds all of its steps. Design:
//! `docs/actor-model/token.md`.
//!
//! Supply conservation rests on [`Internal`] messages being decoded only when a token
//! actor sent them, and on every token code path that emits one upholding the invariant
//! (`token.md` §3.3):
//!
//! - a holding sends `IncreaseBalance` only for its own definition, after debiting the same amount;
//! - a definition sends `IncreaseBalance` only for itself, after raising its supply by the amount;
//! - a holding sends `DecreaseSupply` only to its own definition, after debiting the amount;
//! - a definition sends `InitHolding` with its supply only at creation, otherwise with 0 and only
//!   to the holding that asked (`RequestHolding`).
//!
//! Owner operations check `is_authorized` and nothing about the sender: a signature, an ATA
//! seed grant and an AMM vault seed all look the same here (`token.md` §3.4).

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
pub use token_core as core;
use token_core::{Internal, Message, TokenDefinition, TokenHolding, TokenState};

mod burn;
mod initialize_holding;
mod mint;
mod new_definition;
mod read_state;
mod set_authority;
#[cfg(test)]
mod test_utils;
mod transfer;

/// The guest entry point: [`Internal`] messages from token actors, [`Message`]s otherwise.
pub fn handle_message(input: &ReceiveInput) -> Response {
    if input.from_own_program() {
        let from = input.from.expect("an internal message always has a sender");
        let message: Internal =
            borsh::from_slice(&input.message).expect("internal token message must decode");
        match message {
            Internal::IncreaseBalance { definition, amount } => {
                transfer::increase_balance(input, from, definition, amount)
            }
            Internal::RequestHolding => initialize_holding::request_holding(input, from),
            Internal::InitHolding { definition, amount } => {
                initialize_holding::init_holding(input, from, definition, amount)
            }
            Internal::DecreaseSupply { amount } => burn::decrease_supply(input, from, amount),
            Internal::IncreaseSupply { to, amount } => {
                mint::increase_supply(input, from, to, amount)
            }
            Internal::ChangeAuthority { new_authority } => {
                set_authority::change_authority(input, from, new_authority)
            }
        }
    } else {
        let message: Message =
            borsh::from_slice(&input.message).expect("token message must decode");
        match message {
            Message::NewDefinition {
                name,
                total_supply,
                mint_authority,
                holding,
            } => new_definition::new_definition(input, name, total_supply, mint_authority, holding),
            Message::InitializeHolding { definition } => {
                initialize_holding::initialize_holding(input, definition)
            }
            Message::Transfer { to, amount } => transfer::transfer(input, to, amount),
            Message::Burn { amount } => burn::burn(input, amount),
            Message::Mint {
                definition,
                to,
                amount,
            } => mint::mint(input, definition, to, amount),
            Message::SetAuthority {
                definition,
                new_authority,
            } => set_authority::set_authority(input, definition, new_authority),
            Message::ReadState => read_state::read_state(input),
        }
    }
}

/// The token program's actor on `account_id`.
fn token_actor(input: &ReceiveInput, account_id: AccountId) -> Actor {
    Actor::new(account_id, input.receiver.program_account_id)
}

fn definition_state(input: &ReceiveInput) -> TokenDefinition {
    match TokenState::decode(&input.pre_state) {
        Some(TokenState::Definition(definition)) => definition,
        Some(TokenState::Holding(_)) | None => panic!("receiver is not a token definition"),
    }
}

fn holding_state(input: &ReceiveInput) -> TokenHolding {
    match TokenState::decode(&input.pre_state) {
        Some(TokenState::Holding(holding)) => holding,
        Some(TokenState::Definition(_)) | None => panic!("receiver is not a token holding"),
    }
}

/// Panics unless `who` is the definition's current mint authority.
fn assert_authority(definition: &TokenDefinition, who: AccountId) {
    let authority = definition.authority.expect("mint authority is renounced");
    assert!(authority == who, "sender is not the mint authority");
}

/// Panics if `authority` is the zero account id, which no one controls.
fn assert_not_zero(authority: Option<AccountId>) {
    assert!(
        authority != Some(AccountId::default()),
        "mint authority must not be the zero account id"
    );
}

/// The authority speaks through its own token actor, whose state is left untouched; the
/// definition checks that the sender is its authority (`token.md` §3.6).
fn forward_as_authority(
    input: &ReceiveInput,
    definition: AccountId,
    message: &Internal,
) -> Response {
    assert!(input.is_authorized, "mint authority must authorize");
    Response::keep_state().send_sync(token_actor(input, definition), message)
}
