use std::num::NonZeroU128;

use amm_core::{
    assert_supported_fee_tier, compute_config_pda, compute_liquidity_token_pda_seed,
    compute_pool_pda_seed, compute_vault_pda_seed, mul_div_floor, spot_price_q64_64, AmmConfig,
    PoolDefinition, MINIMUM_LIQUIDITY,
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
    assert_supported_fee_tier(pool_def_data.fees);

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

    // Vault addresses do not need to be checked with PDA
    // calculation for setting authorization since stored
    // in the Pool Definition.
    let mut running_vault_a = vault_a.clone();
    let mut running_vault_b = vault_b.clone();
    running_vault_a.is_authorized = true;
    running_vault_b.is_authorized = true;

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
    let mut pool_post = pool.account.clone();
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

    pool_post.data = Data::from(&pool_post_definition);

    // Chaincall for Token A withdraw
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![running_vault_a, user_holding_a.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: withdraw_amount_a,
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
            amount_to_transfer: withdraw_amount_b,
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
            amount_to_burn: delta_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool.account_id)]);

    // Refresh the pool's TWAP current tick from the post-removal spot price. The pool is already
    // owned by this program, so it is passed (in its post-removal state) as the authorized price
    // source.
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
