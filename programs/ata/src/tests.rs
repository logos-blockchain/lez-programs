use ata_core::{AtaState, Holding, Internal, Message, TransferKind, TransferRequest};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply},
};
use token_core::{TokenHolding, TokenState};

use crate::handle_message;

const ATA: AccountId = AccountId::new([1; 32]);
const ASSET: AccountId = AccountId::new([2; 32]);
const OWNER: AccountId = AccountId::new([3; 32]);
const DEF: AccountId = AccountId::new([4; 32]);
const TO: AccountId = AccountId::new([5; 32]);

fn input(account: AccountId, message: &impl borsh::BorshSerialize) -> ReceiveInput {
    ReceiveInput {
        receiver: Actor::new(account, ATA),
        from: None,
        is_authorized: false,
        pre_state: Vec::new().into(),
        message: borsh::to_vec(message).unwrap(),
    }
}

fn request(amount: u128) -> TransferRequest {
    TransferRequest {
        owner: OWNER,
        asset_program: ASSET,
        definition: DEF,
        kind: TransferKind::Token { amount },
    }
}

fn source() -> AccountId {
    ata_core::get_associated_token_account_id(&ATA, &ata_core::compute_ata_seed(ASSET, OWNER, DEF))
}

fn start(amount: u128) -> Response {
    let mut call = input(TO, &Internal::Transfer(request(amount)));
    call.from = Some(Actor::new(OWNER, ATA));
    handle_message(&call)
}

fn reply(response: Response, sender: Actor, balance: u128) -> ReceiveInput {
    let mut call = input(
        TO,
        &StateReply {
            state: TokenState::Holding(TokenHolding {
                definition_id: DEF,
                balance,
            })
            .encode()
            .into(),
        },
    );
    call.pre_state = response.post_state.unwrap();
    call.from = Some(sender);
    call
}

#[test]
fn spending_always_needs_owner_authorization() {
    for message in [
        Message::Transfer {
            token_program_id: ASSET,
            definition: DEF,
            to: TO,
            amount: 1,
        },
        Message::TransferNft {
            nft_program_id: ASSET,
            definition: DEF,
            to: TO,
        },
        Message::Burn {
            token_program_id: ASSET,
            definition: DEF,
            amount: 1,
        },
        Message::BurnNft {
            nft_program_id: ASSET,
            definition: DEF,
            amount: 1,
        },
    ] {
        assert!(std::panic::catch_unwind(|| handle_message(&input(OWNER, &message))).is_err());
    }
}

#[test]
fn forwarding_captures_the_authorized_owner_and_grants_nothing_to_recipient() {
    let mut call = input(
        OWNER,
        &Message::Transfer {
            token_program_id: ASSET,
            definition: DEF,
            to: TO,
            amount: 7,
        },
    );
    call.is_authorized = true;
    // Authorization may come from another program's PDA grant, not only a root signature.
    call.from = Some(Actor::new(DEF, ASSET));
    let response = handle_message(&call);
    assert_eq!(
        response,
        Response::keep_state().send_sync(Actor::new(TO, ATA), &Internal::Transfer(request(7)))
    );
}

#[test]
fn create_is_permissionless_and_grants_only_the_derived_holding() {
    let response = handle_message(&input(
        TO,
        &Message::Create {
            token_program_id: ASSET,
            owner: OWNER,
            definition: DEF,
        },
    ));
    let [call] = response.sync_messages.as_slice() else {
        panic!("one initialization")
    };
    assert_eq!(call.to, Actor::new(source(), ASSET));
    assert_eq!(
        call.pda_seeds,
        [ata_core::compute_ata_seed(ASSET, OWNER, DEF)].into()
    );
    assert_eq!(
        borsh::from_slice::<token_core::Message>(&call.message).unwrap(),
        token_core::Message::InitializeHolding { definition: DEF }
    );
    assert!(response.post_state.is_none());
}

#[test]
fn an_external_program_cannot_forge_an_internal_transfer() {
    let mut call = input(TO, &Internal::Transfer(request(1)));
    call.from = Some(Actor::new(OWNER, ASSET));
    call.is_authorized = true;
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
}

#[test]
fn replies_authenticate_account_and_program_and_refuse_reentry() {
    for sender in [
        None,
        Some(Actor::new(source(), ATA)),
        Some(Actor::new(OWNER, ASSET)),
    ] {
        let mut call = reply(start(1), Actor::new(source(), ASSET), 100);
        call.from = sender;
        assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
    }
    let mut call = input(
        TO,
        &Message::Create {
            token_program_id: ASSET,
            owner: OWNER,
            definition: DEF,
        },
    );
    call.pre_state = start(1).post_state.unwrap();
    call.is_authorized = true;
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
}

#[test]
fn a_selected_asset_program_that_never_replies_cannot_commit_pending_state() {
    // A caller can select a nonconforming program that returns no state reply.
    let response = start(1);
    let [read, guard] = response.sync_messages.as_slice() else {
        panic!("read then completion guard")
    };
    assert_eq!(read.to, Actor::new(source(), ASSET));
    assert!(read.pda_seeds.is_empty());
    assert_eq!(guard.to, Actor::new(TO, ATA));
    let mut call = input(TO, &Internal::AssertIdle);
    call.pre_state = response.post_state.unwrap();
    call.from = Some(call.receiver);
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
    call.pre_state = Vec::new().into();
    assert_eq!(handle_message(&call), Response::keep_state());
    call.from = Some(Actor::new(OWNER, ATA));
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
}

fn transfer(amount: u128) -> Response {
    let response = handle_message(&reply(start(amount), Actor::new(source(), ASSET), 100));
    handle_message(&reply(response, Actor::new(TO, ASSET), 10))
}

#[test]
fn transfer_grants_only_source_and_verifies_after_the_asset_call() {
    let response = transfer(7);
    let [debit, verify] = response.sync_messages.as_slice() else {
        panic!("debit then verification")
    };
    assert_eq!(debit.to, Actor::new(source(), ASSET));
    assert_eq!(
        debit.pda_seeds,
        [ata_core::compute_ata_seed(ASSET, OWNER, DEF)].into()
    );
    assert_eq!(verify.to, Actor::new(TO, ASSET));
    assert!(verify.pda_seeds.is_empty());
    assert_eq!(
        AtaState::decode(response.post_state.as_ref().unwrap()),
        Some(AtaState::VerifyingRecipient {
            asset_program: ASSET,
            definition: DEF,
            expected: Holding::Token(17)
        })
    );
    assert_eq!(
        handle_message(&reply(response, Actor::new(TO, ASSET), 17)),
        Response::set_state(Vec::new())
    );
}

#[test]
fn deferred_or_incorrect_credit_cannot_complete() {
    for balance in [10, 16, 18] {
        let call = reply(transfer(7), Actor::new(TO, ASSET), balance);
        assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
    }
}

#[test]
fn overflowing_recipient_snapshot_is_rejected_before_the_transfer() {
    // A caller-selected asset may report holdings without the canonical token's
    // total-supply invariant. ATA must not wrap its expected settlement balance.
    let response = handle_message(&reply(start(1), Actor::new(source(), ASSET), 100));
    let call = reply(response, Actor::new(TO, ASSET), u128::MAX);
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
}

#[test]
fn zero_transfer_is_a_validated_noop_without_a_credit_or_event() {
    assert_eq!(transfer(0), Response::set_state(Vec::new()));
    let mut call = reply(start(0), Actor::new(source(), ASSET), 100);
    call.message = borsh::to_vec(&StateReply {
        state: Vec::new().into(),
    })
    .unwrap();
    assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
}

#[test]
fn seed_namespaces_and_deployed_program_addresses_are_distinct() {
    let other = AccountId::new([9; 32]);
    let seed = ata_core::compute_ata_seed(ASSET, OWNER, DEF);
    for different in [
        ata_core::compute_ata_seed(other, OWNER, DEF),
        ata_core::compute_ata_seed(ASSET, other, DEF),
        ata_core::compute_ata_seed(ASSET, OWNER, other),
    ] {
        assert_ne!(seed, different);
    }
    assert_ne!(
        ata_core::get_associated_token_account_id(&ATA, &seed),
        ata_core::get_associated_token_account_id(&other, &seed)
    );
}
