//! Producer-independent canonical price validation. No TWAP PDAs, ring state,
//! tick math, or deployment selection are involved here. Consumer policy and
//! public price-query envelopes are added by the canonical-read API.

use lee_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
pub use twap_oracle_core::{OraclePriceAccount, PRICE_FRACTIONAL_BITS};

use super::{
    parse_id, parse_owner_program_id, program::base58_program_id, twap::insert_id, OracleApiError,
    OracleResult,
};
use crate::account::{decode_account, program_id_hex, AccountRead};

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
    expected_owner: ProgramId,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriceAccountRequest {
    /// The canonical price account address as 64 lowercase hex characters.
    pub price_account_id: String,
    /// The expected owning program (ImageID) as 64 lowercase hex characters.
    pub expected_owner_id: String,
    pub read: AccountRead,
}

/// Returns the stored snapshot verbatim, without consumer policy: zero prices
/// and stale timestamps are data, not errors. Every number crosses the API as
/// an exact decimal string, except the constant fractional-bit count.
pub fn price_account(request: PriceAccountRequest) -> OracleResult {
    let expected_id = parse_id(&request.price_account_id, "invalid_account_id")?;
    let expected_owner = parse_owner_program_id(&request.expected_owner_id)?;
    let price = validate_price_account(&request.read, expected_id, expected_owner).map_err(
        |error| {
            OracleApiError::new(match error {
                PriceAccountError::ReadFailed => "account_read_failed",
                PriceAccountError::IdMismatch => "price_account_id_mismatch",
                PriceAccountError::NotFound => "not_found",
                PriceAccountError::OwnerMismatch => "oracle_owner_mismatch",
                PriceAccountError::InvalidData => "invalid_account_data",
            })
        },
    )?;
    let mut result = Map::new();
    insert_id(&mut result, "priceAccountId", expected_id);
    result.insert(
        "ownerId".to_owned(),
        json!(base58_program_id(expected_owner)),
    );
    result.insert(
        "ownerIdHex".to_owned(),
        json!(program_id_hex(expected_owner)),
    );
    insert_id(&mut result, "baseAssetId", price.base_asset);
    insert_id(&mut result, "quoteAssetId", price.quote_asset);
    insert_id(&mut result, "sourceId", price.source_id);
    result.insert("price".to_owned(), json!(price.price.to_string()));
    result.insert(
        "timestampMs".to_owned(),
        json!(price.timestamp.to_string()),
    );
    result.insert(
        "confidenceInterval".to_owned(),
        json!(price.confidence_interval.to_string()),
    );
    result.insert(
        "priceFractionalBits".to_owned(),
        json!(PRICE_FRACTIONAL_BITS),
    );
    Ok(Value::Object(result))
}
