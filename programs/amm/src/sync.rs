use amm_core::{
    compute_pool_pda_seed, spot_price_q64_64, ConfigFacts, PoolDefinition, SyncReserves,
    MINIMUM_LIQUIDITY,
};
use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use token_core::{TokenDescriptor, TokenKind};
use twap_oracle_core::compute_current_tick_account_pda;

const CONTEXT: &str = "Sync reserves";

/// Plan half of `SyncReserves`: check the addresses and chain the tick refresh at
/// the synced price.
///
/// This is the one instruction whose purpose *is* to read the vaults, so unlike
/// its siblings it cannot drop that read — it declares both balances and pins
/// each with a guard on its own Token Program shard
/// ([`amm_core::check_vault_balance`]). Everything derived from them, the synced
/// price included, then follows from pinned numbers.
///
/// Returns `(descriptor_a, descriptor_b, calls)`: the two descriptors are what the
/// vault guards assert the holdings against.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - either vault slot is not the pool's own.
/// - `current_tick_account` is not the pool's current-tick PDA.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit config, pool, vault, oracle, and clock accounts"
)]
#[must_use]
pub fn plan_sync_reserves(
    config: &AccountMeta,
    pool: &AccountMeta,
    vault_a: &AccountMeta,
    vault_b: &AccountMeta,
    current_tick_account: &AccountMeta,
    clock: &AccountMeta,
    facts: &ConfigFacts,
    effect: &SyncReserves,
    now: u64,
    amm_program_id: AccountId,
) -> (TokenDescriptor, TokenDescriptor, Vec<ChainedCall>) {
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
        current_tick_account.account_id,
        compute_current_tick_account_pda(facts.twap_oracle_program_id, pool.account_id),
        "{CONTEXT}: current tick Account ID does not match PDA"
    );

    let descriptor_a = TokenDescriptor {
        definition_id: identity.definition_token_a_id,
        kind: TokenKind::Fungible,
    };
    let descriptor_b = TokenDescriptor {
        definition_id: identity.definition_token_b_id,
        kind: TokenKind::Fungible,
    };

    // Both balances are pinned to their vaults by the guards this emits, so the
    // price derived from them is the pool's own synced price, not a caller's.
    let new_price = spot_price_q64_64(effect.vault_a_balance, effect.vault_b_balance);
    let call_update_tick = ChainedCall::new(
        facts.twap_oracle_program_id,
        vec![
            ProgramShardSelector::new(
                current_tick_account.account_id,
                facts.twap_oracle_program_id,
            ),
            ProgramShardSelector::new(pool.account_id, amm_program_id),
            ProgramShardSelector::from(clock),
        ],
        &twap_oracle_core::Instruction::UpdateCurrentTick {
            price: new_price,
            now,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config.account_id,
        identity.definition_token_a_id,
        identity.definition_token_b_id,
    )]);

    (descriptor_a, descriptor_b, vec![call_update_tick])
}

/// Apply half of `SyncReserves`: set the reserves to the pinned vault balances.
///
/// Reserves may lag a vault but must never exceed it, so a balance below its
/// reserve is refused rather than written: that direction means funds the pool
/// thought it had are gone, which is a broken invariant to investigate and not a
/// state to normalize.
///
/// # Panics
/// Panics if:
/// - the shard is not a valid pool, or the declared identity differs.
/// - the pool's LP supply is below [`MINIMUM_LIQUIDITY`].
/// - either declared balance is below its current reserve.
#[must_use]
pub fn sync_reserves(pre_data: &ShardData, effect: &SyncReserves) -> ShardData {
    let pool = crate::shared::pool_of(pre_data, &effect.identity, CONTEXT);
    assert!(
        pool.liquidity_pool_supply >= MINIMUM_LIQUIDITY,
        "Pool liquidity supply is below minimum liquidity"
    );
    assert!(
        effect.vault_a_balance >= pool.reserve_a,
        "{CONTEXT}: vault A balance is less than its reserve"
    );
    assert!(
        effect.vault_b_balance >= pool.reserve_b,
        "{CONTEXT}: vault B balance is less than its reserve"
    );

    ShardData::from(&PoolDefinition {
        reserve_a: effect.vault_a_balance,
        reserve_b: effect.vault_b_balance,
        ..pool
    })
}
