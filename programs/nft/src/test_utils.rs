//! Fixtures for the handler tests: one hand-built `ReceiveInput` per message, compared
//! against the exact expected `Response`. Conversations between actors are covered by the
//! integration tests (`programs/integration_tests/tests/nft.rs`).

pub(crate) use borsh::BorshSerialize;
pub(crate) use lee_core::{
    account::{AccountId, Actor, ActorState},
    program::{ProgramEvent, ReceiveInput, Response, StateReply},
};
pub(crate) use nft_core::{
    event::{Burned, NewDefinition, NftEvent, Printed, Transferred},
    Internal, Message, MetadataStandard, NftCopy, NftDefinition, NftMaster, NftMetadata, NftState,
    NftValue, MAX_CREATORS_LEN, MAX_NAME_LEN, MAX_URI_LEN,
};

use crate::handle_message;

pub(crate) const NFT: AccountId = AccountId::new([0xAA; 32]);
pub(crate) const OTHER_PROGRAM: AccountId = AccountId::new([0xBB; 32]);

pub(crate) const DEFINITION: AccountId = AccountId::new([1; 32]);
pub(crate) const OTHER_DEFINITION: AccountId = AccountId::new([2; 32]);
pub(crate) const MASTER: AccountId = AccountId::new([3; 32]);
pub(crate) const HOLDING: AccountId = AccountId::new([4; 32]);
pub(crate) const RECIPIENT: AccountId = AccountId::new([5; 32]);

pub(crate) fn nft(account_id: AccountId) -> Actor {
    Actor::new(account_id, NFT)
}

/// An actor of another program, e.g. ATA forwarding with a seed.
pub(crate) fn outsider(account_id: AccountId) -> Actor {
    Actor::new(account_id, OTHER_PROGRAM)
}

pub(crate) fn run(
    receiver: AccountId,
    from: Option<Actor>,
    is_authorized: bool,
    pre: Option<&NftState>,
    message: &impl BorshSerialize,
) -> Response {
    let input = ReceiveInput {
        receiver: nft(receiver),
        from,
        is_authorized,
        pre_state: ActorState::from(pre.map(NftState::encode).unwrap_or_default()),
        message: borsh::to_vec(message).expect("test message encodes"),
    };
    handle_message(&input)
}

pub(crate) fn metadata() -> NftMetadata {
    NftMetadata {
        standard: MetadataStandard::Simple,
        uri: "https://example.org/art.json".to_owned(),
        creators: "alice".to_owned(),
    }
}

pub(crate) fn definition(printable_supply: u128) -> NftState {
    NftState::Definition(NftDefinition {
        name: "Art".to_owned(),
        printable_supply,
        metadata: metadata(),
    })
}

pub(crate) fn master(definition_id: AccountId, print_balance: u128) -> NftState {
    NftState::Master(NftMaster {
        definition_id,
        print_balance,
    })
}

pub(crate) fn copy(definition_id: AccountId, owned: bool) -> NftState {
    NftState::Copy(NftCopy {
        definition_id,
        owned,
    })
}

pub(crate) fn write(state: &NftState) -> Response {
    Response::set_state(state.encode())
}
