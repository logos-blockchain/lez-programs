//! Producer-independent canonical price validation. No TWAP PDAs, ring state,
//! tick math, or deployment selection are involved here. Consumer policy and
//! public price-query envelopes are added by the canonical-read API.

use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
pub use twap_oracle_core::{OraclePriceAccount, PRICE_FRACTIONAL_BITS};

use super::{
    numeric::parse_u64, parse_id, parse_owner_program_id, program::base58_program_id,
    twap::insert_id, validate_clock, OracleApiError, OracleResult,
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

fn map_price_error(error: PriceAccountError) -> OracleApiError {
    OracleApiError::new(match error {
        PriceAccountError::ReadFailed => "account_read_failed",
        PriceAccountError::IdMismatch => "price_account_id_mismatch",
        PriceAccountError::NotFound => "not_found",
        PriceAccountError::OwnerMismatch => "oracle_owner_mismatch",
        PriceAccountError::InvalidData => "invalid_account_data",
    })
}

/// The full canonical snapshot shared by the raw and validated reads. Every
/// number crosses the API as an exact decimal string, except the constant
/// fractional-bit count.
fn price_snapshot(
    price: &OraclePriceAccount,
    expected_id: AccountId,
    expected_owner: ProgramId,
) -> Map<String, Value> {
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
    result.insert("timestampMs".to_owned(), json!(price.timestamp.to_string()));
    result.insert(
        "confidenceInterval".to_owned(),
        json!(price.confidence_interval.to_string()),
    );
    result.insert(
        "priceFractionalBits".to_owned(),
        json!(PRICE_FRACTIONAL_BITS),
    );
    result
}

/// Returns the stored snapshot verbatim, without consumer policy: zero prices
/// and stale timestamps are data, not errors.
pub fn price_account(request: PriceAccountRequest) -> OracleResult {
    let expected_id = parse_id(&request.price_account_id, "invalid_account_id")?;
    let expected_owner = parse_owner_program_id(&request.expected_owner_id)?;
    let price = validate_price_account(&request.read, expected_id, expected_owner)
        .map_err(map_price_error)?;
    Ok(Value::Object(price_snapshot(
        &price,
        expected_id,
        expected_owner,
    )))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryPriceRequest {
    /// The canonical price account address as 64 lowercase hex characters.
    pub price_account_id: String,
    /// The expected owning program (ImageID) as 64 lowercase hex characters.
    pub expected_owner_id: String,
    /// The base asset the caller prices; must match the stored snapshot.
    pub base_asset_id: String,
    /// The quote asset denominating the price; must match the stored snapshot.
    pub quote_asset_id: String,
    /// The source the caller trusts; must match the stored snapshot.
    pub expected_source_id: String,
    /// Freshness limit; the stored timestamp's age must not exceed it.
    pub max_age_ms: String,
    pub read: AccountRead,
    /// The canonical CLOCK_01 snapshot fetched by the host for this call.
    pub clock_read: AccountRead,
}

/// Applies the caller's consumer policy to the stored snapshot: expected asset
/// pair and source, nonzero price and timestamp, a stored timestamp no later
/// than the canonical clock, and an age within the caller's limit. An age
/// exactly at the limit is valid. Never returns a usable price after
/// validation fails.
pub fn query_price(request: QueryPriceRequest) -> OracleResult {
    let expected_id = parse_id(&request.price_account_id, "invalid_account_id")?;
    let expected_owner = parse_owner_program_id(&request.expected_owner_id)?;
    let base = parse_id(&request.base_asset_id, "invalid_account_id")?;
    let quote = parse_id(&request.quote_asset_id, "invalid_account_id")?;
    let source = parse_id(&request.expected_source_id, "invalid_account_id")?;
    let max_age = parse_u64(&request.max_age_ms)?;
    let price = validate_price_account(&request.read, expected_id, expected_owner)
        .map_err(map_price_error)?;
    if price.base_asset != base || price.quote_asset != quote {
        return Err(OracleApiError::new("oracle_asset_mismatch"));
    }
    if price.source_id != source {
        return Err(OracleApiError::new("oracle_source_mismatch"));
    }
    if price.price == 0 {
        return Err(OracleApiError::new("oracle_price_zero"));
    }
    if price.timestamp == 0 {
        return Err(OracleApiError::new("oracle_timestamp_zero"));
    }
    let clock = validate_clock(&request.clock_read)?;
    if price.timestamp > clock.timestamp {
        return Err(OracleApiError::new("oracle_future_timestamp"));
    }
    let age = clock.timestamp - price.timestamp;
    if age > max_age {
        return Err(OracleApiError::new("oracle_stale"));
    }
    let mut result = price_snapshot(&price, expected_id, expected_owner);
    result.insert("checkedAtMs".to_owned(), json!(clock.timestamp.to_string()));
    result.insert("ageMs".to_owned(), json!(age.to_string()));
    Ok(Value::Object(result))
}

#[derive(Clone, Debug)]
pub struct ClockAddressRequest {}

/// Deserializes only from the exact empty object `{}`. Derived deserialization
/// would also accept the positional sequence form (`[]`), which the wire
/// contract forbids: requests are always objects.
impl<'de> Deserialize<'de> for ClockAddressRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Empty;
        impl<'de> serde::de::Visitor<'de> for Empty {
            type Value = ClockAddressRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("exactly an empty object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                if map.next_key::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom("unexpected field"));
                }
                Ok(ClockAddressRequest {})
            }
        }
        deserializer.deserialize_map(Empty)
    }
}

/// The canonical CLOCK_01 address, independent of any program selection: the
/// host fetches and internally validates the clock account for validated
/// reads, so reads work without TWAP configuration.
pub fn clock_address(_request: ClockAddressRequest) -> OracleResult {
    let mut result = Map::new();
    insert_id(&mut result, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);
    Ok(Value::Object(result))
}
