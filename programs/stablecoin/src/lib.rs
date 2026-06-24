//! The Stablecoin Program implementation.

pub use stablecoin_core as core;

/// Roll the global stability-fee accumulator forward.
pub mod accrue_stability_fee;

/// Mint stablecoin debt against an existing position.
pub mod generate_debt;

/// Initialize protocol globals and the stablecoin token definition.
pub mod initialize_program;

/// Open a new collateral-only position for a calling owner.
pub mod open_position;

/// Repay outstanding stablecoin debt against an existing position.
pub mod repay_debt;

/// Update the stability-fee rate after accruing at the old rate.
pub mod set_stability_fee_per_millisecond;

mod shared;

/// Withdraw collateral from an existing position back to a user-controlled holding.
pub mod withdraw_collateral;

#[cfg(test)]
mod tests;
