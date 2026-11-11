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
        expected_account: expected.account_id,
        expected_program: expected.program_account_id,
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
        expected_account: expected.account_id,
        expected_program: expected.program_account_id,
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

#[test]
fn lp_definition_read_authenticates_account_and_program_without_a_pda_grant() {
    let (a, b) = (AccountId::new([5; 32]), AccountId::new([6; 32]));
    let config_id = amm_core::compute_config_pda(AMM, OWNER, [0; 32]);
    let pool_id = amm_core::compute_pool_pda(AMM, config_id, a, b);
    let lp = amm_core::compute_liquidity_token_pda(AMM, pool_id);
    let pool = amm_core::PoolDefinition {
        config_id,
        definition_token_a_id: a,
        definition_token_b_id: b,
        vault_a_id: amm_core::compute_vault_pda(AMM, pool_id, a),
        vault_b_id: amm_core::compute_vault_pda(AMM, pool_id, b),
        liquidity_pool_id: lp,
        liquidity_pool_supply: 10_000,
        reserve_a: 10_000,
        reserve_b: 10_000,
    };
    let mut call = input(
        Some(AmmState::Pool(pool)),
        &Message::AddLiquidity {
            user_holding_a: OWNER,
            user_holding_b: AccountId::new([7; 32]),
            user_holding_lp: AccountId::new([8; 32]),
            min_amount_liquidity: 1,
            max_amount_to_add_token_a: 100,
            max_amount_to_add_token_b: 100,
            deadline: u64::MAX,
        },
    );
    call.receiver = Actor::new(pool_id, AMM);
    let mut response = handle_message(&call);
    let mut snapshots = vec![AmmState::Config(config()).encode()];
    // The holding snapshots reflect a direct burn of 1,000 out of 10,000 LP;
    // the pool still caches the old supply until its definition reply arrives.
    snapshots.extend(
        [
            (a, 10_000),
            (b, 10_000),
            (lp, 8_000),
            (a, 10_000),
            (b, 10_000),
        ]
        .map(|(definition_id, balance)| {
            token_core::TokenState::Holding(token_core::TokenHolding {
                definition_id,
                balance,
            })
            .encode()
        }),
    );
    for state in snapshots {
        let [read] = response.sync_messages.as_slice() else {
            panic!("one outstanding read")
        };
        call.from = Some(read.to);
        call.pre_state = response.post_state.expect("pending read");
        call.message = borsh::to_vec(&StateReply {
            state: state.into(),
        })
        .expect("reply encodes");
        response = handle_message(&call);
    }
    let [read] = response.sync_messages.as_slice() else {
        panic!("LP definition read")
    };
    assert_eq!(read.to, Actor::new(lp, TOKEN));
    assert!(read.pda_seeds.is_empty());
    assert_eq!(
        borsh::from_slice::<token_core::Message>(&read.message).expect("token request"),
        token_core::Message::ReadState
    );
    call.pre_state = response.post_state.expect("pending LP supply");
    assert!(matches!(
        AmmState::decode(&call.pre_state),
        Some(AmmState::ReadingLpSupply(_))
    ));
    call.message = borsh::to_vec(&StateReply {
        state: token_core::TokenState::Definition(token_core::TokenDefinition {
            name: "LP Token".to_owned(),
            total_supply: 9_000,
            authority: Some(lp),
        })
        .encode()
        .into(),
    })
    .expect("definition reply encodes");
    for from in [
        None,
        Some(Actor::new(lp, ORACLE)),
        Some(Actor::new(OWNER, TOKEN)),
    ] {
        call.from = from;
        assert!(std::panic::catch_unwind(|| handle_message(&call)).is_err());
    }
}
