//! Checks and apply halves shared by every instruction.

use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{account::ShardData, program::AccountMeta};

/// Plan half: the clock slot must be the canonical 1-block LEZ clock.
///
/// Enforcing the clock's *identity* is still a plan-phase job; enforcing the
/// *timestamp* is not, and moved to [`check_clock`].
///
/// Identity is now two things, not one. v0.3.0 keys a shard by the program that
/// wrote it, so the timestamp lives in `shards[clock_account_id()]` *at* the
/// `CLOCK_01` address, and a selector naming the right address under some other
/// program would feed [`check_clock`] bytes that program authored. Pinning
/// `now` against those is pinning it against a forgery, so the owning program is
/// asserted here alongside the address.
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

/// Guard on the clock shard: it must read exactly the declared time.
///
/// Exact rather than bounded — see the `now` field on every instruction. A forged
/// or caller-chosen timestamp drives the sampling guard, the accumulator and the
/// consumer-facing freshness signal, so it must be the clock's own value.
///
/// # Panics
/// If the clock shard is empty or disagrees with `now`.
pub fn check_clock(pre_data: &ShardData, now: u64) {
    assert!(!pre_data.is_empty(), "Clock account must be initialized");
    assert_eq!(
        ClockAccountData::from_bytes(pre_data.as_ref()).timestamp,
        now,
        "Declared timestamp does not match the canonical clock"
    );
}

/// Apply half: write a whole record into a shard that must be empty.
///
/// This is what `#[account(init)]` and the old `account == Account::default()`
/// assert become: emptiness is visible only here.
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
