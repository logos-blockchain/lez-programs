use amm_core::{AmmConfig, AmmState, Internal, Message, Pending};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, StateReply},
};

use crate::handle_message;

const AMM: AccountId = AccountId::new([1; 32]);
const TOKEN: AccountId = AccountId::new([2; 32]);
const OWNER: AccountId = AccountId::new([3; 32]);
const ORACLE: AccountId = AccountId::new([4; 32]);

fn config() -> AmmConfig {
    AmmConfig {
        token_program_id: TOKEN,
        twap_oracle_program_id: ORACLE,
        authority: OWNER,
        swap_fee_bps: 30,
        protocol_fee_bps: 1_000,
    }
}

fn input(state: Option<AmmState>, message: &impl borsh::BorshSerialize) -> ReceiveInput {
    ReceiveInput {
        receiver: Actor::new(OWNER, AMM),
        from: None,
        is_authorized: false,
        pre_state: state.map(|s| s.encode()).unwrap_or_default().into(),
        message: borsh::to_vec(message).expect("message encodes"),
    }
}

#[test]
#[should_panic(expected = "AMM owner or authority must authorize")]
fn namespace_creation_requires_authorization() {
    let _ = handle_message(&input(
        None,
        &Message::Initialize {
            nonce: [0; 32],
            config: config(),
        },
    ));
}

#[test]
#[should_panic(expected = "unexpected AMM state reply sender")]
fn state_reply_from_another_deployment_is_refused() {
    let expected = Actor::new(OWNER, TOKEN);
    let state = AmmState::Pending(Box::new(Pending {
        config_id: OWNER,
        config: Some(config()),
        pool: None,
        request: Message::SyncReserves,
        expected,
        remaining: vec![],
        holdings: vec![],
    }));
    let mut call = input(
        Some(state),
        &StateReply {
            state: Vec::new().into(),
        },
    );
    call.from = Some(Actor::new(OWNER, ORACLE));
    let _ = handle_message(&call);
}

#[test]
#[should_panic(expected = "unexpected AMM state reply sender")]
fn transaction_author_cannot_forge_state_reply() {
    let expected = Actor::new(OWNER, TOKEN);
    let state = AmmState::Pending(Box::new(Pending {
        config_id: OWNER,
        config: Some(config()),
        pool: None,
        request: Message::SyncReserves,
        expected,
        remaining: vec![],
        holdings: vec![],
    }));
    let _ = handle_message(&input(
        Some(state),
        &StateReply {
            state: Vec::new().into(),
        },
    ));
}

#[test]
#[should_panic(expected = "AMM operation is settling")]
fn external_program_cannot_reenter_a_settling_actor() {
    let state = AmmState::Settling(Box::new(AmmState::Config(config())));
    let mut call = input(
        Some(state),
        &Message::UpdateConfig {
            config: OWNER,
            new_authority: ORACLE,
        },
    );
    call.from = Some(Actor::new(OWNER, ORACLE));
    call.is_authorized = true;
    let _ = handle_message(&call);
}

#[test]
fn only_self_completion_restores_stable_state() {
    let stable = AmmState::Config(config());
    let mut call = input(
        Some(AmmState::Settling(Box::new(stable.clone()))),
        &Internal::Complete,
    );
    call.from = Some(call.receiver);
    let response = handle_message(&call);
    assert_eq!(
        AmmState::decode(&response.post_state.expect("state restored")),
        Some(stable)
    );
}

#[test]
fn deadline_is_inclusive_and_maximum_does_not_overflow() {
    for deadline in [0, 17, u64::MAX] {
        let request = Message::NewDefinition {
            config: OWNER,
            definition_token_a_id: OWNER,
            definition_token_b_id: TOKEN,
            user_holding_a: OWNER,
            user_holding_b: TOKEN,
            user_holding_lp: ORACLE,
            token_a_amount: 10_000,
            token_b_amount: 10_000,
            deadline,
        };
        let response = crate::reads::start(&input(None, &request), None, request);
        assert!(response.validity.timestamps.is_valid_for(deadline));
        if let Some(after) = deadline.checked_add(1) {
            assert!(!response.validity.timestamps.is_valid_for(after));
        }
    }
}
