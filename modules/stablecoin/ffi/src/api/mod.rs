//! Transport-independent stablecoin client operations.

mod decode;
mod plan;
mod program;
mod request;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt};

pub use decode::decode_protocol_parameters;
pub use plan::initialize_program_plan;
pub use program::program_info;
pub use request::{
    DecodeProtocolParametersRequest, InitializeProgramPlanRequest, ProgramInfoRequest,
};
use serde_json::Value;

use crate::account::parse_program_id;

pub type StablecoinResponse = Value;
pub type StablecoinResult = Result<StablecoinResponse, StablecoinApiError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StablecoinApiError {
    code: &'static str,
}

impl StablecoinApiError {
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for StablecoinApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl Error for StablecoinApiError {}

fn parse_stablecoin_program_id(
    value: &str,
) -> Result<lee_core::account::AccountId, StablecoinApiError> {
    let program_id =
        parse_program_id(value).map_err(|_| StablecoinApiError::new("invalid_program_id"))?;
    // The all-zero id is `NATIVE_TOKEN_PROGRAM_ID`, the program whose shard holds
    // every account's balance -- a real program, and never this one. v0.2.5 rejected
    // the same value as `DEFAULT_PROGRAM_OWNER` ("no program owns this"), which is
    // gone along with ownership itself.
    if program_id == lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID {
        return Err(StablecoinApiError::new("invalid_program_id"));
    }
    Ok(program_id)
}
