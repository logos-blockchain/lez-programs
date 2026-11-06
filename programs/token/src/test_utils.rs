//! Fixtures for the handler tests: one hand-built `ReceiveInput` per message, compared
//! against the exact expected `Response`. Conversations between actors are covered by the
//! integration tests (`programs/integration_tests/tests/token.rs`).

pub(crate) use borsh::BorshSerialize;
pub(crate) use lee_core::{
    account::{AccountId, Actor, ActorState},
    program::{ProgramEvent, ReceiveInput, Response, StateReply},
};
pub(crate) use token_core::{
    event::{AuthorityChanged, Burned, Minted, NewDefinition, TokenEvent, Transferred},
    Internal, Message, TokenDefinition, TokenHolding, TokenState, MAX_NAME_LEN,
};

use crate::handle_message;

pub(crate) const TOKEN: AccountId = AccountId::new([0xAA; 32]);
pub(crate) const OTHER_PROGRAM: AccountId = AccountId::new([0xBB; 32]);

pub(crate) const DEFINITION: AccountId = AccountId::new([1; 32]);
pub(crate) const OTHER_DEFINITION: AccountId = AccountId::new([2; 32]);
pub(crate) const HOLDING: AccountId = AccountId::new([3; 32]);
pub(crate) const RECIPIENT: AccountId = AccountId::new([4; 32]);
pub(crate) const AUTHORITY: AccountId = AccountId::new([5; 32]);
pub(crate) const NEW_AUTHORITY: AccountId = AccountId::new([6; 32]);
pub(crate) const ZERO: AccountId = AccountId::new([0; 32]);

pub(crate) fn token(account_id: AccountId) -> Actor {
    Actor::new(account_id, TOKEN)
}

/// An actor of another program, e.g. ATA or the AMM forwarding with a seed.
pub(crate) fn outsider(account_id: AccountId) -> Actor {
    Actor::new(account_id, OTHER_PROGRAM)
}

pub(crate) fn run(
    receiver: AccountId,
    from: Option<Actor>,
    is_authorized: bool,
    pre: Option<&TokenState>,
    message: &impl BorshSerialize,
) -> Response {
    let input = ReceiveInput {
        receiver: token(receiver),
        from,
        is_authorized,
        pre_state: ActorState::from(pre.map(TokenState::encode).unwrap_or_default()),
        message: borsh::to_vec(message).expect("test message encodes"),
    };
    handle_message(&input)
}

pub(crate) fn definition(total_supply: u128, authority: Option<AccountId>) -> TokenState {
    TokenState::Definition(TokenDefinition {
        name: "Test".to_owned(),
        total_supply,
        authority,
    })
}

pub(crate) fn holding(definition_id: AccountId, balance: u128) -> TokenState {
    TokenState::Holding(TokenHolding {
        definition_id,
        balance,
    })
}

pub(crate) fn write(state: &TokenState) -> Response {
    Response::set_state(state.encode())
}
