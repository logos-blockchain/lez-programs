//! Stateless oracle client contracts. Wallet and network I/O belong to the host.
#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

#[cfg(test)]
mod tests;

pub use account::{decode_account, AccountRead, WalletAccount};
pub use api::{
    create_current_tick_account, create_oracle_price_account, create_price_observations,
    feed_addresses, program_info, publish_price, record_tick, update_current_tick, validate_clock,
    FeedAddressesRequest, OracleApiError, OracleResult, ProgramInfoRequest, TransactionPlan,
};
