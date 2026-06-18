use std::num::NonZeroU128;

use amm_core::{
    assert_supported_fee_tier, compute_config_pda, compute_liquidity_token_pda,
    compute_liquidity_token_pda_seed, compute_lp_lock_holding_pda,
    compute_lp_lock_holding_pda_seed, compute_pool_pda, compute_pool_pda_seed, compute_vault_pda,
    compute_vault_pda_seed, spot_price_q64_64, AmmConfig, PoolDefinition, MINIMUM_LIQUIDITY,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, Claim, ProgramId},
};
use token_core::TokenDefinition;
use twap_oracle_core::compute_current_tick_account_pda;

#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, mint, lock, and user accounts"
)]
pub fn new_definition(
    config: AccountWithMetadata,
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    pool_definition_lp: AccountWithMetadata,
    lp_lock_holding: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    user_holding_lp: AccountWithMetadata,
    current_tick_account: AccountWithMetadata,
    clock: AccountWithMetadata,
    token_a_amount: NonZeroU128,
    token_b_amount: NonZeroU128,
    fees: u128,
    amm_program_id: ProgramId,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let definition_token_a_id = token_core::TokenHolding::try_from(&user_holding_a.account.data)
        .expect("New definition: AMM Program expects valid Token Holding account for Token A")
        .definition_id();
    let definition_token_b_id = token_core::TokenHolding::try_from(&user_holding_b.account.data)
        .expect("New definition: AMM Program expects valid Token Holding account for Token B")
        .definition_id();

    // The Token Program is taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account_id,
        compute_config_pda(amm_program_id),
        "New definition: AMM config Account ID does not match PDA"
    );
    let config_data = AmmConfig::try_from(&config.account.data)
        .expect("New definition: AMM Program must be initialized before use");
    let token_program_id = config_data.token_program_id;
    let twap_oracle_program_id = config_data.twap_oracle_program_id;

    assert_eq!(
        user_holding_a.account.program_owner, token_program_id,
        "User Token A holding must be owned by the configured Token Program"
    );
    assert_eq!(
        user_holding_b.account.program_owner, token_program_id,
        "User Token B holding must be owned by the configured Token Program"
    );
    // Verify token_a and token_b are different
    assert!(
        definition_token_a_id != definition_token_b_id,
        "Cannot set up a swap for a token with itself"
    );
    assert_eq!(
        pool.account_id,
        compute_pool_pda(amm_program_id, definition_token_a_id, definition_token_b_id),
        "Pool Definition Account ID does not match PDA"
    );
    assert_eq!(
        vault_a.account_id,
        compute_vault_pda(amm_program_id, pool.account_id, definition_token_a_id),
        "Vault ID does not match PDA"
    );
    assert_eq!(
        vault_b.account_id,
        compute_vault_pda(amm_program_id, pool.account_id, definition_token_b_id),
        "Vault ID does not match PDA"
    );
    assert_eq!(
        pool_definition_lp.account_id,
        compute_liquidity_token_pda(amm_program_id, pool.account_id),
        "Liquidity pool Token Definition Account ID does not match PDA"
    );
    assert_eq!(
        lp_lock_holding.account_id,
        compute_lp_lock_holding_pda(amm_program_id, pool.account_id),
        "LP lock holding Account ID does not match PDA"
    );
    assert_supported_fee_tier(fees);

    // Assert that pool is uninitialized (hard precondition)
    assert_eq!(
        pool.account,
        Account::default(),
        "Pool account must be uninitialized"
    );
    assert!(
        user_holding_lp.account != Account::default() || user_holding_lp.is_authorized,
        "Fresh user LP holding requires user authorization"
    );

    // The pool's TWAP current-tick account is created in the same transaction (a chained call to
    // the oracle). Validate its PDA and that the clock is the canonical 1-block LEZ clock.
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "New definition: current tick Account ID does not match PDA"
    );
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "New definition: clock account must be the canonical 1-block LEZ clock account"
    );

    // LP Token minting calculation
    let initial_lp = token_a_amount
        .get()
        .checked_mul(token_b_amount.get())
        .expect("token_a * token_b overflows u128")
        .isqrt();
    assert!(
        initial_lp > MINIMUM_LIQUIDITY,
        "Initial liquidity must exceed minimum liquidity lock"
    );
    let user_lp = initial_lp
        .checked_sub(MINIMUM_LIQUIDITY)
        .expect("initial liquidity must exceed minimum liquidity after validation");

    // Update pool account
    let pool_post_definition = PoolDefinition {
        definition_token_a_id,
        definition_token_b_id,
        vault_a_id: vault_a.account_id,
        vault_b_id: vault_b.account_id,
        liquidity_pool_id: pool_definition_lp.account_id,
        liquidity_pool_supply: initial_lp,
        reserve_a: token_a_amount.into(),
        reserve_b: token_b_amount.into(),
        fees,
    };

    let mut pool_initialized = pool.account.clone();
    pool_initialized.data = Data::from(&pool_post_definition);
    let pool_post: AccountPostState = AccountPostState::new_claimed(
        pool_initialized.clone(),
        Claim::Pda(compute_pool_pda_seed(
            definition_token_a_id,
            definition_token_b_id,
        )),
    );

    // Chain call for Token A (user_holding_a -> Vault_A)
    let mut vault_a_authorized = vault_a.clone();
    vault_a_authorized.is_authorized = true;
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![user_holding_a.clone(), vault_a_authorized],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_a_amount.into(),
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool.account_id,
        definition_token_a_id,
    )]);
    // Chain call for Token B (user_holding_b -> Vault_B)
    let mut vault_b_authorized = vault_b.clone();
    vault_b_authorized.is_authorized = true;
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![user_holding_b.clone(), vault_b_authorized],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_b_amount.into(),
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool.account_id,
        definition_token_b_id,
    )]);

    // Chain call for liquidity token lock holding
    let mut pool_lp_auth = pool_definition_lp.clone();
    pool_lp_auth.is_authorized = true;
    let mut lp_lock_holding_auth = lp_lock_holding.clone();
    lp_lock_holding_auth.is_authorized = true;

    let call_token_lp_lock = ChainedCall::new(
        token_program_id,
        vec![pool_lp_auth.clone(), lp_lock_holding_auth],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
        },
    )
    .with_pda_seeds(vec![
        compute_liquidity_token_pda_seed(pool.account_id),
        compute_lp_lock_holding_pda_seed(pool.account_id),
    ]);

    let mut pool_lp_after_lock = pool_lp_auth.clone();
    pool_lp_after_lock.account.program_owner = token_program_id;
    pool_lp_after_lock.account.data = Data::from(&TokenDefinition::Fungible {
        name: String::from("LP Token"),
        total_supply: MINIMUM_LIQUIDITY,
        metadata_id: None,
    });

    let call_token_lp_user = ChainedCall::new(
        token_program_id,
        vec![pool_lp_after_lock, user_holding_lp.clone()],
        &token_core::Instruction::Mint {
            amount_to_mint: user_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool.account_id)]);

    // Chain call to create the pool's TWAP current-tick account, with the pool as the price
    // source. The oracle derives the tick from the opening spot price (reserve_b / reserve_a as a
    // Q64.64 ratio), so the seed value is taken from the pool's own reserves, not the caller.
    //
    // The pool is claimed (and thus owned by this program) by this same instruction, so the
    // chained call must present the pool in its post-claim state to match the accumulated state
    // diff: the runtime sets the claimed pool's owner to this program, so we predict that here.
    let initial_price = spot_price_q64_64(token_a_amount.get(), token_b_amount.get());
    let mut pool_price_source_account = pool_initialized;
    pool_price_source_account.program_owner = amm_program_id;
    let pool_price_source = AccountWithMetadata {
        account: pool_price_source_account,
        is_authorized: true,
        account_id: pool.account_id,
    };
    let call_create_current_tick = ChainedCall::new(
        twap_oracle_program_id,
        vec![
            current_tick_account.clone(),
            pool_price_source,
            clock.clone(),
        ],
        &twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        definition_token_a_id,
        definition_token_b_id,
    )]);

    let chained_calls = vec![
        call_token_lp_lock,
        call_token_lp_user,
        call_token_b,
        call_token_a,
        call_create_current_tick,
    ];

    let post_states = vec![
        AccountPostState::new(config.account.clone()),
        pool_post.clone(),
        AccountPostState::new(vault_a.account.clone()),
        AccountPostState::new(vault_b.account.clone()),
        AccountPostState::new(pool_definition_lp.account.clone()),
        AccountPostState::new(lp_lock_holding.account.clone()),
        AccountPostState::new(user_holding_a.account.clone()),
        AccountPostState::new(user_holding_b.account.clone()),
        AccountPostState::new(user_holding_lp.account.clone()),
        AccountPostState::new(current_tick_account.account.clone()),
        AccountPostState::new(clock.account.clone()),
    ];

    (post_states, chained_calls)
}
