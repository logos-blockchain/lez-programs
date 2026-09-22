use amm_core::{
    compute_pool_pda, compute_pool_pda_seed, read_vault_fungible_balances, spot_price_q64_64,
    AmmConfig, PoolDefinition, MINIMUM_LIQUIDITY,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::{AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
use twap_oracle_core::compute_current_tick_account_pda;

pub fn sync_reserves(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    amm_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Sync reserves: AMM Program expects a valid Pool Definition Account");

    // The TWAP oracle program ID is taken from the config account. Validating the config PDA is
    // also the Program's initialization gate.
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "Sync reserves: AMM config account must be owned by the AMM Program"
    );
    let twap_oracle_program_id = AmmConfig::try_from(&config.account.data)
        .expect("Sync reserves: AMM Program must be initialized before use")
        .twap_oracle_program_id;

    // The pool must be derived under THIS config's namespace. config.account_id is the
    // namespace root, so a pool belonging to another instance — even a valid AMM-owned
    // pool with the same token pair — derives a different PDA and is rejected here. This
    // stops a caller from pairing any AMM-owned config with an arbitrary pool (matching
    // the check new_definition makes when it creates the pool).
    assert_eq!(
        pool.account_id,
        compute_pool_pda(
            amm_program_id,
            config.account_id,
            pool_def_data.definition_token_a_id,
            pool_def_data.definition_token_b_id,
        ),
        "Sync reserves: pool account is not derived under this config's namespace"
    );

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
    // Refresh the pool's TWAP current tick from the synced spot price. The oracle reads the
    // pool in its *synced* state without this program hand-building that state: a chained
    // call names accounts by id, and the runtime resolves each one from this transaction's
    // diff first, so the pool write below is what the callee sees. The pool PDA seed is what
    // authorizes the oracle over it.
    let new_price = spot_price_q64_64(vault_a_balance, vault_b_balance);
    let pool_seed = compute_pool_pda_seed(
        config.account_id,
        pool_def_data.definition_token_a_id,
        pool_def_data.definition_token_b_id,
    );
    let update_tick_call = ChainedCall::new(
        twap_oracle_program_id,
        vec![
            current_tick_account.account_id,
            pool.account_id,
            clock.account_id,
        ],
        &twap_oracle_core::Instruction::UpdateCurrentTick { price: new_price },
    )
    .with_pda_seeds(vec![pool_seed]);

    (
        vec![
            AccountStateDiff::unchanged(config),
            AccountStateDiff::new(pool, BalanceDiff::Add(0), Data::from(&pool_post_definition)),
            AccountStateDiff::unchanged(vault_a),
            AccountStateDiff::unchanged(vault_b),
            AccountStateDiff::unchanged(current_tick_account),
            AccountStateDiff::unchanged(clock),
        ],
        vec![update_tick_call],
    )
}
