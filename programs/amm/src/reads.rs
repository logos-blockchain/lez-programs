use amm_core::{AmmState, Internal, Message, Pending, PoolDefinition, MINIMUM_LIQUIDITY};
use lee_core::{
    account::{AccountId, Actor},
    program::{ReceiveInput, Response, StateReply},
};
use token_core::{TokenHolding, TokenState};

use crate::{actor, add, config, new_definition, remove, swap, sync};

pub(crate) fn start(input: &ReceiveInput, state: Option<AmmState>, request: Message) -> Response {
    let (config_id, pool) = if let Message::NewDefinition { config, .. } = &request {
        assert!(state.is_none(), "pool already exists");
        (*config, None)
    } else {
        let Some(AmmState::Pool(pool)) = state else {
            panic!("receiver is not an AMM pool")
        };
        (pool.config_id, Some(pool))
    };
    let deadline = match &request {
        Message::NewDefinition { deadline, .. }
        | Message::AddLiquidity { deadline, .. }
        | Message::RemoveLiquidity { deadline, .. }
        | Message::SwapExactInput { deadline, .. }
        | Message::SwapExactOutput { deadline, .. } => Some(*deadline),
        Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. } => None,
        Message::Initialize { .. }
        | Message::UpdateConfig { .. }
        | Message::WithdrawProtocolFees { .. }
        | Message::ReadState => unreachable!("pool operation"),
    };
    let expected = actor(input, config_id);
    let pending = Pending {
        config_id,
        config: None,
        pool,
        request,
        expected_account: expected.account_id,
        expected_program: expected.program_account_id,
        remaining: vec![],
        holdings: vec![],
    };
    let response = Response::set_state(AmmState::Pending(Box::new(pending)).encode())
        .send_sync(expected, &Internal::ReadState);
    // The former clock check accepted timestamp == deadline. An unbounded upper end
    // represents u64::MAX without overflowing its exclusive endpoint.
    match deadline.and_then(|value| value.checked_add(1)) {
        Some(end) => response.timestamp_window(..end),
        None => response,
    }
}

pub(crate) fn reply(input: &ReceiveInput, mut pending: Pending) -> Response {
    assert_eq!(
        input.from,
        Some(Actor::new(
            pending.expected_account,
            pending.expected_program
        )),
        "unexpected AMM state reply sender"
    );
    let reply: StateReply = borsh::from_slice(&input.message).expect("state reply must decode");
    if pending.config.is_none() {
        let Some(AmmState::Config(config)) = AmmState::decode(&reply.state) else {
            panic!("AMM config must be initialized")
        };
        pending.config = Some(config);
        let mut accounts = match &pending.request {
            Message::NewDefinition {
                user_holding_a,
                user_holding_b,
                ..
            } => vec![*user_holding_a, *user_holding_b],
            Message::AddLiquidity {
                user_holding_a,
                user_holding_b,
                user_holding_lp,
                ..
            }
            | Message::RemoveLiquidity {
                user_holding_a,
                user_holding_b,
                user_holding_lp,
                ..
            } => vec![*user_holding_a, *user_holding_b, *user_holding_lp],
            Message::SwapExactInput {
                user_input_holding,
                user_output_holding,
                ..
            }
            | Message::SwapExactOutput {
                user_input_holding,
                user_output_holding,
                ..
            } => vec![*user_input_holding, *user_output_holding],
            Message::SyncReserves
            | Message::CreatePriceObservations { .. }
            | Message::CreateOraclePriceAccount { .. } => vec![],
            Message::Initialize { .. }
            | Message::UpdateConfig { .. }
            | Message::WithdrawProtocolFees { .. }
            | Message::ReadState => unreachable!("pool operation"),
        };
        if let Some(pool) = &pending.pool {
            assert!(
                pool.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
                "pool liquidity is below minimum"
            );
            accounts.extend([pool.vault_a_id, pool.vault_b_id]);
        }
        return holdings(input, pending, accounts);
    }
    let holding = match TokenState::decode(&reply.state) {
        Some(TokenState::Holding(holding)) => Some(holding),
        None => None,
        Some(TokenState::Definition(_)) => panic!("expected a token holding"),
    };
    pending.holdings.push(holding);
    next(input, pending)
}

pub(crate) fn holdings(
    input: &ReceiveInput,
    mut pending: Pending,
    mut accounts: Vec<AccountId>,
) -> Response {
    accounts.reverse();
    pending.remaining = accounts;
    next(input, pending)
}

fn next(input: &ReceiveInput, mut pending: Pending) -> Response {
    if let Some(account) = pending.remaining.pop() {
        let token = pending
            .config
            .as_ref()
            .expect("config was read")
            .token_program_id;
        let expected = Actor::new(account, token);
        pending.expected_account = expected.account_id;
        pending.expected_program = expected.program_account_id;
        return Response::set_state(AmmState::Pending(Box::new(pending)).encode())
            .send_sync(expected, &token_core::Message::ReadState);
    }
    if let Some(pool) = &pending.pool {
        let [.., a, b] = pending.holdings.as_slice() else {
            unreachable!("vaults were read")
        };
        assert!(
            holding(a, pool.definition_token_a_id).balance >= pool.reserve_a,
            "vault A is below reserve"
        );
        assert!(
            holding(b, pool.definition_token_b_id).balance >= pool.reserve_b,
            "vault B is below reserve"
        );
    }
    match pending.request {
        Message::NewDefinition { .. } => new_definition::execute(input, &pending),
        Message::AddLiquidity { .. } => add::execute(input, &pending),
        Message::RemoveLiquidity { .. } => remove::execute(input, &pending),
        Message::SwapExactInput { .. } | Message::SwapExactOutput { .. } => {
            swap::execute(input, &pending)
        }
        Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. } => sync::execute(input, &pending),
        Message::WithdrawProtocolFees { .. } => config::withdraw(input, &pending),
        Message::Initialize { .. } | Message::UpdateConfig { .. } | Message::ReadState => {
            unreachable!("operation continuation")
        }
    }
}

pub(crate) fn holding(snapshot: &Option<TokenHolding>, definition: AccountId) -> &TokenHolding {
    let holding = snapshot
        .as_ref()
        .expect("token holding must be initialized");
    assert_eq!(
        holding.definition_id, definition,
        "holding has the wrong token definition"
    );
    holding
}

pub(crate) fn pool(pending: &Pending) -> PoolDefinition {
    pending.pool.clone().expect("pool was loaded")
}
