//! Stateless oracle client contracts. Wallet and network I/O belong to the host.
#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

#[cfg(test)]
mod tests;

pub use account::{decode_account, AccountRead, WalletAccount};
pub use api::{
    feed_addresses, program_info, validate_clock, FeedAddressesRequest, OracleApiError,
    OracleResult, ProgramInfoRequest, TransactionPlan,
};
