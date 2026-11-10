//! The NFT Program on LEZ's actor interface.
//!
//! A handler sees one actor: a definition, a master, a copy, or an empty one. Operations
//! that touch several (creation, initialization, transfer, print, burn) are short
//! conversations between NFT actors; each operation's module holds all of its steps.
//! Design: `docs/actor-model/nft.md`.
//!
//! The supply invariant, `printable_supply == Σ master.print_balance + count(owned
//! copies)`, rests on [`Internal`] messages being decoded only when an NFT actor sent them,
//! and on every NFT code path that emits one upholding it (`nft.md` §6):
//!
//! - a definition sends `InitMaster` only at creation, with its whole supply;
//! - a definition sends `InitHolding` only to the holding that asked, and it holds nothing;
//! - a holding sends `Receive` only for its own definition, after giving up the same value;
//! - a master sends `ReceivePrint` only for its own definition, after lowering its print balance by
//!   one;
//! - a holding sends `DecreaseSupply` only to its own definition, after burning the amount.
//!
//! Owner operations check `is_authorized` and nothing about the sender, as in the token
//! program: a signature and a seed grant look the same here.

use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response},
};
pub use nft_core as core;
use nft_core::{Internal, Message, NftDefinition, NftState};

mod burn;
mod initialize_holding;
mod new_definition;
mod print;
mod read_state;
#[cfg(test)]
mod test_utils;
mod transfer;

/// The guest entry point: [`Internal`] messages from NFT actors, [`Message`]s otherwise.
pub fn handle_message(input: &ReceiveInput) -> Response {
    if input.from_own_program() {
        let from = input.from.expect("an internal message always has a sender");
        let message: Internal =
            borsh::from_slice(&input.message).expect("internal nft message must decode");
        match message {
            Internal::InitMaster {
                definition,
                print_balance,
            } => new_definition::init_master(input, from, definition, print_balance),
            Internal::RequestHolding => initialize_holding::request_holding(input, from),
            Internal::InitHolding { definition } => {
                initialize_holding::init_holding(input, from, definition)
            }
            Internal::Receive { definition, value } => {
                transfer::receive(input, from, definition, value)
            }
            Internal::ReceivePrint { definition } => print::receive_print(input, from, definition),
            Internal::DecreaseSupply { amount } => burn::decrease_supply(input, from, amount),
        }
    } else {
        let message: Message = borsh::from_slice(&input.message).expect("nft message must decode");
        match message {
            Message::NewDefinition {
                name,
                printable_supply,
                metadata,
                master,
            } => new_definition::new_definition(input, name, printable_supply, metadata, master),
            Message::InitializeHolding { definition } => {
                initialize_holding::initialize_holding(input, definition)
            }
            Message::Transfer { to } => transfer::transfer(input, to),
            Message::PrintNft { to } => print::print_nft(input, to),
            Message::Burn { amount } => burn::burn(input, amount),
            Message::ReadState => read_state::read_state(input),
        }
    }
}

/// The NFT program's actor on `account_id`.
fn nft_actor(input: &ReceiveInput, account_id: AccountId) -> Actor {
    Actor::new(account_id, input.receiver.program_account_id)
}

fn definition_state(input: &ReceiveInput) -> NftDefinition {
    match NftState::decode(&input.pre_state) {
        Some(NftState::Definition(definition)) => definition,
        Some(NftState::Master(_) | NftState::Copy(_)) | None => {
            panic!("receiver is not an NFT definition")
        }
    }
}

/// Panics unless the receiver is a holding of `definition` that holds nothing. Shared by the
/// two credits, `Receive` and `ReceivePrint`: either value lands only on such a holding
/// (`nft.md` §2.2), so nobody can occupy an account's NFT actor with an NFT its owner didn't
/// choose.
fn assert_holds_nothing_of(input: &ReceiveInput, definition: AccountId) {
    let (definition_id, holds_nothing) = match NftState::decode(&input.pre_state) {
        Some(NftState::Master(master)) => (master.definition_id, master.print_balance == 0),
        Some(NftState::Copy(copy)) => (copy.definition_id, !copy.owned),
        None => panic!("recipient has no NFT holding; initialize it first"),
        Some(NftState::Definition(_)) => panic!("recipient is an NFT definition"),
    };
    assert!(definition_id == definition, "recipient holds another NFT");
    assert!(holds_nothing, "recipient already holds an NFT");
}
