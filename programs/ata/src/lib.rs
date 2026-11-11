//! ATA authorization and atomic transfers over the existing token/NFT actor APIs.
//!
//! Owner actors authorize spending. Recipient actors coordinate authenticated state
//! reads and verify settlement, so private balances stay in private execution. Asset
//! programs retain balance, supply and event logic. Only the derived source holding
//! receives a PDA grant; replies and recipient reads receive no grants.

pub use ata_core as core;
use ata_core::{AtaState, Internal, Message, TransferKind, TransferRequest};
use borsh::BorshSerialize;
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, SyncMessage},
};

#[cfg(test)]
mod tests;
mod transfer;

pub fn handle_message(input: &ReceiveInput) -> Response {
    let state = AtaState::decode(&input.pre_state);
    if input.from == Some(input.receiver)
        && matches!(
            borsh::from_slice::<Internal>(&input.message),
            Ok(Internal::AssertIdle)
        )
    {
        assert!(state.is_none(), "ATA transfer did not complete");
        return Response::keep_state();
    }
    if let Some(state) = state {
        return transfer::reply(input, state);
    }
    if input.from_own_program() {
        return match borsh::from_slice(&input.message).expect("ATA internal message must decode") {
            Internal::Transfer(request) => transfer::start(input, request),
            Internal::AssertIdle => panic!("only self may assert ATA completion"),
        };
    }
    match borsh::from_slice(&input.message).expect("ATA message must decode") {
        Message::Create {
            token_program_id,
            owner,
            definition,
        } => Response::keep_state().send(holding_call(
            input,
            owner,
            token_program_id,
            definition,
            &token_core::Message::InitializeHolding { definition },
        )),
        Message::CreateNft {
            nft_program_id,
            owner,
            definition,
        } => Response::keep_state().send(holding_call(
            input,
            owner,
            nft_program_id,
            definition,
            &nft_core::Message::InitializeHolding { definition },
        )),
        Message::Burn {
            token_program_id,
            definition,
            amount,
        } => {
            authorize(input);
            Response::keep_state().send(holding_call(
                input,
                input.receiver.account_id,
                token_program_id,
                definition,
                &token_core::Message::Burn { amount },
            ))
        }
        Message::BurnNft {
            nft_program_id,
            definition,
            amount,
        } => {
            authorize(input);
            Response::keep_state().send(holding_call(
                input,
                input.receiver.account_id,
                nft_program_id,
                definition,
                &nft_core::Message::Burn { amount },
            ))
        }
        Message::Transfer {
            token_program_id,
            definition,
            to,
            amount,
        } => forward(
            input,
            to,
            token_program_id,
            definition,
            TransferKind::Token { amount },
        ),
        Message::TransferNft {
            nft_program_id,
            definition,
            to,
        } => forward(input, to, nft_program_id, definition, TransferKind::Nft),
    }
}

fn authorize(input: &ReceiveInput) {
    assert!(input.is_authorized, "ATA owner must authorize spending");
}

fn forward(
    input: &ReceiveInput,
    to: AccountId,
    asset_program: AccountId,
    definition: AccountId,
    kind: TransferKind,
) -> Response {
    authorize(input);
    Response::keep_state().send_sync(
        Actor::new(to, input.receiver.program_account_id),
        &Internal::Transfer(TransferRequest {
            owner: input.receiver.account_id,
            asset_program,
            definition,
            kind,
        }),
    )
}

fn holding_call(
    input: &ReceiveInput,
    owner: AccountId,
    program: AccountId,
    definition: AccountId,
    message: &impl BorshSerialize,
) -> SyncMessage {
    let seed = ata_core::compute_ata_seed(program, owner, definition);
    let holding =
        ata_core::get_associated_token_account_id(&input.receiver.program_account_id, &seed);
    SyncMessage::new(Actor::new(holding, program), message).with_pda_seeds([seed])
}
