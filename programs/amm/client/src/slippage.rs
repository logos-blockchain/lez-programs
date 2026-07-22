//! Integer-only construction of AMM instruction guards from validated quotes.

use amm_core::{checked_mul_div_ceil, checked_mul_div_floor, FEE_BPS_DENOMINATOR};
use amm_program::quote::{AddLiquidityQuote, CreatePoolQuote, RemoveLiquidityQuote, SwapQuote};

use crate::{
    quote::{
        self as client_quote, ValidatedFungibleDefinition, ValidatedFungibleHolding,
        ValidatedPoolSnapshot,
    },
    AmmContext, ClientError,
};

/// Denominator used by client slippage tolerances.
///
/// This aliases the program's canonical basis-point denominator so wire consumers do not maintain
/// a separate numeric convention.
pub const SLIPPAGE_BPS_DENOMINATOR: u128 = FEE_BPS_DENOMINATOR;

/// Validated price-movement tolerance in basis points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlippageTolerance {
    bps: u128,
}

impl SlippageTolerance {
    /// Creates a tolerance between zero and 10,000 basis points, inclusive.
    pub fn new(bps: u128) -> Result<Self, ClientError> {
        if bps > SLIPPAGE_BPS_DENOMINATOR {
            return Err(ClientError::SlippageToleranceOutOfRange {
                bps,
                maximum_bps: SLIPPAGE_BPS_DENOMINATOR,
            });
        }
        Ok(Self { bps })
    }

    /// Returns the exact basis-point value.
    #[must_use]
    pub const fn bps(self) -> u128 {
        self.bps
    }
}

/// Builds a conservative minimum chain guard with integer floor rounding.
///
/// Positive quotes are clamped to one raw unit because AMM liquidity instructions reject zero
/// minimums and a one-unit quote has no smaller executable guard. A zero quote remains zero.
pub fn minimum_guard_amount(
    quoted_amount: u128,
    tolerance: SlippageTolerance,
) -> Result<u128, ClientError> {
    let retained_bps = SLIPPAGE_BPS_DENOMINATOR.checked_sub(tolerance.bps).ok_or(
        ClientError::SlippageToleranceOutOfRange {
            bps: tolerance.bps,
            maximum_bps: SLIPPAGE_BPS_DENOMINATOR,
        },
    )?;
    let guard = checked_mul_div_floor(quoted_amount, retained_bps, SLIPPAGE_BPS_DENOMINATOR)
        .ok_or(ClientError::SlippageBoundOverflow {
            quoted_amount,
            slippage_bps: tolerance.bps,
        })?;

    Ok(if quoted_amount == 0 { 0 } else { guard.max(1) })
}

/// Builds a conservative maximum chain guard with integer ceil rounding.
pub fn maximum_guard_amount(
    quoted_amount: u128,
    tolerance: SlippageTolerance,
) -> Result<u128, ClientError> {
    let expanded_bps = SLIPPAGE_BPS_DENOMINATOR.checked_add(tolerance.bps).ok_or(
        ClientError::SlippageBoundOverflow {
            quoted_amount,
            slippage_bps: tolerance.bps,
        },
    )?;
    checked_mul_div_ceil(quoted_amount, expanded_bps, SLIPPAGE_BPS_DENOMINATOR).ok_or(
        ClientError::SlippageBoundOverflow {
            quoted_amount,
            slippage_bps: tolerance.bps,
        },
    )
}

/// Pool-creation quote plus exact `NewDefinition` amount fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedCreatePool {
    pub quote: CreatePoolQuote,
    pub token_a_amount: u128,
    pub token_b_amount: u128,
    pub fees: u128,
}

/// Add-liquidity quote plus slippage-safe `AddLiquidity` amount fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedAddLiquidity {
    pub quote: AddLiquidityQuote,
    pub min_amount_liquidity: u128,
    pub max_amount_to_add_token_a: u128,
    pub max_amount_to_add_token_b: u128,
}

/// Remove-liquidity quote plus slippage-safe `RemoveLiquidity` amount fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedRemoveLiquidity {
    pub quote: RemoveLiquidityQuote,
    pub remove_liquidity_amount: u128,
    pub min_amount_to_remove_token_a: u128,
    pub min_amount_to_remove_token_b: u128,
}

/// Exact-input quote plus slippage-safe `SwapExactInput` amount fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedSwapExactInput {
    pub quote: SwapQuote,
    pub swap_amount_in: u128,
    pub min_amount_out: u128,
}

/// Exact-output quote plus slippage-safe `SwapExactOutput` amount fields.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedSwapExactOutput {
    pub quote: SwapQuote,
    pub exact_amount_out: u128,
    pub max_amount_in: u128,
}

/// Quotes pool creation and returns the exact instruction amount fields.
pub fn prepare_create_pool(
    context: &AmmContext,
    token_a: &ValidatedFungibleDefinition,
    token_b: &ValidatedFungibleDefinition,
    token_a_amount: u128,
    token_b_amount: u128,
    fee_bps: u128,
) -> Result<PreparedCreatePool, ClientError> {
    let quote = client_quote::create_pool(
        context,
        token_a,
        token_b,
        token_a_amount,
        token_b_amount,
        fee_bps,
    )?;
    Ok(PreparedCreatePool {
        quote,
        token_a_amount: quote.pool.reserve_a,
        token_b_amount: quote.pool.reserve_b,
        fees: fee_bps,
    })
}

/// Quotes add liquidity and derives its minimum-LP guard.
pub fn prepare_add_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    max_amount_a: u128,
    max_amount_b: u128,
    tolerance: SlippageTolerance,
) -> Result<PreparedAddLiquidity, ClientError> {
    let preview = client_quote::preview_add_liquidity(snapshot, max_amount_a, max_amount_b)?;
    let min_amount_liquidity = minimum_guard_amount(preview.liquidity_to_mint, tolerance)?;
    let max_amount_to_add_token_a = preview.actual_amount_a;
    let max_amount_to_add_token_b = preview.actual_amount_b;
    let quote = client_quote::add_liquidity(
        snapshot,
        max_amount_to_add_token_a,
        max_amount_to_add_token_b,
        min_amount_liquidity,
    )?;

    Ok(PreparedAddLiquidity {
        quote,
        min_amount_liquidity,
        max_amount_to_add_token_a,
        max_amount_to_add_token_b,
    })
}

/// Quotes remove liquidity and derives both minimum-withdrawal guards.
pub fn prepare_remove_liquidity(
    snapshot: &ValidatedPoolSnapshot,
    user_liquidity: &ValidatedFungibleHolding,
    remove_liquidity_amount: u128,
    tolerance: SlippageTolerance,
) -> Result<PreparedRemoveLiquidity, ClientError> {
    let preview =
        client_quote::preview_remove_liquidity(snapshot, user_liquidity, remove_liquidity_amount)?;
    let min_amount_to_remove_token_a = minimum_guard_amount(preview.withdraw_amount_a, tolerance)?;
    let min_amount_to_remove_token_b = minimum_guard_amount(preview.withdraw_amount_b, tolerance)?;
    let quote = client_quote::remove_liquidity(
        snapshot,
        user_liquidity,
        remove_liquidity_amount,
        min_amount_to_remove_token_a,
        min_amount_to_remove_token_b,
    )?;

    Ok(PreparedRemoveLiquidity {
        quote,
        remove_liquidity_amount: quote.liquidity_to_burn,
        min_amount_to_remove_token_a,
        min_amount_to_remove_token_b,
    })
}

/// Quotes an exact-input swap and derives its minimum-output guard.
pub fn prepare_swap_exact_input(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    amount_in: u128,
    tolerance: SlippageTolerance,
) -> Result<PreparedSwapExactInput, ClientError> {
    let preview =
        client_quote::preview_swap_exact_input(snapshot, user_input, user_output, amount_in)?;
    let min_amount_out = minimum_guard_amount(preview.amount_out, tolerance)?;
    let quote = client_quote::swap_exact_input(
        snapshot,
        user_input,
        user_output,
        amount_in,
        min_amount_out,
    )?;

    Ok(PreparedSwapExactInput {
        quote,
        swap_amount_in: quote.amount_in,
        min_amount_out,
    })
}

/// Quotes an exact-output swap and derives its maximum-input guard.
pub fn prepare_swap_exact_output(
    snapshot: &ValidatedPoolSnapshot,
    user_input: &ValidatedFungibleHolding,
    user_output: &ValidatedFungibleHolding,
    exact_amount_out: u128,
    tolerance: SlippageTolerance,
) -> Result<PreparedSwapExactOutput, ClientError> {
    let preview = client_quote::preview_swap_exact_output(
        snapshot,
        user_input,
        user_output,
        exact_amount_out,
    )?;
    let max_amount_in = maximum_guard_amount(preview.amount_in, tolerance)?;
    let quote = client_quote::swap_exact_output(
        snapshot,
        user_input,
        user_output,
        exact_amount_out,
        max_amount_in,
    )?;

    Ok(PreparedSwapExactOutput {
        quote,
        exact_amount_out: quote.amount_out,
        max_amount_in,
    })
}
