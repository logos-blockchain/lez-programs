use amm_core::{
    compute_liquidity_token_pda, compute_liquidity_token_pda_seed, compute_lp_lock_holding_pda,
    compute_lp_lock_holding_pda_seed, compute_pool_pda, compute_pool_pda_seed, compute_vault_pda,
    compute_vault_pda_seed, event::Event, isqrt_product, oracle, spot_price_q64_64, AmmState,
    Internal, Message, Pending, PoolDefinition, MINIMUM_LIQUIDITY,
};
use lee_core::program::{ReceiveInput, Response, SyncMessage};
use token_core::Message as TokenMessage;

use crate::settlement::{finish, oracle_call, token_call};

pub(crate) fn forward(input: &ReceiveInput, state: Option<AmmState>, request: Message) -> Response {
    assert!(
        matches!(state, Some(AmmState::Config(_))),
        "pool creation goes to an initialized config"
    );
    let Message::NewDefinition {
        config,
        definition_token_a_id: a,
        definition_token_b_id: b,
        ..
    } = request
    else {
        unreachable!("pool creation")
    };
    assert_eq!(
        config, input.receiver.account_id,
        "pool config does not match receiver"
    );
    let pool = compute_pool_pda(input.receiver.program_account_id, config, a, b);
    Response::keep_state().send(
        SyncMessage::new(
            crate::actor(input, pool),
            &Internal::NewPool {
                request: Box::new(request),
            },
        )
        .with_pda_seeds([compute_pool_pda_seed(config, a, b)]),
    )
}

pub(crate) fn execute(input: &ReceiveInput, pending: &Pending) -> Response {
    let Message::NewDefinition {
        user_holding_a,
        user_holding_b,
        user_holding_lp,
        token_a_amount,
        token_b_amount,
        definition_token_a_id,
        definition_token_b_id,
        ..
    } = pending.request
    else {
        unreachable!("new pool continuation")
    };
    let [a, b] = pending.holdings.as_slice() else {
        unreachable!("two holdings read")
    };
    let a = a.as_ref().expect("token A holding must exist");
    let b = b.as_ref().expect("token B holding must exist");
    assert_eq!(
        a.definition_id, definition_token_a_id,
        "token A definition mismatch"
    );
    assert_eq!(
        b.definition_id, definition_token_b_id,
        "token B definition mismatch"
    );
    assert_ne!(
        a.definition_id, b.definition_id,
        "cannot pool a token with itself"
    );
    assert!(
        token_a_amount != 0 && token_b_amount != 0,
        "initial amounts must be nonzero"
    );
    assert!(
        a.balance >= token_a_amount && b.balance >= token_b_amount,
        "insufficient initial balances"
    );
    let program = input.receiver.program_account_id;
    let pool_id = input.receiver.account_id;
    assert_eq!(
        pool_id,
        compute_pool_pda(program, pending.config_id, a.definition_id, b.definition_id),
        "pool does not match config and token pair"
    );
    let supply = isqrt_product(token_a_amount, token_b_amount);
    assert!(
        supply > MINIMUM_LIQUIDITY,
        "initial liquidity must exceed minimum liquidity lock"
    );
    let pool = PoolDefinition {
        config_id: pending.config_id,
        definition_token_a_id: a.definition_id,
        definition_token_b_id: b.definition_id,
        vault_a_id: compute_vault_pda(program, pool_id, a.definition_id),
        vault_b_id: compute_vault_pda(program, pool_id, b.definition_id),
        liquidity_pool_id: compute_liquidity_token_pda(program, pool_id),
        liquidity_pool_supply: supply,
        reserve_a: token_a_amount,
        reserve_b: token_b_amount,
    };
    let config = pending.config.as_ref().expect("config was read");
    let lock = compute_lp_lock_holding_pda(program, pool_id);
    let mut response = Response::keep_state();
    for (vault, definition) in [
        (pool.vault_a_id, a.definition_id),
        (pool.vault_b_id, b.definition_id),
    ] {
        response = response.send(token_call(
            config,
            vault,
            &TokenMessage::InitializeHolding { definition },
            Some(compute_vault_pda_seed(pool_id, definition)),
        ));
    }
    // The initial LP holding signs the definition's InitHolding and the transfer into the
    // permanent lock. No lock seed is ever granted to a debit operation.
    response = response
        .send(token_call(
            config,
            pool.liquidity_pool_id,
            &TokenMessage::NewDefinition {
                name: "LP Token".to_owned(),
                total_supply: supply,
                mint_authority: Some(pool.liquidity_pool_id),
                holding: user_holding_lp,
            },
            Some(compute_liquidity_token_pda_seed(pool_id)),
        ))
        .send(token_call(
            config,
            lock,
            &TokenMessage::InitializeHolding {
                definition: pool.liquidity_pool_id,
            },
            Some(compute_lp_lock_holding_pda_seed(pool_id)),
        ))
        .send(token_call(
            config,
            user_holding_lp,
            &TokenMessage::Transfer {
                to: lock,
                amount: MINIMUM_LIQUIDITY,
            },
            None,
        ))
        .send(token_call(
            config,
            user_holding_a,
            &TokenMessage::Transfer {
                to: pool.vault_a_id,
                amount: token_a_amount,
            },
            None,
        ))
        .send(token_call(
            config,
            user_holding_b,
            &TokenMessage::Transfer {
                to: pool.vault_b_id,
                amount: token_b_amount,
            },
            None,
        ))
        .send(oracle_call(
            input,
            config,
            &pool,
            &oracle::Message::CreateCurrentTick {
                price: spot_price_q64_64(token_a_amount, token_b_amount),
            },
        ))
        .event(
            Event::PoolCreated {
                amount_a: token_a_amount,
                amount_b: token_b_amount,
                liquidity: supply,
            }
            .to_program_event(),
        );
    finish(input, AmmState::Pool(pool), response)
}
