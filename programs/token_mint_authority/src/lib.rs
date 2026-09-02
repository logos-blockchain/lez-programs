//! The Token-Mint-Authority Program implementation.
//!
//! A permissionless testnet faucet that delegates to the Token Program:
//! `user -> token-mint-authority -> token`. See [`token_mint_authority_core`] for the account
//! contract and the mint-authority setup requirement.

pub use token_mint_authority_core as core;

/// Mint a fixed grant of the faucet token to the caller, rate-limited per day.
pub mod faucet_mint;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
