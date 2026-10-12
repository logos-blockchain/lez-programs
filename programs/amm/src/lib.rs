//! The AMM Program implementation.
//!
//! Each instruction is split the way v0.3.0 splits execution. A `plan_*` function
//! runs in the plan phase, where there is no account data at all: it checks ids
//! and authorization, derives the addresses the transaction needs, and builds the
//! chained calls. The remaining functions are apply halves — each sees exactly
//! one shard's bytes and returns their replacement, or nothing at all when it is
//! only reading.
//!
//! Nine of the eleven instructions used to read the config account and six read
//! the pool. Those reads became declarations: a caller states the values and an
//! effect on the shard they came from pins the statement
//! ([`amm_core::check_config`], [`shared::pool_of`]). Where a figure is a
//! *function* of what it declares — a swap's output, a liquidity add's LP — the
//! pin is a bound rather than an equality, so the trade still executes when the
//! pool moved under the quote and the surplus stays with the pool. Where a figure
//! leaves the AMM for someone else to act on — an oracle price — the pin is exact.
//!
//! The vault-balance reads are gone outright, not relocated. Only this program can
//! debit a vault and every reserve change it records is paired with an equal
//! transfer, so the backing those reads re-derived is structural; `SyncReserves`,
//! which still reads them, is the repair path.

pub use amm_core as core;

pub mod add;
pub mod create_oracle_price_account;
pub mod create_price_observations;
pub mod initialize;
pub mod new_definition;
pub mod remove;
pub mod shared;
pub mod swap;
pub mod sync;
pub mod update_config;
pub mod withdraw_protocol_fees;

mod tests;
