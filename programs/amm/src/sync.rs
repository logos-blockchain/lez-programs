use amm_core::{
    assert_supported_fee_tier, compute_config_pda, compute_pool_pda_seed,
    read_vault_fungible_balances, spot_price_q64_64, AmmConfig, PoolDefinition, MINIMUM_LIQUIDITY,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::compute_current_tick_account_pda;

pub fn sync_reserves(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Sync reserves: AMM Program expects a valid Pool Definition Account");
    assert_supported_fee_tier(pool_def_data.fees);

    // The TWAP oracle program ID is taken from the config account. Validating the config PDA is
    // also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Sync reserves: AMM config Account ID does not match PDA"
    );
    let twap_oracle_program_id = AmmConfig::try_from(&config.account.data)
        .expect("Sync reserves: AMM Program must be initialized before use")
        .twap_oracle_program_id;

    assert!(
        pool_def_data.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
        "Pool liquidity supply is below minimum liquidity"
    );
    assert_eq!(
        vault_a.account_id, pool_def_data.vault_a_id,
        "Vault A was not provided"
    );
    assert_eq!(
        vault_b.account_id, pool_def_data.vault_b_id,
        "Vault B was not provided"
    );
    // The current tick is refreshed by a chained call to the oracle; validate its PDA and the
    // clock here so the sync is rejected early with an AMM-level error.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Sync reserves: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Sync reserves: current tick Account ID does not match PDA"
    );

    let (vault_a_balance, vault_b_balance) =
        read_vault_fungible_balances("Sync reserves", &vault_a, &vault_b);
    assert!(
        vault_a_balance >= pool_def_data.reserve_a,
        "Sync reserves: vault A balance is less than its reserve"
    );
    assert!(
        vault_b_balance >= pool_def_data.reserve_b,
        "Sync reserves: vault B balance is less than its reserve"
    );

    let pool_post_definition = PoolDefinition {
        reserve_a: vault_a_balance,
        reserve_b: vault_b_balance,
        ..pool_def_data
    };
    let mut pool_post = pool.account.clone();
    pool_post.data = Data::from(&pool_post_definition);

    // Refresh the pool's TWAP current tick from the synced spot price. The pool is already owned by
    // this program, so it is passed (in its synced state) as the authorized price source.
    let new_price = spot_price_q64_64(vault_a_balance, vault_b_balance);
    let pool_price_source = AccountWithMetadata {
        account: pool_post.clone(),
        is_authorized: true,
        account_id: pool.account_id,
    };
    let update_tick_call = ChainedCall::new(
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

    (
        vec![
            AccountPostState::new(config.account.clone()),
            AccountPostState::new(pool_post),
            AccountPostState::new(vault_a.account.clone()),
            AccountPostState::new(vault_b.account.clone()),
            AccountPostState::new(current_tick_account.account.clone()),
            AccountPostState::new(clock.account.clone()),
        ],
        vec![update_tick_call],
    )
}
