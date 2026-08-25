//! Transport-independent token client operations.

mod decode;
mod plan;
mod request;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt};

pub use decode::{decode_account, decode_definition, decode_holding, decode_metadata};
pub use plan::{
    burn_plan, create_fungible_plan, create_fungible_with_metadata_plan, create_non_fungible_plan,
    initialize_holding_plan, mint_plan, mint_with_authority_plan, print_nft_plan, program_id,
    set_authority_plan, set_authority_with_authority_plan, transfer_plan,
};
pub use request::{
    BurnPlanRequest, CreateFungiblePlanRequest, CreateFungibleWithMetadataPlanRequest,
    CreateNonFungiblePlanRequest, DecodeAccountRequest, DecodeDefinitionRequest,
    DecodeHoldingRequest, DecodeMetadataRequest, InitializeHoldingPlanRequest, MintPlanRequest,
    MintWithAuthorityPlanRequest, PrintNftPlanRequest, ProgramIdRequest, SetAuthorityPlanRequest,
    SetAuthorityWithAuthorityPlanRequest, TransferPlanRequest,
};
use serde_json::Value;

use crate::account::parse_program_id;

pub type TokenResponse = Value;
pub type TokenResult = Result<TokenResponse, TokenApiError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenApiError {
    code: &'static str,
}

impl TokenApiError {
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for TokenApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl Error for TokenApiError {}

fn parse_token_program_id(value: &str) -> Result<lee_core::program::ProgramId, TokenApiError> {
    parse_program_id(value).map_err(|_| TokenApiError::new("bad_request"))
}
