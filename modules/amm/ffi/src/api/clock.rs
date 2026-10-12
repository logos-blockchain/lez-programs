//! The clock read every plan that chains into the TWAP oracle now needs.
//!
//! `now` is a declared instruction field in v0.3.0: the AMM forwards it to the
//! oracle's `UpdateCurrentTick`, and the oracle's own clock guard pins it to the
//! canonical `CLOCK_01` shard. So the client-side read below is what *makes* the
//! declaration, and the guard on-chain is what makes it true -- a plan phase can
//! no longer read the clock for itself.

use borsh::from_slice;
use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};

use crate::account::{program_shard, AccountRead};

/// The canonical clock's timestamp in milliseconds.
pub(super) fn now_from_clock(read: &AccountRead) -> Result<u64, String> {
    let (account_id, shard) = program_shard(read, clock_account_id(), "clock")?;
    if account_id != CLOCK_01_PROGRAM_ACCOUNT_ID {
        return Err(String::from("clock is not the canonical 1-block LEZ clock"));
    }
    let clock = from_slice::<ClockAccountData>(shard.as_ref())
        .map_err(|_| String::from("clock shard is not a ClockAccountData"))?;
    Ok(clock.timestamp)
}

#[cfg(test)]
pub(super) fn clock_read(timestamp: u64) -> AccountRead {
    crate::account::account_read(
        CLOCK_01_PROGRAM_ACCOUNT_ID,
        clock_account_id(),
        &lee_core::account::ShardData::try_from(
            borsh::to_vec(&ClockAccountData {
                block_id: 1,
                timestamp,
            })
            .expect("ClockAccountData serializes"),
        )
        .expect("a clock shard fits"),
    )
}
