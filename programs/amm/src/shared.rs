//! Plan-phase checks and apply halves shared across the instruction set.

use amm_core::{check_pool_identity, compute_pool_pda, PoolDefinition, PoolIdentity};
use clock_core::{clock_account_id, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};

/// Plan half: the clock slot must be the canonical 1-block LEZ clock.
///
/// The AMM itself never reads the clock any more — it only forwards `now` to the
/// oracle, whose own guard pins it to this shard. Asserting it here still earns
/// its place: it rejects a wrong clock with an AMM-level error before any chained
/// call runs.
///
/// Both halves of the identity matter, and the program half is new. v0.3.0 keys a
/// shard by the program that wrote it, so the clock's timestamp lives in
/// `shards[clock_account_id()]` *at* the `CLOCK_01` address. A selector naming the
/// right address under some other program would hand the oracle's guard bytes
/// that program authored — a forged timestamp wearing the clock's address — so
/// the address alone is no longer the whole check.
///
/// # Panics
/// If `clock` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`], or does not select the
/// clock program's shard of it.
pub fn assert_canonical_clock(clock: &AccountMeta, context: &str) {
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "{context}: clock account must be the canonical 1-block LEZ clock account"
    );
    assert_eq!(
        clock.program_account_id,
        clock_account_id(),
        "{context}: clock account must select the clock program's shard"
    );
}

/// Plan half: the pool must be derived under this config's namespace.
///
/// `config_id` is the namespace root, so a pool belonging to another instance —
/// even a genuine AMM pool over the same token pair — derives a different PDA and
/// is rejected here. The pair it derives from is declared rather than read, but
/// the pool's own effect pins the declaration to the pool shard, so a caller
/// cannot satisfy this check by naming a pair the pool does not hold.
///
/// # Panics
/// If `pool` is not the pool PDA for `(config_id, declared pair)`.
pub fn assert_pool_in_namespace(
    pool: &AccountMeta,
    config_id: AccountId,
    identity: &PoolIdentity,
    amm_program_id: AccountId,
    context: &str,
) {
    assert_eq!(
        pool.account_id,
        compute_pool_pda(
            amm_program_id,
            config_id,
            identity.definition_token_a_id,
            identity.definition_token_b_id,
        ),
        "{context}: pool account is not derived under this config's namespace"
    );
}

/// Plan half: the two vault slots must be the pool's own vaults.
///
/// Redundant with the identity pin in the strict sense — the vault ids come from
/// the same declaration — but it is what ties the *slots the caller filled* to
/// that declaration, and a mismatch here is an AMM-level error instead of a
/// token-program one three calls deep.
///
/// # Panics
/// If either slot names something other than the declared vault for its side.
pub fn assert_vault_slots(
    vault_a: &AccountMeta,
    vault_b: &AccountMeta,
    identity: &PoolIdentity,
    context: &str,
) {
    assert_eq!(
        vault_a.account_id, identity.vault_a_id,
        "{context}: Vault A was not provided"
    );
    assert_eq!(
        vault_b.account_id, identity.vault_b_id,
        "{context}: Vault B was not provided"
    );
}

/// Apply half: decode the pool and pin the declared identity to it.
///
/// Every effect on a pool shard starts here, so no handler acts on a declared id
/// the pool has not confirmed.
///
/// # Panics
/// If the shard is not a valid [`PoolDefinition`], or the identity differs.
pub fn pool_of(pre_data: &ShardData, identity: &PoolIdentity, context: &str) -> PoolDefinition {
    let pool = PoolDefinition::try_from(pre_data).unwrap_or_else(|_| {
        panic!("{context}: AMM Program expects a valid Pool Definition Account")
    });
    check_pool_identity(&pool, identity);
    pool
}

/// Apply half: write a whole record into a shard that must be empty.
///
/// What `#[account(init)]` and the old `account == Account::default()` assert
/// become: emptiness is visible only here.
///
/// # Panics
/// If the shard already holds data.
#[must_use]
pub fn create(pre_data: &ShardData, data: ShardData) -> ShardData {
    assert!(
        pre_data.is_empty(),
        "Target account must not already hold data"
    );
    data
}
