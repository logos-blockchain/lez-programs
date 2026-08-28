//! Transport-independent stablecoin client operations.

mod decode;
mod plan;
mod position;
mod program;
mod request;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt};

pub use decode::decode_protocol_parameters;
pub use plan::initialize_program_plan;
pub use position::{decode_position, position_info};
pub use program::program_info;
pub use request::{
    DecodePositionRequest, DecodeProtocolParametersRequest, InitializeProgramPlanRequest,
    PositionInfoRequest, ProgramInfoRequest,
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
) -> Result<lee_core::program::ProgramId, StablecoinApiError> {
    let program_id =
        parse_program_id(value).map_err(|_| StablecoinApiError::new("invalid_program_id"))?;
    if program_id == [0_u32; 8] {
        return Err(StablecoinApiError::new("invalid_program_id"));
    }
    Ok(program_id)
}
