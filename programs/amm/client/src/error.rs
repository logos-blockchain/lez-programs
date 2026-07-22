use std::{error::Error, fmt};

use nssa_core::{account::AccountId, program::ProgramId};

/// Failure while validating AMM client input or constructing a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ClientError {
    /// An account ID differs from its canonical or stored value.
    AccountIdMismatch {
        account: &'static str,
        expected: AccountId,
        actual: AccountId,
    },
    /// An account owner differs from the owner required by the program.
    ProgramOwnerMismatch {
        account: &'static str,
        expected: ProgramId,
        actual: ProgramId,
    },
    /// Account bytes cannot be decoded as the required program type.
    InvalidAccountData {
        account: &'static str,
        expected: &'static str,
    },
    /// A token account is not a fungible holding.
    ExpectedFungibleToken { account: &'static str },
    /// A token holding points at the wrong definition.
    TokenDefinitionMismatch {
        account: &'static str,
        expected: AccountId,
        actual: AccountId,
    },
    /// A holding cannot cover the amount required by a quoted operation.
    InsufficientBalance {
        account: &'static str,
        available: u128,
        required: u128,
    },
    /// A pool was requested with the same token on both sides.
    IdenticalTokenDefinitions,
    /// Slippage basis points exceed one whole quoted amount.
    SlippageToleranceOutOfRange { bps: u128, maximum_bps: u128 },
    /// A slippage-adjusted upper guard exceeds the chain amount range.
    SlippageBoundOverflow {
        quoted_amount: u128,
        slippage_bps: u128,
    },
    /// Program-owned quote logic rejected the requested transition.
    Quote {
        code: &'static str,
        message: &'static str,
    },
}

impl ClientError {
    /// Stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AccountIdMismatch { .. } => "account_id_mismatch",
            Self::ProgramOwnerMismatch { .. } => "program_owner_mismatch",
            Self::InvalidAccountData { .. } => "invalid_account_data",
            Self::ExpectedFungibleToken { .. } => "expected_fungible_token",
            Self::TokenDefinitionMismatch { .. } => "token_definition_mismatch",
            Self::InsufficientBalance { .. } => "insufficient_balance",
            Self::IdenticalTokenDefinitions => "identical_token_definitions",
            Self::SlippageToleranceOutOfRange { .. } => "slippage_tolerance_out_of_range",
            Self::SlippageBoundOverflow { .. } => "slippage_bound_overflow",
            Self::Quote { code, .. } => code,
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountIdMismatch { account, .. } => {
                write!(formatter, "{account} account ID mismatch")
            }
            Self::ProgramOwnerMismatch { account, .. } => {
                write!(formatter, "{account} program owner mismatch")
            }
            Self::InvalidAccountData { account, expected } => {
                write!(
                    formatter,
                    "{account} does not contain valid {expected} data"
                )
            }
            Self::ExpectedFungibleToken { account } => {
                write!(formatter, "{account} must be a fungible token holding")
            }
            Self::TokenDefinitionMismatch { account, .. } => {
                write!(formatter, "{account} token definition mismatch")
            }
            Self::InsufficientBalance {
                account,
                available,
                required,
            } => write!(
                formatter,
                "{account} balance {available} is less than required amount {required}"
            ),
            Self::IdenticalTokenDefinitions => {
                formatter.write_str("pool token definitions must be distinct")
            }
            Self::SlippageToleranceOutOfRange {
                bps,
                maximum_bps,
            } => write!(
                formatter,
                "slippage tolerance {bps} bps exceeds maximum {maximum_bps} bps"
            ),
            Self::SlippageBoundOverflow {
                quoted_amount,
                slippage_bps,
            } => write!(
                formatter,
                "slippage-adjusted upper guard for {quoted_amount} at {slippage_bps} bps exceeds u128"
            ),
            Self::Quote { message, .. } => formatter.write_str(message),
        }
    }
}

impl Error for ClientError {}
