use std::num::NonZeroU128;

use amm_core::{
    compute_config_pda, compute_liquidity_token_pda_seed, compute_pool_pda_seed,
    compute_vault_pda_seed, AmmConfig, PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::compute_current_tick_account_pda;

use crate::quote;

#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
pub fn remove_liquidity(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    pool_definition_lp: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    user_holding_lp: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    remove_liquidity_amount: NonZeroU128,
    min_amount_to_remove_token_a: u128,
    min_amount_to_remove_token_b: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let remove_liquidity_amount: u128 = remove_liquidity_amount.into();

    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Remove liquidity: AMM config Account ID does not match PDA"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Remove liquidity: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;

    // 1. Fetch Pool state
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Remove liquidity: AMM Program expects a valid Pool Definition Account");
    assert_eq!(
        pool_def_data.liquidity_pool_id, pool_definition_lp.account_id,
        "LP definition mismatch"
    );
    assert_eq!(
        vault_a.account_id, pool_def_data.vault_a_id,
        "Vault A was not provided"
    );
    assert_eq!(
        vault_b.account_id, pool_def_data.vault_b_id,
        "Vault B was not provided"
    );

    assert_eq!(
        vault_a.account.program_owner, token_program_id,
        "Vault A must be owned by the configured Token Program"
    );
    assert_eq!(
        vault_b.account.program_owner, token_program_id,
        "Vault B must be owned by the configured Token Program"
    );
    assert_eq!(
        user_holding_a.account.program_owner, token_program_id,
        "User Token A holding must be owned by the configured Token Program"
    );
    assert_eq!(
        user_holding_b.account.program_owner, token_program_id,
        "User Token B holding must be owned by the configured Token Program"
    );
    // The current tick is refreshed by a chained call to the oracle; validate its PDA and the
    // clock here so the removal is rejected early with an AMM-level error.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Remove liquidity: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Remove liquidity: current tick Account ID does not match PDA"
    );

    // Vault addresses do not need to be checked with PDA
    // calculation for setting authorization since stored
    // in the Pool Definition.
    let mut running_vault_a = vault_a.clone();
    let mut running_vault_b = vault_b.clone();
    running_vault_a.is_authorized = true;
    running_vault_b.is_authorized = true;

    // 2. Compute withdrawal amounts
    let user_holding_lp_data = token_core::TokenHolding::try_from(&user_holding_lp.account.data)
        .expect("Remove liquidity: AMM Program expects a valid Token Account for liquidity token");
    let token_core::TokenHolding::Fungible {
        definition_id: _,
        balance: user_lp_balance,
    } = user_holding_lp_data
    else {
        panic!(
            "Remove liquidity: AMM Program expects a valid Fungible Token Holding Account for liquidity token"
        );
    };

    assert_eq!(
        user_holding_lp_data.definition_id(),
        pool_def_data.liquidity_pool_id,
        "Invalid liquidity account provided"
    );
    let liquidity_quote = quote::remove_liquidity(
        &pool_def_data,
        user_lp_balance,
        remove_liquidity_amount,
        min_amount_to_remove_token_a,
        min_amount_to_remove_token_b,
    )
    .unwrap_or_else(|error| panic!("{error}"));

    // 5. Update pool account
    let mut pool_post = pool.account.clone();
    let pool_post_definition = liquidity_quote.pool.apply_to(&pool_def_data);

    pool_post.data = Data::from(&pool_post_definition);

    // Chaincall for Token A withdraw
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![running_vault_a, user_holding_a.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: liquidity_quote.withdraw_amount_a,
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool.account_id,
        pool_def_data.definition_token_a_id,
    )]);
    // Chaincall for Token B withdraw
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![running_vault_b, user_holding_b.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: liquidity_quote.withdraw_amount_b,
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool.account_id,
        pool_def_data.definition_token_b_id,
    )]);
    // Chaincall for LP adjustment
    let mut pool_definition_lp_auth = pool_definition_lp.clone();
    pool_definition_lp_auth.is_authorized = true;
    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_auth, user_holding_lp.clone()],
        &token_core::Instruction::Burn {
            amount_to_burn: liquidity_quote.liquidity_to_burn,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool.account_id)]);

    // Refresh the pool's TWAP current tick from the post-removal spot price. The pool is already
    // owned by this program, so it is passed (in its post-removal state) as the authorized price
    // source.
    let pool_price_source = AccountWithMetadata {
        account: pool_post.clone(),
        is_authorized: true,
        account_id: pool.account_id,
    };
    let call_update_tick = ChainedCall::new(
        twap_oracle_program_id,
        vec![
            current_tick_account.clone(),
            pool_price_source,
            clock.clone(),
        ],
        &twap_oracle_core::Instruction::UpdateCurrentTick {
            price: liquidity_quote.pool.spot_price_q64_64,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        pool_def_data.definition_token_a_id,
        pool_def_data.definition_token_b_id,
    )]);

    let chained_calls = vec![call_token_lp, call_token_b, call_token_a, call_update_tick];

    let post_states = vec![
        AccountPostState::new(config.account.clone()),
        AccountPostState::new(pool_post.clone()),
        AccountPostState::new(vault_a.account.clone()),
        AccountPostState::new(vault_b.account.clone()),
        AccountPostState::new(pool_definition_lp.account.clone()),
        AccountPostState::new(user_holding_a.account.clone()),
        AccountPostState::new(user_holding_b.account.clone()),
        AccountPostState::new(user_holding_lp.account.clone()),
        AccountPostState::new(current_tick_account.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ];

    (post_states, chained_calls)
}
