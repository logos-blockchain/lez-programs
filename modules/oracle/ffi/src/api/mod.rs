//! Typed discovery, transaction planning, and shared validation; no wallet or
//! network access.

pub mod numeric;
pub mod plan;
pub mod price;
mod program;
pub mod twap;

use std::{error::Error, fmt};

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{account::AccountId, program::ProgramId};
pub use plan::{
    create_current_tick_account, create_oracle_price_account, create_price_observations,
    publish_price, record_tick, update_current_tick, CreateCurrentTickAccountRequest,
    CreateOraclePriceAccountRequest, CreatePriceObservationsRequest, FeedMaintenanceRequest,
    UpdateCurrentTickRequest,
};
pub use program::program_info;
use serde::Deserialize;
use serde_json::Value;
pub use twap::{feed_addresses, TransactionPlan};

use crate::account::{account_id_from_hex, decode_account, parse_program_id, AccountRead};

pub type OracleResult = Result<Value, OracleApiError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleApiError(&'static str);

impl OracleApiError {
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    pub const fn code(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for OracleApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for OracleApiError {}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramInfoRequest {
    /// The program's ImageID as 64 lowercase hex characters.
    pub oracle_program_id: Option<String>,
    /// Hex-encoded deployable RISC Zero binary; its ImageID is the program id.
    pub elf: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedAddressesRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub window_duration_ms: String,
}

pub(crate) fn parse_id(value: &str, error: &'static str) -> Result<AccountId, OracleApiError> {
    let id = account_id_from_hex(value, "account id").map_err(|_| OracleApiError::new(error))?;
    if id == AccountId::new([0; 32]) {
        return Err(OracleApiError::new(error));
    }
    Ok(id)
}

pub(crate) fn parse_oracle_program_id(value: &str) -> Result<ProgramId, OracleApiError> {
    let id = parse_program_id(value).map_err(|_| OracleApiError::new("invalid_program_id"))?;
    if id == ProgramId::default() {
        return Err(OracleApiError::new("invalid_program_id"));
    }
    Ok(id)
}

/// Checks the canonical clock address and exact Borsh layout. A transport failure
/// stays distinct from a readable account with an invalid identity or payload.
pub fn validate_clock(read: &AccountRead) -> Result<ClockAccountData, OracleApiError> {
    let (id, account) =
        decode_account(read).map_err(|_| OracleApiError::new("account_read_failed"))?;
    if id != CLOCK_01_PROGRAM_ACCOUNT_ID {
        return Err(OracleApiError::new("invalid_clock"));
    }
    borsh::from_slice(account.data.as_ref()).map_err(|_| OracleApiError::new("invalid_clock"))
}
