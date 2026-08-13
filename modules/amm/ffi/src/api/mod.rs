//! Transport-independent AMM client operations.

mod admin;
mod config;
mod context;
mod fee;
mod holding;
mod liquidity;
mod pair;
mod quote;
mod request;
mod swap;
mod token_holdings;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt};

pub use request::{
    AddLiquidityPlanRequest, AddLiquidityQuoteRequest, ConfigAccountRequest, ConfigIdRequest,
    CreatePoolPlanRequest, CreatePoolQuoteRequest, FeeTiersRequest, PairIdsRequest, PoolIdRequest,
    ProgramIdRequest, RemoveLiquidityPlanRequest, RemoveLiquidityQuoteRequest, ResolvePoolRequest,
    ResolveTokensRequest, SwapExactInPlanRequest, SwapExactInQuoteRequest, SwapExactOutPlanRequest,
    SwapExactOutQuoteRequest, SwapPairRequest, SyncReservesPlanRequest, TokenHoldingsRequest,
    TransferOwnershipPlanRequest,
};
use serde_json::Value;

pub use crate::account::{AccountRead, WalletAccount};

/// JSON response shared by direct Rust callers and transport adapters.
pub type AmmResponse = Value;

/// Result returned by AMM client operations.
pub type AmmResult = Result<AmmResponse, AmmApiError>;

/// Failure produced before an AMM response can be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmmApiError {
    message: String,
}

impl AmmApiError {
    /// Returns the stable human-readable failure detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AmmApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AmmApiError {}

impl From<String> for AmmApiError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

/// Derives the AMM configuration account ID.
pub fn config_id(request: ConfigIdRequest) -> AmmResult {
    config::config_id(request).map_err(Into::into)
}

/// Decodes the singleton config account (authority + token/twap program ids).
pub fn config_account(request: ConfigAccountRequest) -> AmmResult {
    config::config_account(request).map_err(Into::into)
}

/// Derives canonical accounts for one token pair.
pub fn pair_ids(request: PairIdsRequest) -> AmmResult {
    pair::pair_ids(request).map_err(Into::into)
}

/// Resolves an app-provided set of token ids into liquidity selector rows.
pub fn resolve_tokens(request: ResolveTokensRequest) -> AmmResult {
    context::resolve_tokens(request).map_err(Into::into)
}

/// Derives the canonical account ids for a swap pair (tokens in either order).
pub fn swap_pair(request: SwapPairRequest) -> AmmResult {
    swap::swap_pair(request).map_err(Into::into)
}

/// Decodes a pool account: existence, reserves (canonical order), fee tier.
pub fn resolve_pool(request: ResolvePoolRequest) -> AmmResult {
    swap::resolve_pool(request).map_err(Into::into)
}

/// Derives the pool PDA for a pair — config-free, so a reader needn't load config.
pub fn pool_id(request: PoolIdRequest) -> AmmResult {
    swap::pool_id(request).map_err(Into::into)
}

/// Prices a `SwapExactInput`: expected output, slippage floor, and price impact.
pub fn swap_exact_in_quote(request: SwapExactInQuoteRequest) -> AmmResult {
    swap::swap_exact_in_quote(request).map_err(Into::into)
}

/// Prices a `SwapExactOutput`: required input, slippage ceiling, and price impact.
pub fn swap_exact_out_quote(request: SwapExactOutQuoteRequest) -> AmmResult {
    swap::swap_exact_out_quote(request).map_err(Into::into)
}

/// Builds the `SwapExactInput` wallet submission for a token pair.
pub fn swap_exact_in_plan(request: SwapExactInPlanRequest) -> AmmResult {
    swap::swap_exact_in_plan(request).map_err(Into::into)
}

/// Builds the `SwapExactOutput` wallet submission for a token pair.
pub fn swap_exact_out_plan(request: SwapExactOutPlanRequest) -> AmmResult {
    swap::swap_exact_out_plan(request).map_err(Into::into)
}

/// Prices a create-pool deposit: the LP the creator receives and the opening price.
pub fn create_pool_quote(request: CreatePoolQuoteRequest) -> AmmResult {
    liquidity::create_pool_quote(request).map_err(Into::into)
}

/// Builds the `NewDefinition` submission for creating a pool.
pub fn create_pool_plan(request: CreatePoolPlanRequest) -> AmmResult {
    liquidity::create_pool_plan(request).map_err(Into::into)
}

pub fn add_liquidity_quote(request: AddLiquidityQuoteRequest) -> AmmResult {
    liquidity::add_liquidity_quote(request).map_err(Into::into)
}

pub fn add_liquidity_plan(request: AddLiquidityPlanRequest) -> AmmResult {
    liquidity::add_liquidity_plan(request).map_err(Into::into)
}

pub fn remove_liquidity_quote(request: RemoveLiquidityQuoteRequest) -> AmmResult {
    liquidity::remove_liquidity_quote(request).map_err(Into::into)
}

pub fn remove_liquidity_plan(request: RemoveLiquidityPlanRequest) -> AmmResult {
    liquidity::remove_liquidity_plan(request).map_err(Into::into)
}

pub fn sync_reserves_plan(request: SyncReservesPlanRequest) -> AmmResult {
    liquidity::sync_reserves_plan(request).map_err(Into::into)
}

/// Builds the `UpdateConfig` submission that transfers the AMM's admin authority.
pub fn transfer_ownership_plan(request: TransferOwnershipPlanRequest) -> AmmResult {
    admin::transfer_ownership_plan(request).map_err(Into::into)
}

/// Lists the wallet's fungible token holdings for the account selector.
pub fn token_holdings(request: TokenHoldingsRequest) -> AmmResult {
    token_holdings::token_holdings(request).map_err(Into::into)
}

/// Derives the AMM `ProgramId` (Image ID) from a deployed program binary.
pub fn program_id(request: ProgramIdRequest) -> AmmResult {
    swap::program_id(request).map_err(Into::into)
}

/// Lists the AMM's supported fee tiers (raw bps) from `amm_core` — no inputs.
pub fn fee_tiers(request: FeeTiersRequest) -> AmmResult {
    fee::fee_tiers(request).map_err(Into::into)
}
