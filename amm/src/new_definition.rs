use std::num::NonZeroU128;

use amm_core::{
    assert_supported_fee_tier, compute_liquidity_token_pda, compute_liquidity_token_pda_seed,
    compute_lp_lock_holding_pda, compute_pool_pda, compute_vault_pda, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use nssa_core::{
    account::{Account, AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use token_core::TokenDefinition;

#[expect(clippy::too_many_arguments, reason = "TODO: Fix later")]
pub fn new_definition(
    pool: AccountWithMetadata,
    vault_a: AccountWithMetadata,
    vault_b: AccountWithMetadata,
    pool_definition_lp: AccountWithMetadata,
    lp_lock_holding: AccountWithMetadata,
    user_holding_a: AccountWithMetadata,
    user_holding_b: AccountWithMetadata,
    user_holding_lp: AccountWithMetadata,
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

    let token_program = user_holding_a.account.program_owner;

    // both instances of the same token program
    assert_eq!(
        user_holding_b.account.program_owner, token_program,
        "User Token holdings must use the same Token Program"
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
    let user_lp = initial_lp - MINIMUM_LIQUIDITY;

    // Update pool account
    let mut pool_post = pool.account.clone();
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

    pool_post.data = Data::from(&pool_post_definition);
    let pool_post: AccountPostState = AccountPostState::new_claimed(pool_post.clone());

    let token_program_id = user_holding_a.account.program_owner;

    // Chain call for Token A (user_holding_a -> Vault_A)
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![user_holding_a.clone(), vault_a.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_a_amount.into(),
        },
    );
    // Chain call for Token B (user_holding_b -> Vault_B)
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![user_holding_b.clone(), vault_b.clone()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_b_amount.into(),
        },
    );

    // Chain call for liquidity token lock holding
    let mut pool_lp_auth = pool_definition_lp.clone();
    pool_lp_auth.is_authorized = true;

    let call_token_lp_lock = ChainedCall::new(
        token_program_id,
        vec![pool_lp_auth.clone(), lp_lock_holding.clone()],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool.account_id)]);

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

    let chained_calls = vec![
        call_token_lp_lock,
        call_token_lp_user,
        call_token_b,
        call_token_a,
    ];

    let post_states = vec![
        pool_post.clone(),
        AccountPostState::new(vault_a.account.clone()),
        AccountPostState::new(vault_b.account.clone()),
        AccountPostState::new(pool_definition_lp.account.clone()),
        AccountPostState::new(lp_lock_holding.account.clone()),
        AccountPostState::new(user_holding_a.account.clone()),
        AccountPostState::new(user_holding_b.account.clone()),
        AccountPostState::new(user_holding_lp.account.clone()),
    ];

    (post_states, chained_calls)
}
