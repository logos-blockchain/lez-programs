//! The AMM Program implementation.
//!
//! Runtime handlers live in instruction-named modules. Host applications should use [`quote`] for
//! fallible deterministic previews backed by the same arithmetic as those handlers.

pub use amm_core as core;

pub mod add;
pub mod create_oracle_price_account;
pub mod create_price_observations;
pub mod initialize;
pub mod new_definition;
pub mod quote;
pub mod remove;
pub mod swap;
pub mod sync;
pub mod update_config;

mod tests;
