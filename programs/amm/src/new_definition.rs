use std::num::NonZeroU128;

use amm_core::{
    compute_liquidity_token_pda, compute_liquidity_token_pda_seed, compute_lp_lock_holding_pda,
    compute_lp_lock_holding_pda_seed, compute_pool_pda, compute_pool_pda_seed, compute_vault_pda,
    compute_vault_pda_seed, isqrt_product, spot_price_q64_64, AmmConfig, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ChainedCall},
};
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
    amm_program_id: AccountId,
) -> (Vec<AccountStateDiff>, Vec<ChainedCall>) {
    let definition_token_a_id = token_core::TokenHolding::try_from(&user_holding_a.account.data)
        .expect("New definition: AMM Program expects valid Token Holding account for Token A")
        .definition_id();
    let definition_token_b_id = token_core::TokenHolding::try_from(&user_holding_b.account.data)
        .expect("New definition: AMM Program expects valid Token Holding account for Token B")
        .definition_id();

    // The Token Program is taken from the config account, not trusted from a caller-supplied
    // holding. Validating the config PDA is also the Program's initialization gate.
    assert_eq!(
        config.account.program_owner, amm_program_id,
        "New definition: AMM config account must be owned by the AMM Program"
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
        compute_pool_pda(
            amm_program_id,
            config.account_id,
            definition_token_a_id,
            definition_token_b_id
        ),
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

    // LP Token minting calculation. The `token_a * token_b` product is computed in U256 (via
    // `isqrt_product`) so realistic 18-decimal amounts can't overflow u128 before the sqrt.
    let initial_lp = isqrt_product(token_a_amount.get(), token_b_amount.get());
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
    };

    // Capture the ids the chained calls name before the pre-states move into the diffs.
    let pool_id = pool.account_id;
    let config_id = config.account_id;
    let vault_a_id = vault_a.account_id;
    let vault_b_id = vault_b.account_id;
    let pool_definition_lp_id = pool_definition_lp.account_id;
    let lp_lock_holding_id = lp_lock_holding.account_id;
    let user_holding_a_id = user_holding_a.account_id;
    let user_holding_b_id = user_holding_b.account_id;
    let user_holding_lp_id = user_holding_lp.account_id;
    let current_tick_account_id = current_tick_account.account_id;
    let clock_id = clock.account_id;

    // Writing the pool definition is itself the claim on the pool PDA: v0.2.5 makes the
    // writing program the owner of a default-owned account it writes data to.
    let pool_diff =
        AccountStateDiff::new(pool, BalanceDiff::Add(0), Data::from(&pool_post_definition));

    // Every chained call below names its accounts by id and carries its authority in
    // `pda_seeds`. The runtime resolves each id against this transaction's accumulated
    // diff, so a callee sees the pool as claimed by the diff above and the LP definition
    // as created by an earlier call — none of that has to be predicted and hand-built here.

    // Chain call for Token A (user_holding_a -> Vault_A)
    let call_token_a = ChainedCall::new(
        token_program_id,
        vec![user_holding_a_id, vault_a_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_a_amount.into(),
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(pool_id, definition_token_a_id)]);
    // Chain call for Token B (user_holding_b -> Vault_B)
    let call_token_b = ChainedCall::new(
        token_program_id,
        vec![user_holding_b_id, vault_b_id],
        &token_core::Instruction::Transfer {
            amount_to_transfer: token_b_amount.into(),
        },
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(pool_id, definition_token_b_id)]);

    // Chain call for liquidity token lock holding
    let call_token_lp_lock = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_id, lp_lock_holding_id],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
            // Self-authority: the LP token is mintable only by the pool, which
            // presents this PDA as the authorized minter in the chained Mint call.
            mint_authority: Some(pool_definition_lp_id),
        },
    )
    .with_pda_seeds(vec![
        compute_liquidity_token_pda_seed(pool_id),
        compute_lp_lock_holding_pda_seed(pool_id),
    ]);

    let call_token_lp_user = ChainedCall::new(
        token_program_id,
        vec![pool_definition_lp_id, user_holding_lp_id],
        &token_core::Instruction::Mint {
            amount_to_mint: user_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    // Chain call to create the pool's TWAP current-tick account, with the pool as the price
    // source. The oracle derives the tick from the opening spot price (reserve_b / reserve_a as a
    // Q64.64 ratio), so the seed value is taken from the pool's own reserves, not the caller.
    let initial_price = spot_price_q64_64(token_a_amount.get(), token_b_amount.get());
    let call_create_current_tick = ChainedCall::new(
        twap_oracle_program_id,
        vec![current_tick_account_id, pool_id, clock_id],
        &twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config_id,
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

    let state_diffs = vec![
        AccountStateDiff::unchanged(config),
        pool_diff,
        AccountStateDiff::unchanged(vault_a),
        AccountStateDiff::unchanged(vault_b),
        AccountStateDiff::unchanged(pool_definition_lp),
        AccountStateDiff::unchanged(lp_lock_holding),
        AccountStateDiff::unchanged(user_holding_a),
        AccountStateDiff::unchanged(user_holding_b),
        AccountStateDiff::unchanged(user_holding_lp),
        AccountStateDiff::unchanged(current_tick_account),
        AccountStateDiff::unchanged(clock),
    ];

    (state_diffs, chained_calls)
}
