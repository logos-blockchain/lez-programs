use std::num::NonZeroU128;

use amm_core::{
    compute_liquidity_token_pda_seed, compute_pool_pda, compute_pool_pda_seed, mul_div_floor,
    read_vault_fungible_balances, spot_price_q64_64, AmmConfig, PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::{AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
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
    amm_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "Add liquidity: AMM config account must be owned by the AMM Program"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Add liquidity: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;

    // 1. Fetch Pool state
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Add liquidity: AMM Program expects valid Pool Definition Account");

    // The pool must be derived under THIS config's namespace. config.account_id is the
    // namespace root, so a pool belonging to another instance — even a valid AMM-owned
    // pool with the same token pair — derives a different PDA and is rejected here. This
    // stops a caller from pairing a config from one instance with a pool from another
    // (matching the check new_definition makes when it creates the pool).
    assert_eq!(
        pool.account_id,
        compute_pool_pda(
            amm_program_id,
            config.account_id,
            pool_def_data.definition_token_a_id,
            pool_def_data.definition_token_b_id,
        ),
        "Add liquidity: pool account is not derived under this config's namespace"
    );

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

    // Ids for the chained calls, captured before the pre-states move into the diffs.
    let pool_id = pool.account_id;
    let config_id = config.account_id;
    let vault_a_id = vault_a.account_id;
    let vault_b_id = vault_b.account_id;
    let pool_definition_lp_id = pool_definition_lp.account_id;
    let user_holding_a_id = user_holding_a.account_id;
    let user_holding_b_id = user_holding_b.account_id;
    let user_holding_lp_id = user_holding_lp.account_id;
    let current_tick_account_id = current_tick_account.account_id;
    let clock_id = clock.account_id;

    // Chain call for Token A (UserHoldingA -> Vault_A)
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![user_holding_a_id, vault_a_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: actual_amount_a,
        },
    );
    // Chain call for Token B (UserHoldingB -> Vault_B)
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![user_holding_b_id, vault_b_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: actual_amount_b,
        },
    );
    // Chain call for LP (mint new tokens for user_holding_lp). The LP definition PDA seed
    // is what authorizes the mint — chained calls carry ids, not authorized pre-states.
    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_id, user_holding_lp_id],
        &token_core::Instruction::Mint {
            amount_to_mint: delta_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    // Refresh the pool's TWAP current tick from the post-add spot price. The oracle sees the
    // pool in its post-add state because the runtime resolves a call's account ids against
    // this transaction's diff, which already carries the pool write below.
    let new_price = spot_price_q64_64(
        pool_post_definition.reserve_a,
        pool_post_definition.reserve_b,
    );
    let call_update_tick = ChainedCall::new(
        twap_oracle_program_id,
        vec![current_tick_account_id, pool_id, clock_id],
        &twap_oracle_core::Instruction::UpdateCurrentTick { price: new_price },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config_id,
        pool_def_data.definition_token_a_id,
        pool_def_data.definition_token_b_id,
    )]);

    let chained_calls = vec![call_token_lp, call_token_b, call_token_a, call_update_tick];

    let state_diffs = vec![
        AccountStateDiff::unchanged(config),
        AccountStateDiff::new(pool, BalanceDiff::Add(0), Data::from(&pool_post_definition)),
        AccountStateDiff::unchanged(vault_a),
        AccountStateDiff::unchanged(vault_b),
        AccountStateDiff::unchanged(pool_definition_lp),
        AccountStateDiff::unchanged(user_holding_a),
        AccountStateDiff::unchanged(user_holding_b),
        AccountStateDiff::unchanged(user_holding_lp),
        AccountStateDiff::unchanged(current_tick_account),
        AccountStateDiff::unchanged(clock),
    ];

    (state_diffs, chained_calls)
}
