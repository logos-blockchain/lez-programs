//! Fallible, deterministic previews of AMM state transitions.
//!
//! These functions own the arithmetic used by the AMM instruction handlers. Host clients can call
//! the same functions to quote user operations without constructing runtime accounts or recovering
//! from guest-style assertion failures. Account ownership, signer/init constraints, deadlines, and
//! chained-call construction remain instruction-layer concerns.

use std::{error::Error, fmt};

use amm_core::{
    checked_mul_div_ceil, checked_mul_div_floor, is_supported_fee_tier, isqrt_product,
    spot_price_q64_64, PoolDefinition, FEE_BPS_DENOMINATOR, MINIMUM_LIQUIDITY,
};
use nssa_core::account::AccountId;
use twap_oracle_core::OBSERVATIONS_CAPACITY;

/// A stable, machine-readable quote failure with its program-facing message.
///
/// Consumers should branch on [`QuoteError::code`] and treat [`QuoteError::message`] as display or
/// diagnostic text. New codes may be added without changing this type's layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuoteError {
    code: &'static str,
    message: &'static str,
}

impl QuoteError {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the program-facing failure message.
    #[must_use]
    pub const fn message(&self) -> &'static str {
        self.message
    }
}

impl fmt::Display for QuoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for QuoteError {}

/// A token pair's order relative to the pool's stored token A/B order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairOrder {
    /// The caller's first/second tokens are the pool's A/B tokens.
    Stored,
    /// The caller's first/second tokens are the pool's B/A tokens.
    Reversed,
}

impl PairOrder {
    /// Converts caller-ordered raw amounts to the pool's stored A/B order.
    #[must_use]
    pub const fn amounts_to_stored(self, first: u128, second: u128) -> (u128, u128) {
        match self {
            Self::Stored => (first, second),
            Self::Reversed => (second, first),
        }
    }

    /// Converts pool A/B raw amounts back to the caller's first/second order.
    #[must_use]
    pub const fn amounts_from_stored(self, amount_a: u128, amount_b: u128) -> (u128, u128) {
        match self {
            Self::Stored => (amount_a, amount_b),
            Self::Reversed => (amount_b, amount_a),
        }
    }
}

/// Resolves a caller token pair against a pool's stored token order.
pub fn pair_order(
    pool: &PoolDefinition,
    first_token_id: AccountId,
    second_token_id: AccountId,
) -> Result<PairOrder, QuoteError> {
    if first_token_id == pool.definition_token_a_id && second_token_id == pool.definition_token_b_id
    {
        Ok(PairOrder::Stored)
    } else if first_token_id == pool.definition_token_b_id
        && second_token_id == pool.definition_token_a_id
    {
        Ok(PairOrder::Reversed)
    } else {
        Err(QuoteError::new(
            "token_pair_not_in_pool",
            "Token pair does not match the pool",
        ))
    }
}

/// Swap direction relative to the pool's stored token A/B order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwapDirection {
    /// Deposit token A and withdraw token B.
    AToB,
    /// Deposit token B and withdraw token A.
    BToA,
}

/// Resolves swap direction from the input token definition.
pub fn swap_direction(
    pool: &PoolDefinition,
    input_token_id: AccountId,
) -> Result<SwapDirection, QuoteError> {
    if input_token_id == pool.definition_token_a_id {
        Ok(SwapDirection::AToB)
    } else if input_token_id == pool.definition_token_b_id {
        Ok(SwapDirection::BToA)
    } else {
        Err(QuoteError::new(
            "input_token_not_in_pool",
            "Input token is not part of the pool",
        ))
    }
}

/// Pool scalar values after a quoted operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PoolUpdate {
    /// Total LP supply after the operation.
    pub liquidity_pool_supply: u128,
    /// Stored token-A reserve after the operation.
    pub reserve_a: u128,
    /// Stored token-B reserve after the operation.
    pub reserve_b: u128,
    /// Token-B per token-A spot price after the operation, encoded as Q64.64.
    pub spot_price_q64_64: u128,
}

impl PoolUpdate {
    /// Applies the quoted scalar values to a pool while preserving identity and fee fields.
    #[must_use]
    pub fn apply_to(&self, pool: &PoolDefinition) -> PoolDefinition {
        PoolDefinition {
            liquidity_pool_supply: self.liquidity_pool_supply,
            reserve_a: self.reserve_a,
            reserve_b: self.reserve_b,
            ..pool.clone()
        }
    }
}

/// Result of creating a pool's initial liquidity position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreatePoolQuote {
    /// Initial pool scalar values.
    pub pool: PoolUpdate,
    /// LP tokens permanently assigned to the lock holding.
    pub locked_liquidity: u128,
    /// LP tokens minted to the pool creator.
    pub user_liquidity: u128,
}

/// Quotes the `NewDefinition` economic state transition.
pub fn create_pool(
    token_a_amount: u128,
    token_b_amount: u128,
    fee_bps: u128,
) -> Result<CreatePoolQuote, QuoteError> {
    if token_a_amount == 0 {
        return Err(QuoteError::new(
            "token_a_amount_zero",
            "token_a_amount must be nonzero",
        ));
    }
    if token_b_amount == 0 {
        return Err(QuoteError::new(
            "token_b_amount_zero",
            "token_b_amount must be nonzero",
        ));
    }
    ensure_supported_fee_tier(fee_bps)?;

    let initial_liquidity = isqrt_product(token_a_amount, token_b_amount);
    if initial_liquidity <= MINIMUM_LIQUIDITY {
        return Err(QuoteError::new(
            "initial_liquidity_too_low",
            "Initial liquidity must exceed minimum liquidity lock",
        ));
    }
    let user_liquidity = initial_liquidity
        .checked_sub(MINIMUM_LIQUIDITY)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "initial liquidity must exceed minimum liquidity after validation",
            )
        })?;
    let pool = pool_update(initial_liquidity, token_a_amount, token_b_amount)?;

    Ok(CreatePoolQuote {
        pool,
        locked_liquidity: MINIMUM_LIQUIDITY,
        user_liquidity,
    })
}

/// Result of adding liquidity to an initialized pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddLiquidityQuote {
    /// Token-A amount transferred into the pool.
    pub actual_amount_a: u128,
    /// Token-B amount transferred into the pool.
    pub actual_amount_b: u128,
    /// LP amount minted to the caller.
    pub liquidity_to_mint: u128,
    /// Pool scalar values after the deposit.
    pub pool: PoolUpdate,
}

/// Previews `AddLiquidity` using the smallest executable LP guard.
///
/// Use [`add_liquidity`] with the caller's slippage-derived guard before constructing an
/// instruction.
pub fn preview_add_liquidity(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    max_amount_a: u128,
    max_amount_b: u128,
) -> Result<AddLiquidityQuote, QuoteError> {
    add_liquidity(
        pool,
        vault_a_balance,
        vault_b_balance,
        max_amount_a,
        max_amount_b,
        1,
    )
}

/// Quotes the `AddLiquidity` economic state transition.
pub fn add_liquidity(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    max_amount_a: u128,
    max_amount_b: u128,
    minimum_liquidity: u128,
) -> Result<AddLiquidityQuote, QuoteError> {
    ensure_supported_fee_tier(pool.fees)?;
    if minimum_liquidity == 0 {
        return Err(QuoteError::new(
            "minimum_liquidity_zero",
            "min_amount_liquidity must be nonzero",
        ));
    }
    if max_amount_a == 0 || max_amount_b == 0 {
        return Err(QuoteError::new(
            "maximum_deposit_zero",
            "Both max-balances must be nonzero",
        ));
    }
    ensure_vault_balances(
        pool,
        vault_a_balance,
        vault_b_balance,
        "Vaults' balances must be at least the reserve amounts",
        "Vaults' balances must be at least the reserve amounts",
    )?;
    if pool.reserve_a == 0 || pool.reserve_b == 0 {
        return Err(QuoteError::new("reserve_zero", "Reserves must be nonzero"));
    }

    let ideal_a = checked_floor(
        pool.reserve_a,
        max_amount_b,
        pool.reserve_b,
        "mul_div_floor result exceeds u128",
    )?;
    let ideal_b = checked_floor(
        pool.reserve_b,
        max_amount_a,
        pool.reserve_a,
        "mul_div_floor result exceeds u128",
    )?;
    let actual_amount_a = max_amount_a.min(ideal_a);
    let actual_amount_b = max_amount_b.min(ideal_b);
    if actual_amount_a == 0 || actual_amount_b == 0 {
        return Err(QuoteError::new(
            "deposit_amount_zero",
            "A trade amount is 0",
        ));
    }

    let liquidity_from_a = checked_floor(
        pool.liquidity_pool_supply,
        actual_amount_a,
        pool.reserve_a,
        "mul_div_floor result exceeds u128",
    )?;
    let liquidity_from_b = checked_floor(
        pool.liquidity_pool_supply,
        actual_amount_b,
        pool.reserve_b,
        "mul_div_floor result exceeds u128",
    )?;
    let liquidity_to_mint = liquidity_from_a.min(liquidity_from_b);
    if liquidity_to_mint == 0 {
        return Err(QuoteError::new(
            "minted_liquidity_zero",
            "Payable LP must be nonzero",
        ));
    }
    if liquidity_to_mint < minimum_liquidity {
        return Err(QuoteError::new(
            "minted_liquidity_below_minimum",
            "Payable LP is less than provided minimum LP amount",
        ));
    }

    let liquidity_pool_supply = pool
        .liquidity_pool_supply
        .checked_add(liquidity_to_mint)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "liquidity_pool_supply + delta_lp overflows u128",
            )
        })?;
    let reserve_a = pool.reserve_a.checked_add(actual_amount_a).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "reserve_a + actual_amount_a overflows u128",
        )
    })?;
    let reserve_b = pool.reserve_b.checked_add(actual_amount_b).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "reserve_b + actual_amount_b overflows u128",
        )
    })?;

    Ok(AddLiquidityQuote {
        actual_amount_a,
        actual_amount_b,
        liquidity_to_mint,
        pool: pool_update(liquidity_pool_supply, reserve_a, reserve_b)?,
    })
}

/// Result of removing liquidity from a pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveLiquidityQuote {
    /// Token-A amount withdrawn from the pool.
    pub withdraw_amount_a: u128,
    /// Token-B amount withdrawn from the pool.
    pub withdraw_amount_b: u128,
    /// LP amount burned from the caller.
    pub liquidity_to_burn: u128,
    /// Pool scalar values after the withdrawal.
    pub pool: PoolUpdate,
}

/// Previews `RemoveLiquidity` using the smallest executable withdrawal guards.
///
/// Use [`remove_liquidity`] with the caller's slippage-derived guards before constructing an
/// instruction.
pub fn preview_remove_liquidity(
    pool: &PoolDefinition,
    user_liquidity_balance: u128,
    remove_liquidity_amount: u128,
) -> Result<RemoveLiquidityQuote, QuoteError> {
    remove_liquidity(pool, user_liquidity_balance, remove_liquidity_amount, 1, 1)
}

/// Quotes the `RemoveLiquidity` economic state transition.
pub fn remove_liquidity(
    pool: &PoolDefinition,
    user_liquidity_balance: u128,
    remove_liquidity_amount: u128,
    minimum_amount_a: u128,
    minimum_amount_b: u128,
) -> Result<RemoveLiquidityQuote, QuoteError> {
    ensure_supported_fee_tier(pool.fees)?;
    if pool.liquidity_pool_supply < MINIMUM_LIQUIDITY {
        return Err(QuoteError::new(
            "liquidity_supply_below_minimum",
            "Pool liquidity supply is below minimum liquidity",
        ));
    }
    if minimum_amount_a == 0 || minimum_amount_b == 0 {
        return Err(QuoteError::new(
            "minimum_withdrawal_zero",
            "Minimum withdraw amount must be nonzero",
        ));
    }
    if user_liquidity_balance > pool.liquidity_pool_supply {
        return Err(QuoteError::new(
            "invalid_liquidity_account",
            "Invalid liquidity account provided",
        ));
    }
    if pool.liquidity_pool_supply == MINIMUM_LIQUIDITY {
        return Err(QuoteError::new(
            "pool_contains_only_locked_liquidity",
            "Pool only contains locked liquidity",
        ));
    }
    if remove_liquidity_amount == 0 {
        return Err(QuoteError::new(
            "remove_liquidity_amount_zero",
            "remove_liquidity_amount must be nonzero",
        ));
    }
    if remove_liquidity_amount > user_liquidity_balance {
        return Err(QuoteError::new(
            "remove_amount_exceeds_user_balance",
            "Remove amount exceeds user LP balance",
        ));
    }
    let unlocked_liquidity = pool
        .liquidity_pool_supply
        .checked_sub(MINIMUM_LIQUIDITY)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "liquidity supply must be at least the locked minimum after validation",
            )
        })?;
    if remove_liquidity_amount > unlocked_liquidity {
        return Err(QuoteError::new(
            "remove_amount_exceeds_unlocked_liquidity",
            "Cannot remove locked minimum liquidity",
        ));
    }

    let withdraw_amount_a = checked_floor(
        pool.reserve_a,
        remove_liquidity_amount,
        pool.liquidity_pool_supply,
        "mul_div_floor result exceeds u128",
    )?;
    let withdraw_amount_b = checked_floor(
        pool.reserve_b,
        remove_liquidity_amount,
        pool.liquidity_pool_supply,
        "mul_div_floor result exceeds u128",
    )?;
    if withdraw_amount_a < minimum_amount_a {
        return Err(QuoteError::new(
            "withdrawal_a_below_minimum",
            "Insufficient minimal withdraw amount (Token A) provided for liquidity amount",
        ));
    }
    if withdraw_amount_b < minimum_amount_b {
        return Err(QuoteError::new(
            "withdrawal_b_below_minimum",
            "Insufficient minimal withdraw amount (Token B) provided for liquidity amount",
        ));
    }

    let liquidity_pool_supply = pool
        .liquidity_pool_supply
        .checked_sub(remove_liquidity_amount)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "liquidity_pool_supply - delta_lp underflows",
            )
        })?;
    let reserve_a = pool
        .reserve_a
        .checked_sub(withdraw_amount_a)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "reserve_a - withdraw_amount_a underflows",
            )
        })?;
    let reserve_b = pool
        .reserve_b
        .checked_sub(withdraw_amount_b)
        .ok_or_else(|| {
            QuoteError::new(
                "arithmetic_overflow",
                "reserve_b - withdraw_amount_b underflows",
            )
        })?;

    Ok(RemoveLiquidityQuote {
        withdraw_amount_a,
        withdraw_amount_b,
        liquidity_to_burn: remove_liquidity_amount,
        pool: pool_update(liquidity_pool_supply, reserve_a, reserve_b)?,
    })
}

/// Result of either exact-input or exact-output swap quoting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwapQuote {
    /// Direction relative to stored pool order.
    pub direction: SwapDirection,
    /// Gross amount transferred from the user.
    pub amount_in: u128,
    /// Input amount used by constant-product pricing after fee rounding.
    pub effective_amount_in: u128,
    /// Gross input retained as LP fee.
    pub fee_amount: u128,
    /// Amount transferred to the user.
    pub amount_out: u128,
    /// Pool scalar values after the trade.
    pub pool: PoolUpdate,
}

/// Previews `SwapExactInput` without a minimum-output guard.
///
/// Use [`swap_exact_input`] with the caller's slippage-derived minimum before constructing an
/// instruction.
pub fn preview_swap_exact_input(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    direction: SwapDirection,
    amount_in: u128,
) -> Result<SwapQuote, QuoteError> {
    swap_exact_input(
        pool,
        vault_a_balance,
        vault_b_balance,
        direction,
        amount_in,
        0,
    )
}

/// Quotes a `SwapExactInput` state transition.
pub fn swap_exact_input(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    direction: SwapDirection,
    amount_in: u128,
    minimum_amount_out: u128,
) -> Result<SwapQuote, QuoteError> {
    validate_swap_pool(pool, vault_a_balance, vault_b_balance)?;
    let (reserve_in, reserve_out) = directional_reserves(pool, direction);
    let fee_multiplier = fee_multiplier(pool.fees)?;
    let effective_amount_in = checked_floor(
        amount_in,
        fee_multiplier,
        FEE_BPS_DENOMINATOR,
        "mul_div_floor result exceeds u128",
    )?;
    if effective_amount_in == 0 {
        return Err(QuoteError::new(
            "effective_swap_input_zero",
            "Effective swap amount should be nonzero",
        ));
    }
    let reserve_plus_effective = reserve_in.checked_add(effective_amount_in).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "reserve + effective_amount_in overflows u128",
        )
    })?;
    let amount_out = checked_floor(
        reserve_out,
        effective_amount_in,
        reserve_plus_effective,
        "mul_div_floor result exceeds u128",
    )?;
    if amount_out < minimum_amount_out {
        return Err(QuoteError::new(
            "swap_output_below_minimum",
            "Withdraw amount is less than minimal amount out",
        ));
    }
    if amount_out == 0 {
        return Err(QuoteError::new(
            "swap_output_zero",
            "Withdraw amount should be nonzero",
        ));
    }

    finish_swap_quote(pool, direction, amount_in, effective_amount_in, amount_out)
}

/// Previews `SwapExactOutput` without a restrictive maximum-input guard.
///
/// Use [`swap_exact_output`] with the caller's slippage-derived maximum before constructing an
/// instruction.
pub fn preview_swap_exact_output(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    direction: SwapDirection,
    exact_amount_out: u128,
) -> Result<SwapQuote, QuoteError> {
    swap_exact_output(
        pool,
        vault_a_balance,
        vault_b_balance,
        direction,
        exact_amount_out,
        u128::MAX,
    )
}

/// Quotes a `SwapExactOutput` state transition.
pub fn swap_exact_output(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    direction: SwapDirection,
    exact_amount_out: u128,
    maximum_amount_in: u128,
) -> Result<SwapQuote, QuoteError> {
    validate_swap_pool(pool, vault_a_balance, vault_b_balance)?;
    if exact_amount_out == 0 {
        return Err(QuoteError::new(
            "exact_output_zero",
            "Exact amount out must be nonzero",
        ));
    }

    let (reserve_in, reserve_out) = directional_reserves(pool, direction);
    if exact_amount_out >= reserve_out {
        return Err(QuoteError::new(
            "exact_output_exceeds_reserve",
            "Exact amount out exceeds reserve",
        ));
    }
    let effective_input_denominator =
        reserve_out.checked_sub(exact_amount_out).ok_or_else(|| {
            QuoteError::new("arithmetic_overflow", "reserve_out - amount_out underflows")
        })?;
    let minimum_effective_input = checked_ceil(
        reserve_in,
        exact_amount_out,
        effective_input_denominator,
        "mul_div_ceil result exceeds u128",
    )?;
    let fee_multiplier = fee_multiplier(pool.fees)?;
    let amount_in = checked_ceil(
        minimum_effective_input,
        FEE_BPS_DENOMINATOR,
        fee_multiplier,
        "mul_div_ceil result exceeds u128",
    )?;
    if amount_in > maximum_amount_in {
        return Err(QuoteError::new(
            "required_input_exceeds_maximum",
            "Required input exceeds maximum amount in",
        ));
    }
    let effective_amount_in = checked_floor(
        amount_in,
        fee_multiplier,
        FEE_BPS_DENOMINATOR,
        "mul_div_floor result exceeds u128",
    )?;

    finish_swap_quote(
        pool,
        direction,
        amount_in,
        effective_amount_in,
        exact_amount_out,
    )
}

/// Result of synchronizing stored reserves to vault balances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncReservesQuote {
    /// Untracked token-A balance incorporated into the reserve.
    pub donated_amount_a: u128,
    /// Untracked token-B balance incorporated into the reserve.
    pub donated_amount_b: u128,
    /// Pool scalar values after synchronization.
    pub pool: PoolUpdate,
}

/// Quotes a `SyncReserves` state transition.
pub fn sync_reserves(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
) -> Result<SyncReservesQuote, QuoteError> {
    ensure_supported_fee_tier(pool.fees)?;
    if pool.liquidity_pool_supply < MINIMUM_LIQUIDITY {
        return Err(QuoteError::new(
            "liquidity_supply_below_minimum",
            "Pool liquidity supply is below minimum liquidity",
        ));
    }
    ensure_vault_balances(
        pool,
        vault_a_balance,
        vault_b_balance,
        "Sync reserves: vault A balance is less than its reserve",
        "Sync reserves: vault B balance is less than its reserve",
    )?;
    let donated_amount_a = vault_a_balance.checked_sub(pool.reserve_a).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "vault A balance - reserve A underflows",
        )
    })?;
    let donated_amount_b = vault_b_balance.checked_sub(pool.reserve_b).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "vault B balance - reserve B underflows",
        )
    })?;

    Ok(SyncReservesQuote {
        donated_amount_a,
        donated_amount_b,
        pool: pool_update(pool.liquidity_pool_supply, vault_a_balance, vault_b_balance)?,
    })
}

/// Values used to initialize a pool-backed TWAP oracle price account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OraclePriceAccountQuote {
    /// Pool token A, used as the oracle base asset.
    pub base_asset: AccountId,
    /// Pool token B, used as the oracle quote asset.
    pub quote_asset: AccountId,
    /// Current pool spot price encoded as Q64.64.
    pub initial_price_q64_64: u128,
    /// Requested TWAP window duration in milliseconds.
    pub window_duration: u64,
}

/// Quotes values derived by `CreateOraclePriceAccount` from pool state.
pub fn create_oracle_price_account(
    pool: &PoolDefinition,
    window_duration: u64,
) -> Result<OraclePriceAccountQuote, QuoteError> {
    if window_duration < u64::from(OBSERVATIONS_CAPACITY) {
        return Err(QuoteError::new(
            "oracle_window_too_short",
            "Create oracle price account: window_duration must be >= OBSERVATIONS_CAPACITY so a matching PriceObservations account can exist and PublishPrice can update this price account",
        ));
    }
    if pool.reserve_a == 0 {
        return Err(QuoteError::new(
            "reserve_a_zero",
            "spot_price_q64_64: reserve_base must be non-zero",
        ));
    }
    let initial_price_q64_64 = spot_price_q64_64(pool.reserve_a, pool.reserve_b);
    if initial_price_q64_64 == 0 {
        return Err(QuoteError::new(
            "oracle_price_zero",
            "Create oracle price account: pool spot price must be non-zero (zero is the no-price sentinel; pool reserve_b is zero or negligible relative to reserve_a)",
        ));
    }

    Ok(OraclePriceAccountQuote {
        base_asset: pool.definition_token_a_id,
        quote_asset: pool.definition_token_b_id,
        initial_price_q64_64,
        window_duration,
    })
}

fn ensure_supported_fee_tier(fee_bps: u128) -> Result<(), QuoteError> {
    if is_supported_fee_tier(fee_bps) {
        Ok(())
    } else {
        Err(QuoteError::new(
            "unsupported_fee_tier",
            "Fee tier must be one of 1, 5, 30, or 100 basis points",
        ))
    }
}

fn ensure_vault_balances(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
    vault_a_message: &'static str,
    vault_b_message: &'static str,
) -> Result<(), QuoteError> {
    if vault_a_balance < pool.reserve_a {
        return Err(QuoteError::new(
            "vault_a_balance_below_reserve",
            vault_a_message,
        ));
    }
    if vault_b_balance < pool.reserve_b {
        return Err(QuoteError::new(
            "vault_b_balance_below_reserve",
            vault_b_message,
        ));
    }

    Ok(())
}

fn validate_swap_pool(
    pool: &PoolDefinition,
    vault_a_balance: u128,
    vault_b_balance: u128,
) -> Result<(), QuoteError> {
    ensure_supported_fee_tier(pool.fees)?;
    if pool.liquidity_pool_supply < MINIMUM_LIQUIDITY {
        return Err(QuoteError::new(
            "liquidity_supply_below_minimum",
            "Pool liquidity supply is below minimum liquidity",
        ));
    }
    ensure_vault_balances(
        pool,
        vault_a_balance,
        vault_b_balance,
        "Reserve for Token A exceeds vault balance",
        "Reserve for Token B exceeds vault balance",
    )
}

fn directional_reserves(pool: &PoolDefinition, direction: SwapDirection) -> (u128, u128) {
    match direction {
        SwapDirection::AToB => (pool.reserve_a, pool.reserve_b),
        SwapDirection::BToA => (pool.reserve_b, pool.reserve_a),
    }
}

fn finish_swap_quote(
    pool: &PoolDefinition,
    direction: SwapDirection,
    amount_in: u128,
    effective_amount_in: u128,
    amount_out: u128,
) -> Result<SwapQuote, QuoteError> {
    let fee_amount = amount_in.checked_sub(effective_amount_in).ok_or_else(|| {
        QuoteError::new(
            "arithmetic_overflow",
            "gross input - effective input underflows",
        )
    })?;
    let (reserve_a, reserve_b) = match direction {
        SwapDirection::AToB => (
            pool.reserve_a.checked_add(amount_in).ok_or_else(|| {
                QuoteError::new(
                    "arithmetic_overflow",
                    "reserve_a + deposit_a overflows u128",
                )
            })?,
            pool.reserve_b.checked_sub(amount_out).ok_or_else(|| {
                QuoteError::new(
                    "arithmetic_overflow",
                    "reserve_b + deposit_b - withdraw_b underflows",
                )
            })?,
        ),
        SwapDirection::BToA => (
            pool.reserve_a.checked_sub(amount_out).ok_or_else(|| {
                QuoteError::new(
                    "arithmetic_overflow",
                    "reserve_a + deposit_a - withdraw_a underflows",
                )
            })?,
            pool.reserve_b.checked_add(amount_in).ok_or_else(|| {
                QuoteError::new(
                    "arithmetic_overflow",
                    "reserve_b + deposit_b overflows u128",
                )
            })?,
        ),
    };

    Ok(SwapQuote {
        direction,
        amount_in,
        effective_amount_in,
        fee_amount,
        amount_out,
        pool: pool_update(pool.liquidity_pool_supply, reserve_a, reserve_b)?,
    })
}

fn fee_multiplier(fee_bps: u128) -> Result<u128, QuoteError> {
    FEE_BPS_DENOMINATOR
        .checked_sub(fee_bps)
        .ok_or_else(|| QuoteError::new("unsupported_fee_tier", "fee_bps exceeds fee denominator"))
}

fn pool_update(
    liquidity_pool_supply: u128,
    reserve_a: u128,
    reserve_b: u128,
) -> Result<PoolUpdate, QuoteError> {
    if reserve_a == 0 {
        return Err(QuoteError::new(
            "reserve_a_zero",
            "spot_price_q64_64: reserve_base must be non-zero",
        ));
    }

    Ok(PoolUpdate {
        liquidity_pool_supply,
        reserve_a,
        reserve_b,
        spot_price_q64_64: spot_price_q64_64(reserve_a, reserve_b),
    })
}

fn checked_floor(
    left: u128,
    right: u128,
    denominator: u128,
    overflow_message: &'static str,
) -> Result<u128, QuoteError> {
    checked_mul_div_floor(left, right, denominator)
        .ok_or_else(|| QuoteError::new("arithmetic_overflow", overflow_message))
}

fn checked_ceil(
    left: u128,
    right: u128,
    denominator: u128,
    overflow_message: &'static str,
) -> Result<u128, QuoteError> {
    checked_mul_div_ceil(left, right, denominator)
        .ok_or_else(|| QuoteError::new("arithmetic_overflow", overflow_message))
}
