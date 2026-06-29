use std::num::NonZeroU128;

use amm_core::{
    assert_supported_fee_tier, compute_config_pda, compute_liquidity_token_pda_seed,
    compute_pool_pda_seed, mul_div_floor, read_vault_fungible_balances, spot_price_q64_64,
    AmmConfig, PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::compute_current_tick_account_pda;

#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
pub fn add_liquidity(
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
    min_amount_liquidity: NonZeroU128,
    max_amount_to_add_token_a: u128,
    max_amount_to_add_token_b: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Add liquidity: AMM config Account ID does not match PDA"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Add liquidity: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;

    // 1. Fetch Pool state
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Add liquidity: AMM Program expects valid Pool Definition Account");
    assert_supported_fee_tier(pool_def_data.fees);

    assert_eq!(
        vault_a.account_id, pool_def_data.vault_a_id,
        "Vault A was not provided"
    );

    assert_eq!(
        pool_def_data.liquidity_pool_id, pool_definition_lp.account_id,
        "LP definition mismatch"
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
    // clock here so the add is rejected early with an AMM-level error.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Add liquidity: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Add liquidity: current tick Account ID does not match PDA"
    );

    assert!(
        max_amount_to_add_token_a != 0 && max_amount_to_add_token_b != 0,
        "Both max-balances must be nonzero"
    );

    let (vault_a_balance, vault_b_balance) =
        read_vault_fungible_balances("Add liquidity", &vault_a, &vault_b);

    assert!(
        vault_a_balance >= pool_def_data.reserve_a,
        "Vaults' balances must be at least the reserve amounts"
    );
    assert!(
        vault_b_balance >= pool_def_data.reserve_b,
        "Vaults' balances must be at least the reserve amounts"
    );

    // 2. Determine deposit amount
    assert!(pool_def_data.reserve_a != 0, "Reserves must be nonzero");
    assert!(pool_def_data.reserve_b != 0, "Reserves must be nonzero");

    // floor(reserve * max_amount / reserve), products widened to U256. Reserves are nonzero
    // (asserted above), so the divisors are valid.
    let ideal_a: u128 = mul_div_floor(
        pool_def_data.reserve_a,
        max_amount_to_add_token_b,
        pool_def_data.reserve_b,
    );
    let ideal_b: u128 = mul_div_floor(
        pool_def_data.reserve_b,
        max_amount_to_add_token_a,
        pool_def_data.reserve_a,
    );

    let actual_amount_a = if ideal_a > max_amount_to_add_token_a {
        max_amount_to_add_token_a
    } else {
        ideal_a
    };
    let actual_amount_b = if ideal_b > max_amount_to_add_token_b {
        max_amount_to_add_token_b
    } else {
        ideal_b
    };

    // 3. Validate amounts
    assert!(
        max_amount_to_add_token_a >= actual_amount_a,
        "Actual trade amounts cannot exceed max_amounts"
    );
    assert!(
        max_amount_to_add_token_b >= actual_amount_b,
        "Actual trade amounts cannot exceed max_amounts"
    );

    assert!(actual_amount_a != 0, "A trade amount is 0");
    assert!(actual_amount_b != 0, "A trade amount is 0");

    // 4. Calculate LP to mint
    // floor(supply * actual / reserve), products widened to U256.
    let delta_lp = std::cmp::min(
        mul_div_floor(
            pool_def_data.liquidity_pool_supply,
            actual_amount_a,
            pool_def_data.reserve_a,
        ),
        mul_div_floor(
            pool_def_data.liquidity_pool_supply,
            actual_amount_b,
            pool_def_data.reserve_b,
        ),
    );

    assert!(delta_lp != 0, "Payable LP must be nonzero");

    assert!(
        delta_lp >= min_amount_liquidity.get(),
        "Payable LP is less than provided minimum LP amount"
    );

    // 5. Update pool account
    let mut pool_post = pool.account.clone();
    let pool_post_definition = PoolDefinition {
        liquidity_pool_supply: pool_def_data
            .liquidity_pool_supply
            .checked_add(delta_lp)
            .expect("liquidity_pool_supply + delta_lp overflows u128"),
        reserve_a: pool_def_data
            .reserve_a
            .checked_add(actual_amount_a)
            .expect("reserve_a + actual_amount_a overflows u128"),
        reserve_b: pool_def_data
            .reserve_b
            .checked_add(actual_amount_b)
            .expect("reserve_b + actual_amount_b overflows u128"),
        ..pool_def_data
    };

    pool_post.data = Data::from(&pool_post_definition);

    // Chain call for Token A (UserHoldingA -> Vault_A)
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![user_holding_a.clone(), vault_a.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: actual_amount_a,
        },
    );
    // Chain call for Token B (UserHoldingB -> Vault_B)
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![user_holding_b.clone(), vault_b.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: actual_amount_b,
        },
    );
    // Chain call for LP (mint new tokens for user_holding_lp)
    let mut pool_definition_lp_auth = pool_definition_lp.clone();
    pool_definition_lp_auth.is_authorized = true;
    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_auth.clone(), user_holding_lp.clone()],
        &token_core::Instruction::Mint {
            amount_to_mint: delta_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool.account_id)]);

    // Refresh the pool's TWAP current tick from the post-add spot price. The pool is already owned
    // by this program, so it is passed (in its post-add state) as the authorized price source.
    let new_price = spot_price_q64_64(
        pool_post_definition.reserve_a,
        pool_post_definition.reserve_b,
    );
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
        &twap_oracle_core::Instruction::UpdateCurrentTick { price: new_price },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        pool_def_data.definition_token_a_id,
        pool_def_data.definition_token_b_id,
    )]);

    let chained_calls = vec![call_token_lp, call_token_b, call_token_a, call_update_tick];

    let post_states = vec![
        AccountPostState::new(config.account.clone()),
        AccountPostState::new(pool_post),
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
