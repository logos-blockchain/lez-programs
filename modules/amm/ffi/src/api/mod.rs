//! Transport-independent AMM client operations.

mod accounts;
mod clock;
mod commitment;
mod config;
mod context;
mod funding;
mod holding;
mod liquidity;
mod pair;
mod plan;
mod position;
mod quote;
mod quote_error;
mod request;
mod swap;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt};

pub use request::{
    ConfigIdRequest, ContextRequest, CreatePoolPlanRequest, LiquidityQuoteRequest, PairIdsRequest,
    PairSnapshot, PlanRequest, PoolIdRequest, PositionRequest, ProgramIdRequest, QuoteRequest,
    ResolvePoolRequest, SwapExactInPlanRequest, SwapExactInQuoteRequest, SwapExactOutPlanRequest,
    SwapExactOutQuoteRequest, SwapPairRequest, TokenIdsRequest,
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

/// Discovers token definition IDs available to the active wallet and app.
pub fn token_ids(request: TokenIdsRequest) -> AmmResult {
    context::token_ids(request).map_err(Into::into)
}

/// Derives canonical accounts for one token pair.
pub fn pair_ids(request: PairIdsRequest) -> AmmResult {
    pair::pair_ids(request).map_err(Into::into)
}

/// Builds network, token, holding, and fee-tier context.
pub fn context(request: ContextRequest) -> AmmResult {
    context::context(request).map_err(Into::into)
}

/// Evaluates a pool-creation or add-liquidity request.
pub fn quote(request: QuoteRequest) -> AmmResult {
    quote::quote(request).map_err(Into::into)
}

/// Materializes a previously quoted request into wallet submission arguments.
pub fn plan(request: PlanRequest) -> AmmResult {
    plan::plan(request).map_err(Into::into)
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
pub fn liquidity_quote(request: LiquidityQuoteRequest) -> AmmResult {
    liquidity::liquidity_quote(request).map_err(Into::into)
}

/// Builds the `NewDefinition` submission for creating a pool.
pub fn create_pool_plan(request: CreatePoolPlanRequest) -> AmmResult {
    liquidity::create_pool_plan(request).map_err(Into::into)
}

/// Derives the AMM `ProgramId` (Image ID) from a deployed program binary.
pub fn program_id(request: ProgramIdRequest) -> AmmResult {
    swap::program_id(request).map_err(Into::into)
}
