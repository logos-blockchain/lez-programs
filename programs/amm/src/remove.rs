use std::num::NonZeroU128;

use amm_core::{
    compute_liquidity_token_pda_seed, compute_pool_pda, compute_pool_pda_seed,
    compute_vault_pda_seed, mul_div_floor, spot_price_q64_64, AmmConfig, PoolDefinition,
    MINIMUM_LIQUIDITY,
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
    amm_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    let remove_liquidity_amount: u128 = remove_liquidity_amount.into();

    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "Remove liquidity: AMM config account must be owned by the AMM Program"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Remove liquidity: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;

    // 1. Fetch Pool state
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("Remove liquidity: AMM Program expects a valid Pool Definition Account");

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
        "Remove liquidity: pool account is not derived under this config's namespace"
    );

    assert!(
        pool_def_data.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
        "Pool liquidity supply is below minimum liquidity"
    );
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

    // Vault addresses do not need a PDA check here: they are read from the Pool
    // Definition, and the vault PDA seeds on the chained transfers below are what
    // authorize the token program over them.

    assert!(
        min_amount_to_remove_token_a != 0,
        "Minimum withdraw amount must be nonzero"
    );
    assert!(
        min_amount_to_remove_token_b != 0,
        "Minimum withdraw amount must be nonzero"
    );

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

    assert!(
        user_lp_balance <= pool_def_data.liquidity_pool_supply,
        "Invalid liquidity account provided"
    );
    assert_eq!(
        user_holding_lp_data.definition_id(),
        pool_def_data.liquidity_pool_id,
        "Invalid liquidity account provided"
    );
    // Honest flows should never reach the permanent lock through a valid remove instruction, but
    // we still reject legacy or corrupted states that are already at the locked floor.
    assert!(
        pool_def_data.liquidity_pool_supply > MINIMUM_LIQUIDITY,
        "Pool only contains locked liquidity"
    );
    assert!(
        remove_liquidity_amount <= user_lp_balance,
        "Remove amount exceeds user LP balance"
    );
    let unlocked_liquidity = pool_def_data
        .liquidity_pool_supply
        .checked_sub(MINIMUM_LIQUIDITY)
        .expect("liquidity supply must be at least the locked minimum after validation");
    // The remove instruction never sees the LP lock account directly, so we must still refuse any
    // request that would burn through the permanent floor even if ownership is already corrupted.
    assert!(
        remove_liquidity_amount <= unlocked_liquidity,
        "Cannot remove locked minimum liquidity"
    );

    // floor(reserve * remove_amount / supply), products widened to U256. Supply exceeds
    // MINIMUM_LIQUIDITY (asserted above), so the divisor is nonzero.
    let withdraw_amount_a = mul_div_floor(
        pool_def_data.reserve_a,
        remove_liquidity_amount,
        pool_def_data.liquidity_pool_supply,
    );
    let withdraw_amount_b = mul_div_floor(
        pool_def_data.reserve_b,
        remove_liquidity_amount,
        pool_def_data.liquidity_pool_supply,
    );

    // 3. Validate and slippage check
    assert!(
        withdraw_amount_a >= min_amount_to_remove_token_a,
        "Insufficient minimal withdraw amount (Token A) provided for liquidity amount"
    );
    assert!(
        withdraw_amount_b >= min_amount_to_remove_token_b,
        "Insufficient minimal withdraw amount (Token B) provided for liquidity amount"
    );

    // 4. Calculate LP to reduce cap by
    let delta_lp: u128 = remove_liquidity_amount;

    // 5. Update pool account
    let pool_post_definition = PoolDefinition {
        liquidity_pool_supply: pool_def_data
            .liquidity_pool_supply
            .checked_sub(delta_lp)
            .expect("liquidity_pool_supply - delta_lp underflows"),
        reserve_a: pool_def_data
            .reserve_a
            .checked_sub(withdraw_amount_a)
            .expect("reserve_a - withdraw_amount_a underflows"),
        reserve_b: pool_def_data
            .reserve_b
            .checked_sub(withdraw_amount_b)
            .expect("reserve_b - withdraw_amount_b underflows"),
        ..pool_def_data.clone()
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

    // Chaincall for Token A withdraw
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![vault_a_id, user_holding_a_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: withdraw_amount_a,
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool_id,
        pool_def_data.definition_token_a_id,
    )]);
    // Chaincall for Token B withdraw
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![vault_b_id, user_holding_b_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: withdraw_amount_b,
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool_id,
        pool_def_data.definition_token_b_id,
    )]);
    // Chaincall for LP adjustment
    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_id, user_holding_lp_id],
        &token_core::Instruction::Burn {
            amount_to_burn: delta_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    // Refresh the pool's TWAP current tick from the post-removal spot price. The oracle sees
    // the pool in its post-removal state: a call names accounts by id and the runtime
    // resolves each against this transaction's diff, which carries the pool write below.
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
