use amm_core::{compute_pool_pda_seed, spot_price_q64_64, CheckPoolPrice, ConfigFacts};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    program::{AccountMeta, ChainedCall},
};
use twap_oracle_core::{compute_oracle_price_account_pda, OBSERVATIONS_CAPACITY};

const CONTEXT: &str = "Create oracle price account";

/// Plan half of `CreateOraclePriceAccount`: check the addresses, derive the seed
/// price from the pool's declared reserves, and build the chained call.
///
/// Mirrors [`plan_create_price_observations`](crate::create_price_observations::
/// plan_create_price_observations): the pool is the price source, authorized by
/// its PDA seed on the call.
///
/// Neither the asset pair nor the initial price is caller-chosen. The pair and
/// the reserves are declared and pinned by [`CheckPoolPrice`] on the pool shard,
/// and the price is derived here from the pinned reserves — so what reaches the
/// oracle is still the pool's own spot price, computed from the pool's own
/// numbers.
///
/// # Panics
/// Panics if:
/// - `clock` is not the canonical 1-block LEZ clock shard.
/// - `pool` is not the pool PDA for `(config, declared pair)`.
/// - `oracle_price_account` is not the TWAP PDA for `(pool, window_duration)`.
/// - `window_duration` is below [`OBSERVATIONS_CAPACITY`], which would leave the price account with
///   no `PriceObservations` feed able to update it.
/// - the declared reserves give a zero spot price; zero is the no-price sentinel, so the account
///   must never be seeded with it.
#[expect(
    clippy::too_many_arguments,
    reason = "instruction surface passes explicit config, pool, oracle, and clock accounts"
)]
#[must_use]
pub fn plan_create_oracle_price_account(
    config: &AccountMeta,
    pool: &AccountMeta,
    oracle_price_account: &AccountMeta,
    clock: &AccountMeta,
    window_duration: u64,
    facts: &ConfigFacts,
    guard: &CheckPoolPrice,
    now: u64,
    amm_program_id: AccountId,
) -> ChainedCall {
    crate::shared::assert_canonical_clock(clock, CONTEXT);
    crate::shared::assert_pool_in_namespace(
        pool,
        config.account_id,
        &guard.identity,
        amm_program_id,
        CONTEXT,
    );

    // A window smaller than the observations capacity can never have a matching
    // PriceObservations account, so PublishPrice could never update this price
    // account. The oracle enforces the same bound; this is the early error.
    assert!(
        window_duration >= u64::from(OBSERVATIONS_CAPACITY),
        "{CONTEXT}: window_duration must be >= OBSERVATIONS_CAPACITY so a matching \
         PriceObservations account can exist and PublishPrice can update this price account"
    );

    let twap_oracle_program_id = facts.twap_oracle_program_id;
    assert_eq!(
        oracle_price_account.account_id,
        compute_oracle_price_account_pda(twap_oracle_program_id, pool.account_id, window_duration),
        "{CONTEXT}: oracle price Account ID does not match PDA"
    );

    let initial_price = spot_price_q64_64(guard.reserve_a, guard.reserve_b);
    assert!(
        initial_price != 0,
        "{CONTEXT}: pool spot price must be non-zero (zero is the no-price sentinel; pool \
         reserve_b is zero or negligible relative to reserve_a)"
    );

    ChainedCall::new(
        twap_oracle_program_id,
        vec![
            ProgramShardSelector::new(oracle_price_account.account_id, twap_oracle_program_id),
            ProgramShardSelector::new(pool.account_id, amm_program_id),
            ProgramShardSelector::from(clock),
        ],
        &twap_oracle_core::Instruction::CreateOraclePriceAccount {
            base_asset: guard.identity.definition_token_a_id,
            quote_asset: guard.identity.definition_token_b_id,
            initial_price,
            window_duration,
            now,
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        config.account_id,
        guard.identity.definition_token_a_id,
        guard.identity.definition_token_b_id,
    )])
}

/// Guard on the pool shard: the declared identity *and* reserves must be the
/// stored ones.
///
/// Exact on the reserves, unlike the bounded pins the mutating instructions use.
/// A price account is seeded once and then read by consumers, so a
/// stale-by-one-swap seed is not acceptable; the cost is a retry when the pool
/// moved under the call.
///
/// # Panics
/// If the shard is not a valid pool, or any declared field differs.
pub fn check_pool_price(pre_data: &lee_core::account::ShardData, guard: &CheckPoolPrice) {
    let pool = crate::shared::pool_of(pre_data, &guard.identity, CONTEXT);
    assert_eq!(
        (pool.reserve_a, pool.reserve_b),
        (guard.reserve_a, guard.reserve_b),
        "Declared pool reserves do not match the pool account"
    );
}
