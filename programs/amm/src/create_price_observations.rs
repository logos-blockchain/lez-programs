use amm_core::{compute_pool_pda_seed, ConfigFacts, PoolIdentity};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    program::{AccountMeta, ChainedCall},
};
use twap_oracle_core::{compute_current_tick_account_pda, compute_price_observations_pda};

const CONTEXT: &str = "Create price observations";

/// Plan half of `CreatePriceObservations`: check the addresses and build the
/// chained call that makes the oracle claim the feed.
///
/// The pool is the price source, and the AMM proves its control of the pool PDA
/// with the seed on the call — a chained call ships shard selectors, not
/// authorized accounts, so the seed is the whole of the authority.
///
/// The seed tick is still the oracle's own authoritative tick rather than the
/// caller's, but it gets there differently now. It used to be read out of the
/// `CurrentTickAccount`; the plan cannot read anything, so the caller declares it
/// and the guard this instruction emits on the oracle's current-tick shard holds
/// the declaration to that account.
///
/// Two v0.2.5 checks are gone. `config.program_owner == amm_program_id` is
/// structural now (the config guard reads `shards[self]`), and the "observations
/// account must not already exist" assert belongs to the oracle's own
/// `write_once`, which is where emptiness is visible at all.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - `current_tick_account` is not the pool's current-tick PDA.
/// - `price_observations` is not the TWAP PDA for `(pool, window_duration)`.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit config, pool, oracle, and clock accounts"
)]
#[must_use]
pub fn plan_create_price_observations(
    config: &AccountMeta,
    pool: &AccountMeta,
    current_tick_account: &AccountMeta,
    price_observations: &AccountMeta,
    clock: &AccountMeta,
    window_duration: u64,
    facts: &ConfigFacts,
    identity: &PoolIdentity,
    initial_tick: i32,
    now: u64,
    amm_program_id: AccountId,
) -> ChainedCall {
    crate::shared::assert_canonical_clock(clock, CONTEXT);
    crate::shared::assert_pool_in_namespace(
        pool,
        config.account_id,
        identity,
        amm_program_id,
        CONTEXT,
    );

    let twap_oracle_program_id = facts.twap_oracle_program_id;
    assert_eq!(
        current_tick_account.account_id,
        compute_current_tick_account_pda(twap_oracle_program_id, pool.account_id),
        "{CONTEXT}: current tick Account ID does not match PDA"
    );
    assert_eq!(
        price_observations.account_id,
        compute_price_observations_pda(twap_oracle_program_id, pool.account_id, window_duration),
        "{CONTEXT}: price observations Account ID does not match PDA"
    );

    ChainedCall::new(
        twap_oracle_program_id,
        vec![
            ProgramShardSelector::new(price_observations.account_id, twap_oracle_program_id),
            ProgramShardSelector::new(pool.account_id, amm_program_id),
            ProgramShardSelector::from(clock),
        ],
        &twap_oracle_core::Instruction::CreatePriceObservations {
            initial_tick,
            window_duration,
            now,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config.account_id,
        identity.definition_token_a_id,
        identity.definition_token_b_id,
    )])
}

/// Guard on the oracle's current-tick shard: it must hold exactly the declared
/// tick.
///
/// # Panics
/// If the shard is not a valid `CurrentTickAccount`, or its tick differs.
pub fn check_oracle_tick(pre_data: &lee_core::account::ShardData, tick: i32) {
    let stored = twap_oracle_core::CurrentTickAccount::try_from(pre_data)
        .expect("Create price observations: AMM Program expects a valid CurrentTickAccount")
        .tick;
    assert_eq!(
        stored, tick,
        "Declared initial tick does not match the pool's current tick account"
    );
}
