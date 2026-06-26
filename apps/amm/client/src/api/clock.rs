use borsh::from_slice;
use clock_core::ClockAccountData;
use nssa_core::account::AccountId;

use crate::account::{decode_account, AccountRead};

pub(super) fn decode_clock(read: &AccountRead) -> Result<(AccountId, ClockAccountData), String> {
    let (id, account) = decode_account(read)?;
    let clock = from_slice(account.data.as_ref())
        .map_err(|error| format!("invalid clock account: {error}"))?;
    Ok((id, clock))
}
