use amm_core::{
    compute_liquidity_token_pda_seed, compute_pool_pda_seed, compute_vault_pda_seed, mul_div_floor,
    spot_price_q64_64, ConfigFacts, PoolDefinition, RemoveLiquidity, MINIMUM_LIQUIDITY,
};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use twap_oracle_core::compute_current_tick_account_pda;

use crate::new_definition::transfer_call;

const CONTEXT: &str = "Remove liquidity";

/// Plan half of `RemoveLiquidity`: check the addresses and chain the LP burn, the
/// two withdrawals, and the tick refresh.
///
/// Two v0.2.5 reads of the burner's own LP holding are gone, and nothing is lost
/// with them. That the holding is the pool's LP token, and that it holds enough
/// to burn, are both now the Token Program's business: the chained `Burn` carries
/// the LP definition as its `TokenDescriptor` and debits with a checked
/// subtraction, so a wrong token or an overdraw fails there.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - either vault slot or the LP definition slot is not the pool's own.
/// - `current_tick_account` is not the pool's current-tick PDA.
/// - either minimum-withdrawal bound is zero.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
#[must_use]
pub fn plan_remove_liquidity(
    config: &AccountMeta,
    pool: &AccountMeta,
    vault_a: &AccountMeta,
    vault_b: &AccountMeta,
    pool_definition_lp: &AccountMeta,
    user_holding_a: &AccountMeta,
    user_holding_b: &AccountMeta,
    user_holding_lp: &AccountMeta,
    current_tick_account: &AccountMeta,
    clock: &AccountMeta,
    facts: &ConfigFacts,
    effect: &RemoveLiquidity,
    now: u64,
    amm_program_id: AccountId,
) -> Vec<ChainedCall> {
    let identity = &effect.identity;
    crate::shared::assert_canonical_clock(clock, CONTEXT);
    crate::shared::assert_pool_in_namespace(
        pool,
        config.account_id,
        identity,
        amm_program_id,
        CONTEXT,
    );
    crate::shared::assert_vault_slots(vault_a, vault_b, identity, CONTEXT);
    assert_eq!(
        pool_definition_lp.account_id, identity.liquidity_pool_id,
        "LP definition mismatch"
    );
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(facts.twap_oracle_program_id, pool.account_id),
        "{CONTEXT}: current tick Account ID does not match PDA"
    );
    assert!(
        effect.min_amount_to_remove_token_a != 0,
        "Minimum withdraw amount must be nonzero"
    );
    assert!(
        effect.min_amount_to_remove_token_b != 0,
        "Minimum withdraw amount must be nonzero"
    );

    let pool_id = pool.account_id;
    let token_program_id = facts.token_program_id;

    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(pool_definition_lp.account_id, token_program_id),
            ProgramShardSelector::new(user_holding_lp.account_id, token_program_id),
        ],
        &token_core::Instruction::Burn {
            amount_to_burn: effect.remove_liquidity_amount,
            kind: token_core::TokenKind::Fungible,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    // Each withdrawal is debited from a vault, so each carries that vault's seed:
    // a chained call ships shard selectors, and the seed is the whole authority.
    let call_token_b = transfer_call(
        token_program_id,
        identity.vault_b_id,
        user_holding_b.account_id,
        identity.definition_token_b_id,
        effect.withdraw_amount_b,
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool_id,
        identity.definition_token_b_id,
    )]);
    let call_token_a = transfer_call(
        token_program_id,
        identity.vault_a_id,
        user_holding_a.account_id,
        identity.definition_token_a_id,
        effect.withdraw_amount_a,
    )
    .with_pda_seeds(vec![compute_vault_pda_seed(
        pool_id,
        identity.definition_token_a_id,
    )]);

    let call_update_tick = ChainedCall::new(
        facts.twap_oracle_program_id,
        vec![
            ProgramShardSelector::new(
                current_tick_account.account_id,
                facts.twap_oracle_program_id,
            ),
            ProgramShardSelector::new(pool_id, amm_program_id),
            ProgramShardSelector::from(clock),
        ],
        &twap_oracle_core::Instruction::UpdateCurrentTick {
            price: effect.new_price,
            now,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config.account_id,
        identity.definition_token_a_id,
        identity.definition_token_b_id,
    )]);

    vec![call_token_lp, call_token_b, call_token_a, call_update_tick]
}

/// Apply half of `RemoveLiquidity`: bound the declared withdrawals against the
/// live reserves and record the removal.
///
/// The mirror of [`add_liquidity`](crate::add::add_liquidity): the withdrawals may
/// not exceed the burner's pro-rata share of the reserves at the moment the burn
/// lands, and withdrawing less leaves the surplus with the remaining providers.
///
/// The `MINIMUM_LIQUIDITY` floor is enforced here rather than by looking at the
/// lock holding, which this instruction never sees: an honest flow cannot reach
/// the floor through a valid removal, but a legacy or corrupted pool already at it
/// is refused rather than drained.
///
/// # Panics
/// Panics if:
/// - the shard is not a valid pool, or the declared identity differs.
/// - the pool's supply is below, or exactly at, [`MINIMUM_LIQUIDITY`].
/// - `remove_liquidity_amount` is zero, or exceeds the unlocked supply.
/// - either withdrawal is below its declared minimum, or above the burner's pro-rata share of the
///   live reserves.
/// - `new_price` is not the post-removal spot price.
#[must_use]
pub fn remove_liquidity(pre_data: &ShardData, effect: &RemoveLiquidity) -> ShardData {
    let pool = crate::shared::pool_of(pre_data, &effect.identity, CONTEXT);

    assert!(
        pool.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
        "Pool liquidity supply is below minimum liquidity"
    );
    assert!(
        pool.liquidity_pool_supply > MINIMUM_LIQUIDITY,
        "Pool only contains locked liquidity"
    );
    assert!(
        effect.remove_liquidity_amount != 0,
        "Remove amount must be nonzero"
    );
    let unlocked_liquidity = pool
        .liquidity_pool_supply
        .checked_sub(MINIMUM_LIQUIDITY)
        .expect("liquidity supply exceeds the locked minimum after validation");
    assert!(
        effect.remove_liquidity_amount <= unlocked_liquidity,
        "Cannot remove locked minimum liquidity"
    );

    // floor(reserve * remove / supply), products widened to U256. Supply exceeds
    // MINIMUM_LIQUIDITY (asserted above), so the divisor is nonzero.
    let share_a = mul_div_floor(
        pool.reserve_a,
        effect.remove_liquidity_amount,
        pool.liquidity_pool_supply,
    );
    let share_b = mul_div_floor(
        pool.reserve_b,
        effect.remove_liquidity_amount,
        pool.liquidity_pool_supply,
    );
    assert!(
        effect.withdraw_amount_a <= share_a,
        "Withdrawal (Token A) exceeds the burner's pro-rata share of the pool's live reserves"
    );
    assert!(
        effect.withdraw_amount_b <= share_b,
        "Withdrawal (Token B) exceeds the burner's pro-rata share of the pool's live reserves"
    );
    assert!(
        effect.withdraw_amount_a >= effect.min_amount_to_remove_token_a,
        "Insufficient minimal withdraw amount (Token A) provided for liquidity amount"
    );
    assert!(
        effect.withdraw_amount_b >= effect.min_amount_to_remove_token_b,
        "Insufficient minimal withdraw amount (Token B) provided for liquidity amount"
    );

    let post = PoolDefinition {
        liquidity_pool_supply: pool
            .liquidity_pool_supply
            .checked_sub(effect.remove_liquidity_amount)
            .expect("liquidity_pool_supply - remove_liquidity_amount underflows"),
        reserve_a: pool
            .reserve_a
            .checked_sub(effect.withdraw_amount_a)
            .expect("reserve_a - withdraw_amount_a underflows"),
        reserve_b: pool
            .reserve_b
            .checked_sub(effect.withdraw_amount_b)
            .expect("reserve_b - withdraw_amount_b underflows"),
        ..pool
    };
    assert_eq!(
        effect.new_price,
        spot_price_q64_64(post.reserve_a, post.reserve_b),
        "Declared post-removal price does not match the pool's new spot price"
    );

    ShardData::from(&post)
}
