//! Producer-independent canonical price validation. No TWAP PDAs, ring state,
//! tick math, or deployment selection are involved here. Consumer policy and
//! public price-query envelopes are added by the canonical-read API.

use lee_core::account::{Account, AccountId};
pub use twap_oracle_core::{OraclePriceAccount, PRICE_FRACTIONAL_BITS};

use crate::{decode_account, AccountRead};

/// Typed internal outcomes; these are not yet public module error codes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PriceAccountError {
    ReadFailed,
    IdMismatch,
    NotFound,
    OwnerMismatch,
    InvalidData,
}

/// Reads exactly the canonical account layout, including its discriminator.
/// Preserves zero prices and old timestamps; usability is a consumer policy.
pub fn validate_price_account(
    read: &AccountRead,
    expected_id: AccountId,
    expected_owner: AccountId,
) -> Result<OraclePriceAccount, PriceAccountError> {
    let (id, account) = decode_account(read).map_err(|_| PriceAccountError::ReadFailed)?;
    if id != expected_id {
        return Err(PriceAccountError::IdMismatch);
    }
    if account == Account::default() {
        return Err(PriceAccountError::NotFound);
    }
    if account.program_owner != expected_owner {
        return Err(PriceAccountError::OwnerMismatch);
    }
    OraclePriceAccount::try_from(&account.data).map_err(|_| PriceAccountError::InvalidData)
}
