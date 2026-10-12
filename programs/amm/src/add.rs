use amm_core::{
    compute_liquidity_token_pda_seed, compute_pool_pda_seed, mul_div_floor, spot_price_q64_64,
    AddLiquidity, ConfigFacts, PoolDefinition,
};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use twap_oracle_core::compute_current_tick_account_pda;

use crate::new_definition::transfer_call;

const CONTEXT: &str = "Add liquidity";

/// Plan half of `AddLiquidity`: check the addresses and chain the two deposits,
/// the LP mint, and the tick refresh.
///
/// The figures the calls carry — the two deposits and the LP to mint — are
/// declared, because they are functions of the live reserves and no plan can read
/// those. [`add_liquidity`] is what makes them safe.
///
/// The four `vault_balance >= reserve` assertions from the v0.2.5 version are
/// gone, and not merely because the data moved out of reach: only this program
/// can debit a vault, and every reserve change it records is paired with an equal
/// transfer, so the backing is structural. `SyncReserves` is the repair path if
/// it ever is not.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - either vault slot or the LP definition slot is not the pool's own.
/// - `current_tick_account` is not the pool's current-tick PDA.
/// - either max-deposit bound is zero.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit pool, vault, and user accounts"
)]
#[must_use]
pub fn plan_add_liquidity(
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
    effect: &AddLiquidity,
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
        effect.max_amount_a != 0 && effect.max_amount_b != 0,
        "Both max-balances must be nonzero"
    );

    let pool_id = pool.account_id;
    let token_program_id = facts.token_program_id;

    let call_token_lp = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(pool_definition_lp.account_id, token_program_id),
            ProgramShardSelector::new(user_holding_lp.account_id, token_program_id),
        ],
        &token_core::Instruction::Mint {
            amount_to_mint: effect.delta_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(pool_id)]);

    let call_token_b = transfer_call(
        token_program_id,
        user_holding_b.account_id,
        identity.vault_b_id,
        identity.definition_token_b_id,
        effect.amount_b,
    );
    let call_token_a = transfer_call(
        token_program_id,
        user_holding_a.account_id,
        identity.vault_a_id,
        identity.definition_token_a_id,
        effect.amount_a,
    );

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

/// Apply half of `AddLiquidity`: bound the declared figures against the live
/// reserves and record the deposit.
///
/// The pool's only real interest is that the LP minted not exceed what the
/// deposit bought at the live ratio — `delta_lp <= supply * amount / reserve` on
/// *both* sides, so a lopsided deposit is priced by its scarcer leg. Declaring
/// less LP, or more deposit, dilutes nobody and simply leaves the surplus with
/// the existing providers, which is why one direction suffices. The caller's own
/// bounds are re-asserted here too, against the figures that actually land rather
/// than the ones they were quoted.
///
/// # Panics
/// Panics if:
/// - the shard is not a valid pool, or the declared identity differs.
/// - either stored reserve is zero (an uninitialized or drained pool).
/// - either deposit is zero, or exceeds its declared maximum.
/// - `delta_lp` is zero, below `min_amount_liquidity`, or above the pro-rata share of the deposit
///   at the live reserves.
/// - `new_price` is not the post-add spot price.
#[must_use]
pub fn add_liquidity(pre_data: &ShardData, effect: &AddLiquidity) -> ShardData {
    let pool = crate::shared::pool_of(pre_data, &effect.identity, CONTEXT);

    assert!(pool.reserve_a != 0, "Reserves must be nonzero");
    assert!(pool.reserve_b != 0, "Reserves must be nonzero");
    assert!(effect.amount_a != 0, "A trade amount is 0");
    assert!(effect.amount_b != 0, "A trade amount is 0");
    assert!(
        effect.amount_a <= effect.max_amount_a,
        "Actual trade amounts cannot exceed max_amounts"
    );
    assert!(
        effect.amount_b <= effect.max_amount_b,
        "Actual trade amounts cannot exceed max_amounts"
    );
    assert!(effect.delta_lp != 0, "Payable LP must be nonzero");
    assert!(
        effect.delta_lp >= effect.min_amount_liquidity,
        "Payable LP is less than provided minimum LP amount"
    );

    // floor(supply * amount / reserve) per side, products widened to U256.
    // Reserves are nonzero (asserted above), so the divisors are valid.
    let share_a = mul_div_floor(pool.liquidity_pool_supply, effect.amount_a, pool.reserve_a);
    let share_b = mul_div_floor(pool.liquidity_pool_supply, effect.amount_b, pool.reserve_b);
    assert!(
        effect.delta_lp <= share_a.min(share_b),
        "Payable LP exceeds the pro-rata share of the deposit at the pool's live reserves"
    );

    let post = PoolDefinition {
        liquidity_pool_supply: pool
            .liquidity_pool_supply
            .checked_add(effect.delta_lp)
            .expect("liquidity_pool_supply + delta_lp overflows u128"),
        reserve_a: pool
            .reserve_a
            .checked_add(effect.amount_a)
            .expect("reserve_a + amount_a overflows u128"),
        reserve_b: pool
            .reserve_b
            .checked_add(effect.amount_b)
            .expect("reserve_b + amount_b overflows u128"),
        ..pool
    };
    assert_eq!(
        effect.new_price,
        spot_price_q64_64(post.reserve_a, post.reserve_b),
        "Declared post-add price does not match the pool's new spot price"
    );

    ShardData::from(&post)
}
