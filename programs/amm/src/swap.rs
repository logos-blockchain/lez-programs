use amm_core::{
    assert_supported_fee_tier, compute_config_pda, compute_pool_pda_seed, mul_div_ceil,
    mul_div_floor, read_vault_fungible_balances, spot_price_q64_64, AmmConfig, FEE_BPS_DENOMINATOR,
    MINIMUM_LIQUIDITY,
};
pub use amm_core::{compute_liquidity_token_pda_seed, compute_vault_pda_seed, PoolDefinition};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::compute_current_tick_account_pda;

/// Validates swap setup: checks pool liquidity is ready, vaults match, and reserves are sufficient.
fn validate_swap_setup(
    pool: &AccountWithMetadata,
    vault_a: &AccountWithMetadata,
    vault_b: &AccountWithMetadata,
) -> PoolDefinition {
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("AMM Program expects a valid Pool Definition Account");
    assert_supported_fee_tier(pool_def_data.fees);

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

    let (vault_a_balance, vault_b_balance) =
        read_vault_fungible_balances("Validate swap setup", vault_a, vault_b);

    assert!(
        vault_a_balance >= pool_def_data.reserve_a,
        "Reserve for Token A exceeds vault balance"
    );
    assert!(
        vault_b_balance >= pool_def_data.reserve_b,
        "Reserve for Token B exceeds vault balance"
    );

    pool_def_data
}

/// Assembles the swap post-states (including the echoed current-tick and clock accounts) and the
/// chained call that refreshes the pool's TWAP current tick from the post-swap spot price.
#[expect(
    clippy::too_many_arguments,
    reason = "post-state assembly keeps pool, vault, user, oracle, and delta state explicit"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "consistent with codebase style"
)]
fn finalize_swap(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    pool_def_data: PoolDefinition,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    deposit_a: u128,
    withdraw_a: u128,
    deposit_b: u128,
    withdraw_b: u128,
    twap_oracle_program_id: ProgramId,
) -> (Vec<AccountPostState>, ChainedCall) {
    let pool_post_definition = PoolDefinition {
        reserve_a: pool_def_data
            .reserve_a
            .checked_add(deposit_a)
            .expect("reserve_a + deposit_a overflows u128")
            .checked_sub(withdraw_a)
            .expect("reserve_a + deposit_a - withdraw_a underflows"),
        reserve_b: pool_def_data
            .reserve_b
            .checked_add(deposit_b)
            .expect("reserve_b + deposit_b overflows u128")
            .checked_sub(withdraw_b)
            .expect("reserve_b + deposit_b - withdraw_b underflows"),
        ..pool_def_data
    };

    let mut pool_post = pool.account.clone();
    pool_post.data = Data::from(&pool_post_definition);

    // Refresh the pool's TWAP current tick from the post-swap spot price. The pool is already owned
    // by this program, so it is passed (in its post-swap state) as the authorized price source.
    let new_price = spot_price_q64_64(
        pool_post_definition.reserve_a,
        pool_post_definition.reserve_b,
    );
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

    let post_states = vec![
        AccountPostState::new(config.account),
        AccountPostState::new(pool_post),
        AccountPostState::new(vault_a.account),
        AccountPostState::new(vault_b.account),
        AccountPostState::new(user_holding_a.account),
        AccountPostState::new(user_holding_b.account),
        AccountPostState::new(current_tick_account.account),
        AccountPostState::new(clock.account),
    ];

    (post_states, update_tick_call)
}

#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
#[must_use]
pub fn swap_exact_input(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    swap_amount_in: u128,
    min_amount_out: u128,
    token_in_id: AccountId,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let pool_def_data = validate_swap_setup(&pool, &vault_a, &vault_b);

    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // account. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Swap exact input: AMM config Account ID does not match PDA"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Swap exact input: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;
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
    // clock here so the swap is rejected early with an AMM-level error.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Swap exact input: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Swap exact input: current tick Account ID does not match PDA"
    );

    let (chained_calls, [deposit_a, withdraw_a], [deposit_b, withdraw_b]) =
        if token_in_id == pool_def_data.definition_token_a_id {
            let (chained_calls, deposit_a, withdraw_b) = swap_logic(
                user_holding_a.clone(),
                vault_a.clone(),
                vault_b.clone(),
                user_holding_b.clone(),
                swap_amount_in,
                min_amount_out,
                pool_def_data.fees,
                pool_def_data.reserve_a,
                pool_def_data.reserve_b,
                pool.account_id,
            );

            (chained_calls, [deposit_a, 0], [0, withdraw_b])
        } else if token_in_id == pool_def_data.definition_token_b_id {
            let (chained_calls, deposit_b, withdraw_a) = swap_logic(
                user_holding_b.clone(),
                vault_b.clone(),
                vault_a.clone(),
                user_holding_a.clone(),
                swap_amount_in,
                min_amount_out,
                pool_def_data.fees,
                pool_def_data.reserve_b,
                pool_def_data.reserve_a,
                pool.account_id,
            );

            (chained_calls, [0, withdraw_a], [deposit_b, 0])
        } else {
            panic!("AccountId is not a token type for the pool");
        };

    let (post_states, update_tick_call) = finalize_swap(
        config,
        pool,
        pool_def_data,
        vault_a,
        vault_b,
        user_holding_a,
        user_holding_b,
        current_tick_account,
        clock,
        deposit_a,
        withdraw_a,
        deposit_b,
        withdraw_b,
        twap_oracle_program_id,
    );

    let mut chained_calls = chained_calls;
    chained_calls.push(update_tick_call);

    (post_states, chained_calls)
}

#[expect(
    clippy::too_many_arguments,
    reason = "swap calculation keeps account context and pricing parameters explicit"
)]
fn swap_logic(
    user_deposit: AccountWithMetadata,
    vault_deposit: AccountWithMetadata,
    vault_withdraw: AccountWithMetadata,
    user_withdraw: AccountWithMetadata,
    swap_amount_in: u128,
    min_amount_out: u128,
    fee_bps: u128,
    reserve_deposit_vault_amount: u128,
    reserve_withdraw_vault_amount: u128,
    pool_id: AccountId,
) -> (Vec<ChainedCall>, u128, u128) {
    let fee_multiplier = FEE_BPS_DENOMINATOR
        .checked_sub(fee_bps)
        .expect("fee_bps exceeds fee denominator");
    // floor(swap_amount_in * fee_multiplier / FEE_BPS_DENOMINATOR), product widened to U256.
    let effective_amount_in = mul_div_floor(swap_amount_in, fee_multiplier, FEE_BPS_DENOMINATOR);
    assert!(
        effective_amount_in != 0,
        "Effective swap amount should be nonzero"
    );
    // Compute the withdraw amount using the fee-adjusted input for pricing.
    // The recorded pool reserves are updated later with the full
    // `swap_amount_in`, so LP fees accrue inside `reserve_*` via invariant
    // growth rather than as a separate vault balance surplus over `reserve_*`.
    // The denominator sum stays u128 (overflows only near u128::MAX, an unstorable reserve);
    // only the `reserve * effective` product is widened to U256.
    let reserve_plus_effective = reserve_deposit_vault_amount
        .checked_add(effective_amount_in)
        .expect("reserve + effective_amount_in overflows u128");
    let withdraw_amount = mul_div_floor(
        reserve_withdraw_vault_amount,
        effective_amount_in,
        reserve_plus_effective,
    );

    // Slippage check
    assert!(
        min_amount_out <= withdraw_amount,
        "Withdraw amount is less than minimal amount out"
    );
    assert!(withdraw_amount != 0, "Withdraw amount should be nonzero");

    let token_program_id = user_deposit.account.program_owner;

    let mut chained_calls = Vec::new();
    chained_calls.push(ChainedCall::new(
        token_program_id,
        vec![user_deposit, vault_deposit],
        &token_core::Instruction::Transfer {
            amount_to_transfer: swap_amount_in,
        },
    ));

    let mut vault_withdraw = vault_withdraw.clone();
    vault_withdraw.is_authorized = true;

    let pda_seed = compute_vault_pda_seed(
        pool_id,
        token_core::TokenHolding::try_from(&vault_withdraw.account.data)
            .expect("Swap Logic: AMM Program expects valid token data")
            .definition_id(),
    );

    chained_calls.push(
        ChainedCall::new(
            token_program_id,
            vec![vault_withdraw, user_withdraw],
            &token_core::Instruction::Transfer {
                amount_to_transfer: withdraw_amount,
            },
        )
        .with_pda_seeds(vec![pda_seed]),
    );

    (chained_calls, swap_amount_in, withdraw_amount)
}

#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
#[must_use]
pub fn swap_exact_output(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    exact_amount_out: u128,
    max_amount_in: u128,
    token_in_id: AccountId,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let pool_def_data = validate_swap_setup(&pool, &vault_a, &vault_b);

    // The program IDs are taken from the config account, not trusted from a caller-supplied
    // account. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "Swap exact output: AMM config Account ID does not match PDA"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("Swap exact output: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;
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
    // clock here so the swap is rejected early with an AMM-level error.
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Swap exact output: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "Swap exact output: current tick Account ID does not match PDA"
    );

    let (chained_calls, [deposit_a, withdraw_a], [deposit_b, withdraw_b]) =
        if token_in_id == pool_def_data.definition_token_a_id {
            let (chained_calls, deposit_a, withdraw_b) = exact_output_swap_logic(
                user_holding_a.clone(),
                vault_a.clone(),
                vault_b.clone(),
                user_holding_b.clone(),
                exact_amount_out,
                max_amount_in,
                pool_def_data.reserve_a,
                pool_def_data.reserve_b,
                pool_def_data.fees,
                pool.account_id,
            );

            (chained_calls, [deposit_a, 0], [0, withdraw_b])
        } else if token_in_id == pool_def_data.definition_token_b_id {
            let (chained_calls, deposit_b, withdraw_a) = exact_output_swap_logic(
                user_holding_b.clone(),
                vault_b.clone(),
                vault_a.clone(),
                user_holding_a.clone(),
                exact_amount_out,
                max_amount_in,
                pool_def_data.reserve_b,
                pool_def_data.reserve_a,
                pool_def_data.fees,
                pool.account_id,
            );

            (chained_calls, [0, withdraw_a], [deposit_b, 0])
        } else {
            panic!("AccountId is not a token type for the pool");
        };

    let (post_states, update_tick_call) = finalize_swap(
        config,
        pool,
        pool_def_data,
        vault_a,
        vault_b,
        user_holding_a,
        user_holding_b,
        current_tick_account,
        clock,
        deposit_a,
        withdraw_a,
        deposit_b,
        withdraw_b,
        twap_oracle_program_id,
    );

    let mut chained_calls = chained_calls;
    chained_calls.push(update_tick_call);

    (post_states, chained_calls)
}

#[expect(
    clippy::too_many_arguments,
    reason = "swap calculation keeps account context and pricing parameters explicit"
)]
fn exact_output_swap_logic(
    user_deposit: AccountWithMetadata,
    vault_deposit: AccountWithMetadata,
    vault_withdraw: AccountWithMetadata,
    user_withdraw: AccountWithMetadata,
    exact_amount_out: u128,
    max_amount_in: u128,
    reserve_deposit_vault_amount: u128,
    reserve_withdraw_vault_amount: u128,
    fee_bps: u128,
    pool_id: AccountId,
) -> (Vec<ChainedCall>, u128, u128) {
    // Guard: exact_amount_out must be nonzero
    assert_ne!(exact_amount_out, 0, "Exact amount out must be nonzero");

    // Guard: exact_amount_out must be less than reserve_withdraw_vault_amount
    assert!(
        exact_amount_out < reserve_withdraw_vault_amount,
        "Exact amount out exceeds reserve"
    );

    // Compute the minimum effective input required to achieve exact_amount_out
    // using the same floor-rounded fee application as swap_exact_input.
    //
    // Solve constant product for effective_in (fee already removed):
    //   effective_in >= ceil(reserve_in * amount_out / (reserve_out - amount_out))
    // ceil(reserve_in * amount_out / (reserve_out - amount_out)). The `reserve_in * amount_out`
    // product is widened to U256; the denominator is a subtraction that stays u128.
    let effective_in_denominator = reserve_withdraw_vault_amount
        .checked_sub(exact_amount_out)
        .expect("reserve_out - amount_out underflows");
    let effective_in_min = mul_div_ceil(
        reserve_deposit_vault_amount,
        exact_amount_out,
        effective_in_denominator,
    );

    // Lift back to gross input so that
    //   floor(gross_in * (FEE_DENOM - fee) / FEE_DENOM) >= effective_in_min
    // ceil(effective_in_min * FEE_BPS_DENOMINATOR / fee_multiplier), product widened to U256.
    let fee_multiplier = FEE_BPS_DENOMINATOR
        .checked_sub(fee_bps)
        .expect("fee_bps exceeds fee denominator");
    let deposit_amount = mul_div_ceil(effective_in_min, FEE_BPS_DENOMINATOR, fee_multiplier);

    // Slippage check
    assert!(
        deposit_amount <= max_amount_in,
        "Required input exceeds maximum amount in"
    );

    let token_program_id = user_deposit.account.program_owner;

    let mut chained_calls = Vec::new();
    chained_calls.push(ChainedCall::new(
        token_program_id,
        vec![user_deposit, vault_deposit],
        &token_core::Instruction::Transfer {
            amount_to_transfer: deposit_amount,
        },
    ));

    let mut vault_withdraw = vault_withdraw;
    vault_withdraw.is_authorized = true;

    let pda_seed = compute_vault_pda_seed(
        pool_id,
        token_core::TokenHolding::try_from(&vault_withdraw.account.data)
            .expect("Exact Output Swap Logic: AMM Program expects valid token data")
            .definition_id(),
    );

    chained_calls.push(
        ChainedCall::new(
            token_program_id,
            vec![vault_withdraw, user_withdraw],
            &token_core::Instruction::Transfer {
                amount_to_transfer: exact_amount_out,
            },
        )
        .with_pda_seeds(vec![pda_seed]),
    );

    (chained_calls, deposit_amount, exact_amount_out)
}
