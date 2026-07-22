//! Protocol-aware amount preparation for human-facing AMM intents.

use std::{error::Error, fmt};

use alloy_primitives::U512;
use amm_core::{
    canonical_token_pair, checked_mul_div_ceil, isqrt_product, spot_price_q64_64, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use amm_program::quote::{
    self as program_quote, CreatePoolQuote, PairOrder, SwapDirection, SwapQuote,
};
use nssa_core::account::AccountId;

/// One whole unit in the Q64.64 price representation used by the AMM.
pub const Q64_64_ONE: u128 = 1_u128 << 64;

/// Largest token decimal count accepted by human-price conversion.
///
/// One whole token at a larger decimal count cannot fit in the protocol's `u128` raw amount.
pub const MAX_TOKEN_DECIMALS: u8 = 38;

/// Largest fractional precision accepted for either side of a human price ratio.
pub const MAX_HUMAN_PRICE_FRACTIONAL_DIGITS: u8 = 38;

/// Failure while turning a caller intent into executable AMM amounts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IntentError {
    /// A caller requested a token paired with itself.
    IdenticalTokenDefinitions,
    /// A Q64.64 desired price must be nonzero.
    ZeroDesiredPrice,
    /// One side of a human price ratio is not an unsigned decimal amount.
    InvalidHumanPriceAmount { field: &'static str },
    /// One side of a human price ratio is zero.
    ZeroHumanPriceAmount { field: &'static str },
    /// One side of a human price ratio has unsupported fractional precision.
    HumanPricePrecisionOutOfRange {
        field: &'static str,
        precision: usize,
    },
    /// Token metadata reports a decimal count outside the protocol amount range.
    TokenDecimalsOutOfRange { field: &'static str, decimals: u8 },
    /// A positive human price is smaller than the least positive Q64.64 value.
    HumanPriceUnderflow,
    /// An edited token amount must be nonzero.
    ZeroEditedAmount,
    /// A widened calculation produced a result outside the chain's `u128` amount range.
    ArithmeticOverflow { operation: &'static str },
    /// Explicit opening amounts do not encode the requested Q64.64 spot price exactly.
    SpotPriceMismatch { desired: u128, actual: u128 },
    /// A pool or quoted pool update has a zero directional reserve.
    ZeroDirectionalReserve,
    /// The supplied quote moves the directional spot price opposite to its swap direction.
    SpotMovedAgainstSwap,
    /// Canonical program quote logic rejected the prepared amounts.
    Quote {
        code: &'static str,
        message: &'static str,
    },
}

impl IntentError {
    /// Stable machine-readable error code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::IdenticalTokenDefinitions => "identical_token_definitions",
            Self::ZeroDesiredPrice => "zero_desired_price",
            Self::InvalidHumanPriceAmount { .. } => "invalid_human_price_amount",
            Self::ZeroHumanPriceAmount { .. } => "zero_human_price_amount",
            Self::HumanPricePrecisionOutOfRange { .. } => "human_price_precision_out_of_range",
            Self::TokenDecimalsOutOfRange { .. } => "token_decimals_out_of_range",
            Self::HumanPriceUnderflow => "human_price_underflow",
            Self::ZeroEditedAmount => "zero_edited_amount",
            Self::ArithmeticOverflow { .. } => "intent_arithmetic_overflow",
            Self::SpotPriceMismatch { .. } => "spot_price_mismatch",
            Self::ZeroDirectionalReserve => "zero_directional_reserve",
            Self::SpotMovedAgainstSwap => "spot_moved_against_swap",
            Self::Quote { code, .. } => code,
        }
    }
}

impl fmt::Display for IntentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdenticalTokenDefinitions => {
                formatter.write_str("pool token definitions must be distinct")
            }
            Self::ZeroDesiredPrice => formatter.write_str("desired Q64.64 price must be nonzero"),
            Self::InvalidHumanPriceAmount { field } => {
                write!(formatter, "{field} must be an unsigned decimal amount")
            }
            Self::ZeroHumanPriceAmount { field } => {
                write!(formatter, "{field} must be greater than zero")
            }
            Self::HumanPricePrecisionOutOfRange { field, precision } => write!(
                formatter,
                "{field} has {precision} fractional digits; maximum is {MAX_HUMAN_PRICE_FRACTIONAL_DIGITS}"
            ),
            Self::TokenDecimalsOutOfRange { field, decimals } => write!(
                formatter,
                "{field} is {decimals}; maximum is {MAX_TOKEN_DECIMALS}"
            ),
            Self::HumanPriceUnderflow => {
                formatter.write_str("human price is below the Q64.64 precision range")
            }
            Self::ZeroEditedAmount => formatter.write_str("edited token amount must be nonzero"),
            Self::ArithmeticOverflow { operation } => {
                write!(formatter, "{operation} exceeds the u128 amount range")
            }
            Self::SpotPriceMismatch { desired, actual } => write!(
                formatter,
                "opening amounts encode Q64.64 price {actual}, not requested price {desired}"
            ),
            Self::ZeroDirectionalReserve => {
                formatter.write_str("directional pool reserves must be nonzero")
            }
            Self::SpotMovedAgainstSwap => {
                formatter.write_str("quoted spot price moved opposite to the swap direction")
            }
            Self::Quote { message, .. } => formatter.write_str(message),
        }
    }
}

impl Error for IntentError {}

impl From<program_quote::QuoteError> for IntentError {
    fn from(error: program_quote::QuoteError) -> Self {
        Self::Quote {
            code: error.code(),
            message: error.message(),
        }
    }
}

/// Executable opening amounts plus their canonical program quote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct PreparedOpeningPair {
    /// Requested Q64.64 target used to pair or minimize the amounts.
    pub desired_price_q64_64: u128,
    /// Exact Q64.64 price encoded by the returned integer amounts.
    pub actual_price_q64_64: u128,
    /// Stored token-A amount for `NewDefinition`.
    pub token_a_amount: u128,
    /// Stored token-B amount for `NewDefinition`.
    pub token_b_amount: u128,
    /// Fee tier passed to canonical pool-creation quote logic.
    pub fee_bps: u128,
    /// Canonical pool-creation result for the returned amounts.
    pub quote: CreatePoolQuote,
}

/// Caller-facing source for an opening-liquidity pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum OpeningLiquidityIntent {
    /// Find the smallest executable pair at the requested price.
    Minimum,
    /// Pair an amount edited for the caller's first token.
    FirstAmount(u128),
    /// Pair an amount edited for the caller's second token.
    SecondAmount(u128),
    /// Validate two explicit amounts in caller first/second order.
    Explicit {
        first_amount: u128,
        second_amount: u128,
    },
}

/// Executable opening amounts in both caller and canonical stored order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedCallerOpeningPair {
    caller_order: PairOrder,
    first_amount: u128,
    second_amount: u128,
    stored: PreparedOpeningPair,
}

impl PreparedCallerOpeningPair {
    /// Caller first/second order relative to canonical stored A/B order.
    #[must_use]
    pub const fn caller_order(&self) -> PairOrder {
        self.caller_order
    }

    #[must_use]
    pub const fn first_amount(&self) -> u128 {
        self.first_amount
    }

    #[must_use]
    pub const fn second_amount(&self) -> u128 {
        self.second_amount
    }

    /// Canonical token-A/token-B result ready for pool-creation validation and planning.
    #[must_use]
    pub const fn stored(&self) -> &PreparedOpeningPair {
        &self.stored
    }
}

#[derive(Clone, Copy)]
struct ParsedHumanAmount {
    mantissa: u128,
    fractional_digits: u8,
}

/// Converts an exact human token ratio into the pool's canonical raw Q64.64 price.
///
/// `first_amount` units of the caller's first token are declared equal in value to
/// `second_amount` units of the second token. The token IDs select canonical stored A/B order;
/// callers do not invert the ratio when their display order is reversed. Token decimal counts
/// convert the human ratio into raw-unit reserve B per raw-unit reserve A. Calculation uses integer
/// arithmetic and floors once at Q64.64 conversion.
pub fn human_price_ratio_to_q64_64(
    first_token_definition_id: AccountId,
    second_token_definition_id: AccountId,
    first_amount: &str,
    second_amount: &str,
    first_token_decimals: u8,
    second_token_decimals: u8,
) -> Result<u128, IntentError> {
    let Some((stored_a_id, _)) =
        canonical_token_pair(first_token_definition_id, second_token_definition_id)
    else {
        return Err(IntentError::IdenticalTokenDefinitions);
    };
    validate_token_decimals("firstTokenDecimals", first_token_decimals)?;
    validate_token_decimals("secondTokenDecimals", second_token_decimals)?;
    let first = parse_human_price_amount(first_amount, "firstAmount")?;
    let second = parse_human_price_amount(second_amount, "secondAmount")?;

    let (base, quote, base_decimals, quote_decimals) = if first_token_definition_id == stored_a_id {
        (first, second, first_token_decimals, second_token_decimals)
    } else {
        (second, first, second_token_decimals, first_token_decimals)
    };
    let numerator_exponent = u16::from(quote_decimals)
        .checked_add(u16::from(base.fractional_digits))
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "human price numerator exponent",
        })?;
    let denominator_exponent = u16::from(base_decimals)
        .checked_add(u16::from(quote.fractional_digits))
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "human price denominator exponent",
        })?;
    let (numerator_exponent, denominator_exponent) = if numerator_exponent >= denominator_exponent {
        (
            numerator_exponent.checked_sub(denominator_exponent).ok_or(
                IntentError::ArithmeticOverflow {
                    operation: "human price exponent reduction",
                },
            )?,
            0,
        )
    } else {
        (
            0,
            denominator_exponent.checked_sub(numerator_exponent).ok_or(
                IntentError::ArithmeticOverflow {
                    operation: "human price exponent reduction",
                },
            )?,
        )
    };

    let numerator = U512::from(quote.mantissa)
        .checked_mul(U512::from(Q64_64_ONE))
        .and_then(|value| value.checked_mul(pow10(numerator_exponent)?))
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "human price numerator",
        })?;
    let denominator = U512::from(base.mantissa)
        .checked_mul(
            pow10(denominator_exponent).ok_or(IntentError::ArithmeticOverflow {
                operation: "human price denominator power",
            })?,
        )
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "human price denominator",
        })?;
    let converted = numerator
        .checked_div(denominator)
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "human price division",
        })?;
    if converted == U512::ZERO {
        return Err(IntentError::HumanPriceUnderflow);
    }
    u128::try_from(converted).map_err(|_| IntentError::ArithmeticOverflow {
        operation: "human Q64.64 price",
    })
}

fn validate_token_decimals(field: &'static str, decimals: u8) -> Result<(), IntentError> {
    if decimals > MAX_TOKEN_DECIMALS {
        Err(IntentError::TokenDecimalsOutOfRange { field, decimals })
    } else {
        Ok(())
    }
}

fn parse_human_price_amount(
    value: &str,
    field: &'static str,
) -> Result<ParsedHumanAmount, IntentError> {
    let (whole, fraction) = value.split_once('.').map_or((value, ""), |parts| parts);
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(IntentError::InvalidHumanPriceAmount { field });
    }
    if fraction.len() > usize::from(MAX_HUMAN_PRICE_FRACTIONAL_DIGITS) {
        return Err(IntentError::HumanPricePrecisionOutOfRange {
            field,
            precision: fraction.len(),
        });
    }

    let mut digits = String::from(whole);
    digits.push_str(fraction);
    let mantissa = digits
        .parse::<u128>()
        .map_err(|_| IntentError::InvalidHumanPriceAmount { field })?;
    if mantissa == 0 {
        return Err(IntentError::ZeroHumanPriceAmount { field });
    }
    let fractional_digits =
        u8::try_from(fraction.len()).map_err(|_| IntentError::HumanPricePrecisionOutOfRange {
            field,
            precision: fraction.len(),
        })?;
    Ok(ParsedHumanAmount {
        mantissa,
        fractional_digits,
    })
}

fn pow10(exponent: u16) -> Option<U512> {
    (0..exponent).try_fold(U512::ONE, |value, _| value.checked_mul(U512::from(10_u8)))
}

/// Prepares an opening pair without requiring a caller to reproduce canonical token ordering.
pub fn prepare_caller_opening_pair(
    first_token_definition_id: AccountId,
    second_token_definition_id: AccountId,
    desired_price_q64_64: u128,
    fee_bps: u128,
    intent: OpeningLiquidityIntent,
) -> Result<PreparedCallerOpeningPair, IntentError> {
    let Some((stored_a_id, _)) =
        canonical_token_pair(first_token_definition_id, second_token_definition_id)
    else {
        return Err(IntentError::IdenticalTokenDefinitions);
    };
    let caller_order = if first_token_definition_id == stored_a_id {
        PairOrder::Stored
    } else {
        PairOrder::Reversed
    };
    let stored = match intent {
        OpeningLiquidityIntent::Minimum => {
            prepare_minimum_opening_pair(desired_price_q64_64, fee_bps)?
        }
        OpeningLiquidityIntent::FirstAmount(first_amount) => match caller_order {
            PairOrder::Stored => {
                prepare_opening_from_token_a(first_amount, desired_price_q64_64, fee_bps)?
            }
            PairOrder::Reversed => {
                prepare_opening_from_token_b(first_amount, desired_price_q64_64, fee_bps)?
            }
        },
        OpeningLiquidityIntent::SecondAmount(second_amount) => match caller_order {
            PairOrder::Stored => {
                prepare_opening_from_token_b(second_amount, desired_price_q64_64, fee_bps)?
            }
            PairOrder::Reversed => {
                prepare_opening_from_token_a(second_amount, desired_price_q64_64, fee_bps)?
            }
        },
        OpeningLiquidityIntent::Explicit {
            first_amount,
            second_amount,
        } => {
            let (token_a_amount, token_b_amount) =
                caller_order.amounts_to_stored(first_amount, second_amount);
            validate_explicit_opening_pair(
                token_a_amount,
                token_b_amount,
                desired_price_q64_64,
                fee_bps,
            )?
        }
    };
    let (first_amount, second_amount) =
        caller_order.amounts_from_stored(stored.token_a_amount, stored.token_b_amount);
    Ok(PreparedCallerOpeningPair {
        caller_order,
        first_amount,
        second_amount,
        stored,
    })
}

/// Returns the token-B amount paired with an edited token-A amount.
///
/// The result is `ceil(token_a_amount * desired_price / 2^64)`, computed with the same widened
/// integer helper used by AMM code. Callers can inspect the actual representable price returned by
/// [`prepare_opening_from_token_a`] when integer rounding cannot reproduce the target exactly.
pub fn paired_amount_from_token_a(
    token_a_amount: u128,
    desired_price_q64_64: u128,
) -> Result<u128, IntentError> {
    validate_pairing_inputs(token_a_amount, desired_price_q64_64)?;
    checked_mul_div_ceil(token_a_amount, desired_price_q64_64, Q64_64_ONE).ok_or(
        IntentError::ArithmeticOverflow {
            operation: "token-A to token-B pairing",
        },
    )
}

/// Returns the token-A amount paired with an edited token-B amount.
///
/// The result is `ceil(token_b_amount * 2^64 / desired_price)`, using checked widened arithmetic.
pub fn paired_amount_from_token_b(
    token_b_amount: u128,
    desired_price_q64_64: u128,
) -> Result<u128, IntentError> {
    validate_pairing_inputs(token_b_amount, desired_price_q64_64)?;
    checked_mul_div_ceil(token_b_amount, Q64_64_ONE, desired_price_q64_64).ok_or(
        IntentError::ArithmeticOverflow {
            operation: "token-B to token-A pairing",
        },
    )
}

/// Finds the smallest executable opening pair on the price's base side.
///
/// For prices at least one, token A is minimized. For prices below one, token B is minimized. The
/// opposite amount is conservatively rounded up. The returned values are always passed through
/// [`amm_program::quote::create_pool`] before success is returned.
pub fn prepare_minimum_opening_pair(
    desired_price_q64_64: u128,
    fee_bps: u128,
) -> Result<PreparedOpeningPair, IntentError> {
    if desired_price_q64_64 == 0 {
        return Err(IntentError::ZeroDesiredPrice);
    }
    let upper = MINIMUM_LIQUIDITY
        .checked_add(1)
        .ok_or(IntentError::ArithmeticOverflow {
            operation: "minimum opening-liquidity bound",
        })?;

    let (token_a_amount, token_b_amount) = if desired_price_q64_64 >= Q64_64_ONE {
        let token_a_amount = first_executable(upper, |candidate_a| {
            let candidate_b = paired_amount_from_token_a(candidate_a, desired_price_q64_64)?;
            Ok(isqrt_product(candidate_a, candidate_b) > MINIMUM_LIQUIDITY)
        })?;
        (
            token_a_amount,
            paired_amount_from_token_a(token_a_amount, desired_price_q64_64)?,
        )
    } else {
        let token_b_amount = first_executable(upper, |candidate_b| {
            let candidate_a = paired_amount_from_token_b(candidate_b, desired_price_q64_64)?;
            Ok(isqrt_product(candidate_a, candidate_b) > MINIMUM_LIQUIDITY)
        })?;
        (
            paired_amount_from_token_b(token_b_amount, desired_price_q64_64)?,
            token_b_amount,
        )
    };

    prepare_opening_pair(
        desired_price_q64_64,
        token_a_amount,
        token_b_amount,
        fee_bps,
    )
}

/// Pairs an edited token-A amount and validates the resulting pool creation through program logic.
pub fn prepare_opening_from_token_a(
    token_a_amount: u128,
    desired_price_q64_64: u128,
    fee_bps: u128,
) -> Result<PreparedOpeningPair, IntentError> {
    let token_b_amount = paired_amount_from_token_a(token_a_amount, desired_price_q64_64)?;
    prepare_opening_pair(
        desired_price_q64_64,
        token_a_amount,
        token_b_amount,
        fee_bps,
    )
}

/// Pairs an edited token-B amount and validates the resulting pool creation through program logic.
pub fn prepare_opening_from_token_b(
    token_b_amount: u128,
    desired_price_q64_64: u128,
    fee_bps: u128,
) -> Result<PreparedOpeningPair, IntentError> {
    let token_a_amount = paired_amount_from_token_b(token_b_amount, desired_price_q64_64)?;
    prepare_opening_pair(
        desired_price_q64_64,
        token_a_amount,
        token_b_amount,
        fee_bps,
    )
}

/// Validates explicit opening amounts and requires their Q64.64 spot price to match exactly.
pub fn validate_explicit_opening_pair(
    token_a_amount: u128,
    token_b_amount: u128,
    desired_price_q64_64: u128,
    fee_bps: u128,
) -> Result<PreparedOpeningPair, IntentError> {
    let prepared = prepare_opening_pair(
        desired_price_q64_64,
        token_a_amount,
        token_b_amount,
        fee_bps,
    )?;
    if prepared.actual_price_q64_64 != desired_price_q64_64 {
        return Err(IntentError::SpotPriceMismatch {
            desired: desired_price_q64_64,
            actual: prepared.actual_price_q64_64,
        });
    }
    Ok(prepared)
}

/// Converts caller first/second amounts to the pool's stored A/B order.
#[must_use]
pub const fn caller_amounts_to_stored(order: PairOrder, first: u128, second: u128) -> (u128, u128) {
    order.amounts_to_stored(first, second)
}

/// Converts stored pool A/B amounts back to caller first/second order.
#[must_use]
pub const fn stored_amounts_to_caller(
    order: PairOrder,
    amount_a: u128,
    amount_b: u128,
) -> (u128, u128) {
    order.amounts_from_stored(amount_a, amount_b)
}

/// Returns nonnegative directional pool spot movement in basis points for a canonical swap quote.
///
/// This computes, with one final floor operation:
///
/// `10_000 * (post_price - pre_price) / pre_price`
///
/// Reserves are oriented as input/output according to the quote direction. Intermediate products
/// use a widened integer so values remain exact even when reserve products exceed `u128`.
pub fn pool_spot_change_bps(
    before: &PoolDefinition,
    quote: &SwapQuote,
) -> Result<u128, IntentError> {
    let (pre_in, pre_out, post_in, post_out) = match quote.direction {
        SwapDirection::AToB => (
            before.reserve_a,
            before.reserve_b,
            quote.pool.reserve_a,
            quote.pool.reserve_b,
        ),
        SwapDirection::BToA => (
            before.reserve_b,
            before.reserve_a,
            quote.pool.reserve_b,
            quote.pool.reserve_a,
        ),
    };
    if [pre_in, pre_out, post_in, post_out]
        .into_iter()
        .any(|reserve| reserve == 0)
    {
        return Err(IntentError::ZeroDirectionalReserve);
    }

    let post_price_numerator = U512::from(post_in).checked_mul(U512::from(pre_out)).ok_or(
        IntentError::ArithmeticOverflow {
            operation: "directional post-price numerator",
        },
    )?;
    let relative_change_denominator = U512::from(post_out).checked_mul(U512::from(pre_in)).ok_or(
        IntentError::ArithmeticOverflow {
            operation: "directional relative-change denominator",
        },
    )?;
    let increase = post_price_numerator
        .checked_sub(relative_change_denominator)
        .ok_or(IntentError::SpotMovedAgainstSwap)?;
    let numerator =
        increase
            .checked_mul(U512::from(10_000_u128))
            .ok_or(IntentError::ArithmeticOverflow {
                operation: "directional basis-point numerator",
            })?;
    let change = numerator.checked_div(relative_change_denominator).ok_or(
        IntentError::ArithmeticOverflow {
            operation: "directional basis-point division",
        },
    )?;
    u128::try_from(change).map_err(|_| IntentError::ArithmeticOverflow {
        operation: "directional basis-point result",
    })
}

fn validate_pairing_inputs(
    edited_amount: u128,
    desired_price_q64_64: u128,
) -> Result<(), IntentError> {
    if edited_amount == 0 {
        return Err(IntentError::ZeroEditedAmount);
    }
    if desired_price_q64_64 == 0 {
        return Err(IntentError::ZeroDesiredPrice);
    }
    Ok(())
}

fn prepare_opening_pair(
    desired_price_q64_64: u128,
    token_a_amount: u128,
    token_b_amount: u128,
    fee_bps: u128,
) -> Result<PreparedOpeningPair, IntentError> {
    if desired_price_q64_64 == 0 {
        return Err(IntentError::ZeroDesiredPrice);
    }
    let quote = program_quote::create_pool(token_a_amount, token_b_amount, fee_bps)?;
    let actual_price_q64_64 = spot_price_q64_64(token_a_amount, token_b_amount);
    Ok(PreparedOpeningPair {
        desired_price_q64_64,
        actual_price_q64_64,
        token_a_amount,
        token_b_amount,
        fee_bps,
        quote,
    })
}

fn first_executable(
    upper: u128,
    mut executable: impl FnMut(u128) -> Result<bool, IntentError>,
) -> Result<u128, IntentError> {
    let mut lower = 1_u128;
    let mut upper = upper;
    while lower < upper {
        let distance = upper
            .checked_sub(lower)
            .ok_or(IntentError::ArithmeticOverflow {
                operation: "opening-pair search range",
            })?;
        let half = distance
            .checked_div(2)
            .ok_or(IntentError::ArithmeticOverflow {
                operation: "opening-pair search division",
            })?;
        let midpoint = lower
            .checked_add(half)
            .ok_or(IntentError::ArithmeticOverflow {
                operation: "opening-pair search midpoint",
            })?;
        if executable(midpoint)? {
            upper = midpoint;
        } else {
            lower = midpoint
                .checked_add(1)
                .ok_or(IntentError::ArithmeticOverflow {
                    operation: "opening-pair search increment",
                })?;
        }
    }
    Ok(lower)
}
