use amm_core::{
    compute_config_pda, compute_pool_pda_seed, read_vault_fungible_balances, AmmConfig,
};
pub use amm_core::{compute_liquidity_token_pda_seed, compute_vault_pda_seed, PoolDefinition};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use twap_oracle_core::compute_current_tick_account_pda;

use crate::quote::{self, PoolUpdate, SwapDirection};

/// Decodes pool state, checks vault IDs, and reads vault balances for quote validation.
fn validate_swap_setup(
    pool: &AccountWithMetadata,
    vault_a: &AccountWithMetadata,
    vault_b: &AccountWithMetadata,
) -> (PoolDefinition, u128, u128) {
    let pool_def_data = PoolDefinition::try_from(&pool.account.data)
        .expect("AMM Program expects a valid Pool Definition Account");
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

    (pool_def_data, vault_a_balance, vault_b_balance)
}

/// Assembles the swap post-states (including the echoed current-tick and clock accounts) and the
/// chained call that refreshes the pool's TWAP current tick from the post-swap spot price.
#[expect(
    clippy::too_many_arguments,
    reason = "post-state assembly keeps pool, vault, user, oracle, and quoted pool state explicit"
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
    // Echoed back at the input/output slot positions the guest declared, so the framework matches
    // each post-state to the correct account regardless of swap direction.
    user_holding_input: AccountWithMetadata,
    user_holding_output: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    pool_update: PoolUpdate,
    twap_oracle_program_id: ProgramId,
) -> (Vec<AccountPostState>, ChainedCall) {
    let pool_post_definition = pool_update.apply_to(&pool_def_data);

    let mut pool_post = pool.account.clone();
    pool_post.data = Data::from(&pool_post_definition);

    // Refresh the pool's TWAP current tick from the post-swap spot price. The pool is already owned
    // by this program, so it is passed (in its post-swap state) as the authorized price source.
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
        &twap_oracle_core::Instruction::UpdateCurrentTick {
            price: pool_update.spot_price_q64_64,
        },
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
        AccountPostState::new(user_holding_input.account),
        AccountPostState::new(user_holding_output.account),
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
    user_input_holding: AccountWithMetadata,
    user_output_holding: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    swap_amount_in: u128,
    min_amount_out: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let (pool_def_data, vault_a_balance, vault_b_balance) =
        validate_swap_setup(&pool, &vault_a, &vault_b);

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

    // Swap direction is taken from the (signed) input holding's own token definition, then the
    // role-based holdings are mapped back to the pool's stored A/B order so the rest of the
    // routine — reserve bookkeeping and finalize — stays keyed to token A/B.
    let token_in_id = token_core::TokenHolding::try_from(&user_input_holding.account.data)
        .expect("Swap exact input: input holding must be a valid token holding")
        .definition_id();
    let direction = quote::swap_direction(&pool_def_data, token_in_id).unwrap_or_else(|_| {
        panic!("Swap exact input: input holding token is not part of the pool")
    });
    let (user_holding_a, user_holding_b) = match direction {
        SwapDirection::AToB => (user_input_holding, user_output_holding),
        SwapDirection::BToA => (user_output_holding, user_input_holding),
    };
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

    let swap_quote = quote::swap_exact_input(
        &pool_def_data,
        vault_a_balance,
        vault_b_balance,
        direction,
        swap_amount_in,
        min_amount_out,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let chained_calls = match direction {
        SwapDirection::AToB => swap_chained_calls(
            user_holding_a.clone(),
            vault_a.clone(),
            vault_b.clone(),
            user_holding_b.clone(),
            swap_quote.amount_in,
            swap_quote.amount_out,
            pool.account_id,
        ),
        SwapDirection::BToA => swap_chained_calls(
            user_holding_b.clone(),
            vault_b.clone(),
            vault_a.clone(),
            user_holding_a.clone(),
            swap_quote.amount_in,
            swap_quote.amount_out,
            pool.account_id,
        ),
    };

    // Echo the two user holdings in the guest's declared slot order (input, then output) so the
    // framework matches each post-state to the right account. The a/b mapping above only drives the
    // reserve/vault bookkeeping; post-states are matched to accounts positionally.
    let (user_holding_input, user_holding_output) = match direction {
        SwapDirection::AToB => (user_holding_a, user_holding_b),
        SwapDirection::BToA => (user_holding_b, user_holding_a),
    };
    let (post_states, update_tick_call) = finalize_swap(
        config,
        pool,
        pool_def_data,
        vault_a,
        vault_b,
        user_holding_input,
        user_holding_output,
        current_tick_account,
        clock,
        swap_quote.pool,
        twap_oracle_program_id,
    );

    let mut chained_calls = chained_calls;
    chained_calls.push(update_tick_call);

    (post_states, chained_calls)
}

fn swap_chained_calls(
    user_deposit: AccountWithMetadata,
    vault_deposit: AccountWithMetadata,
    vault_withdraw: AccountWithMetadata,
    user_withdraw: AccountWithMetadata,
    amount_in: u128,
    amount_out: u128,
    pool_id: AccountId,
) -> Vec<ChainedCall> {
    let token_program_id = user_deposit.account.program_owner;

    let mut chained_calls = Vec::new();
    chained_calls.push(ChainedCall::new(
        token_program_id,
        vec![user_deposit, vault_deposit],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount_in,
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
                amount_to_transfer: amount_out,
            },
        )
        .with_pda_seeds(vec![pda_seed]),
    );

    chained_calls
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
    user_input_holding: AccountWithMetadata,
    user_output_holding: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    exact_amount_out: u128,
    max_amount_in: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let (pool_def_data, vault_a_balance, vault_b_balance) =
        validate_swap_setup(&pool, &vault_a, &vault_b);

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

    // Swap direction is taken from the (signed) input holding's own token definition, then the
    // role-based holdings are mapped back to the pool's stored A/B order so the rest of the
    // routine — reserve bookkeeping and finalize — stays keyed to token A/B.
    let token_in_id = token_core::TokenHolding::try_from(&user_input_holding.account.data)
        .expect("Swap exact output: input holding must be a valid token holding")
        .definition_id();
    let direction = quote::swap_direction(&pool_def_data, token_in_id).unwrap_or_else(|_| {
        panic!("Swap exact output: input holding token is not part of the pool")
    });
    let (user_holding_a, user_holding_b) = match direction {
        SwapDirection::AToB => (user_input_holding, user_output_holding),
        SwapDirection::BToA => (user_output_holding, user_input_holding),
    };
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

    let swap_quote = quote::swap_exact_output(
        &pool_def_data,
        vault_a_balance,
        vault_b_balance,
        direction,
        exact_amount_out,
        max_amount_in,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let chained_calls = match direction {
        SwapDirection::AToB => swap_chained_calls(
            user_holding_a.clone(),
            vault_a.clone(),
            vault_b.clone(),
            user_holding_b.clone(),
            swap_quote.amount_in,
            swap_quote.amount_out,
            pool.account_id,
        ),
        SwapDirection::BToA => swap_chained_calls(
            user_holding_b.clone(),
            vault_b.clone(),
            vault_a.clone(),
            user_holding_a.clone(),
            swap_quote.amount_in,
            swap_quote.amount_out,
            pool.account_id,
        ),
    };

    // Echo the two user holdings in the guest's declared slot order (input, then output) so the
    // framework matches each post-state to the right account. The a/b mapping above only drives the
    // reserve/vault bookkeeping; post-states are matched to accounts positionally.
    let (user_holding_input, user_holding_output) = match direction {
        SwapDirection::AToB => (user_holding_a, user_holding_b),
        SwapDirection::BToA => (user_holding_b, user_holding_a),
    };
    let (post_states, update_tick_call) = finalize_swap(
        config,
        pool,
        pool_def_data,
        vault_a,
        vault_b,
        user_holding_input,
        user_holding_output,
        current_tick_account,
        clock,
        swap_quote.pool,
        twap_oracle_program_id,
    );

    let mut chained_calls = chained_calls;
    chained_calls.push(update_tick_call);

    (post_states, chained_calls)
}
