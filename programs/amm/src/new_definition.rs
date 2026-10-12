use std::num::NonZeroU128;

use amm_core::{
    compute_liquidity_token_pda, compute_liquidity_token_pda_seed, compute_lp_lock_holding_pda,
    compute_lp_lock_holding_pda_seed, compute_pool_pda, compute_pool_pda_seed, compute_vault_pda,
    compute_vault_pda_seed, isqrt_product, spot_price_q64_64, ConfigFacts, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use token_core::{TokenDescriptor, TokenKind};
use twap_oracle_core::compute_current_tick_account_pda;

const CONTEXT: &str = "New definition";

/// Plan half of `NewDefinition`: derive every address the pool needs, build the
/// pool record, and chain the five calls that fund and register it.
///
/// Returns the pool's bytes (written by [`shared::create`](crate::shared::create)
/// into a shard that must be empty) and the chained calls, in the order they run.
///
/// The token pair is declared rather than read off the trader's holdings, and
/// three separate things pin it: the pool PDA assertion ties the pair to the pool
/// *address*, the chained transfers carry it as their `TokenDescriptor` so the
/// Token Program refuses them unless the trader's holdings really hold that
/// asset, and the pool record this writes is what every later instruction pins
/// its own declaration against.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - the two declared definitions are the same token.
/// - `pool`, either vault, the LP definition, the LP lock holding, or `current_tick_account` is not
///   its expected PDA.
/// - `isqrt(a * b)` does not exceed [`MINIMUM_LIQUIDITY`], so there would be no LP left for the
///   caller after the permanent lock.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, mint, lock, and user accounts"
)]
#[must_use]
pub fn plan_new_definition(
    config: &AccountMeta,
    pool: &AccountMeta,
    vault_a: &AccountMeta,
    vault_b: &AccountMeta,
    pool_definition_lp: &AccountMeta,
    lp_lock_holding: &AccountMeta,
    user_holding_a: &AccountMeta,
    user_holding_b: &AccountMeta,
    user_holding_lp: &AccountMeta,
    current_tick_account: &AccountMeta,
    clock: &AccountMeta,
    token_a_amount: NonZeroU128,
    token_b_amount: NonZeroU128,
    facts: &ConfigFacts,
    definition_token_a_id: AccountId,
    definition_token_b_id: AccountId,
    now: u64,
    amm_program_id: AccountId,
) -> (ShardData, Vec<ChainedCall>) {
    crate::shared::assert_canonical_clock(clock, CONTEXT);
    assert!(
        definition_token_a_id != definition_token_b_id,
        "Cannot set up a swap for a token with itself"
    );

    let config_id = config.account_id;
    let pool_id = pool.account_id;
    assert_eq!(
        pool_id,
        compute_pool_pda(
            amm_program_id,
            config_id,
            definition_token_a_id,
            definition_token_b_id
        ),
        "Pool Definition Account ID does not match PDA"
    );
    assert_eq!(
        vault_a.account_id,
        compute_vault_pda(amm_program_id, pool_id, definition_token_a_id),
        "Vault ID does not match PDA"
    );
    assert_eq!(
        vault_b.account_id,
        compute_vault_pda(amm_program_id, pool_id, definition_token_b_id),
        "Vault ID does not match PDA"
    );
    assert_eq!(
        pool_definition_lp.account_id,
        compute_liquidity_token_pda(amm_program_id, pool_id),
        "Liquidity pool Token Definition Account ID does not match PDA"
    );
    assert_eq!(
        lp_lock_holding.account_id,
        compute_lp_lock_holding_pda(amm_program_id, pool_id),
        "LP lock holding Account ID does not match PDA"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(facts.twap_oracle_program_id, pool_id),
        "{CONTEXT}: current tick Account ID does not match PDA"
    );

    // LP minting. The `a * b` product is computed in U256 (via `isqrt_product`)
    // so realistic 18-decimal amounts cannot overflow u128 before the sqrt.
    let initial_lp = isqrt_product(token_a_amount.get(), token_b_amount.get());
    assert!(
        initial_lp > MINIMUM_LIQUIDITY,
        "Initial liquidity must exceed minimum liquidity lock"
    );
    let user_lp = initial_lp
        .checked_sub(MINIMUM_LIQUIDITY)
        .expect("initial liquidity must exceed minimum liquidity after validation");

    let pool_data = ShardData::from(&PoolDefinition {
        definition_token_a_id,
        definition_token_b_id,
        vault_a_id: vault_a.account_id,
        vault_b_id: vault_b.account_id,
        liquidity_pool_id: pool_definition_lp.account_id,
        liquidity_pool_supply: initial_lp,
        reserve_a: token_a_amount.into(),
        reserve_b: token_b_amount.into(),
    });

    let token_program_id = facts.token_program_id;
    let pool_seed = compute_pool_pda_seed(config_id, definition_token_a_id, definition_token_b_id);

    // The LP definition is self-authoritative: only the pool can mint it, and it
    // does so by presenting this PDA as the authorized minter in the mint call
    // below. The lock holding takes `MINIMUM_LIQUIDITY` permanently.
    let call_lp_definition = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(pool_definition_lp.account_id, token_program_id),
            ProgramShardSelector::new(lp_lock_holding.account_id, token_program_id),
        ],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
            mint_authority: Some(pool_definition_lp.account_id),
        },
    )
    .with_pda_seeds(vec![
        compute_liquidity_token_pda_seed(pool_id),
        compute_lp_lock_holding_pda_seed(pool_id),
    ]);

    let call_lp_mint = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(pool_definition_lp.account_id, token_program_id),
            ProgramShardSelector::new(user_holding_lp.account_id, token_program_id),
        ],
        &token_core::Instruction::Mint {
            amount_to_mint: user_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    let call_token_b = transfer_call(
        token_program_id,
        user_holding_b.account_id,
        vault_b.account_id,
        definition_token_b_id,
        token_b_amount.get(),
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(pool_id, definition_token_b_id)]);
    let call_token_a = transfer_call(
        token_program_id,
        user_holding_a.account_id,
        vault_a.account_id,
        definition_token_a_id,
        token_a_amount.get(),
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(pool_id, definition_token_a_id)]);

    // The pool's opening tick comes from its own reserves, not the caller: the
    // oracle derives the tick from this Q64.64 ratio.
    let initial_price = spot_price_q64_64(token_a_amount.get(), token_b_amount.get());
    let call_create_current_tick = ChainedCall::new(
        facts.twap_oracle_program_id,
        vec![
            ProgramShardSelector::new(
                current_tick_account.account_id,
                facts.twap_oracle_program_id,
            ),
            ProgramShardSelector::new(pool_id, amm_program_id),
            ProgramShardSelector::from(clock),
        ],
        &twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price, now },
    )
    .with_pda_seeds(vec![pool_seed]);

    (
        pool_data,
        vec![
            call_lp_definition,
            call_lp_mint,
            call_token_b,
            call_token_a,
            call_create_current_tick,
        ],
    )
}

/// A fungible `Transfer` chained into the Token Program.
///
/// The descriptor is the declare-and-pin device the Token Program uses: each
/// side's apply asserts its own holding matches it, which is how the AMM's
/// declared token pair becomes a fact about the trader's accounts.
#[must_use]
pub(crate) fn transfer_call(
    token_program_id: AccountId,
    from: AccountId,
    to: AccountId,
    definition_id: AccountId,
    amount: u128,
) -> ChainedCall {
    ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(from, token_program_id),
            ProgramShardSelector::new(to, token_program_id),
        ],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
            descriptor: TokenDescriptor {
                definition_id,
                kind: TokenKind::Fungible,
            },
        },
    )
}
