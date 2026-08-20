//! The Stablecoin Program implementation.

pub use stablecoin_core as core;

/// Permissionless poke: advance the global stability-fee accumulator.
pub mod accrue_stability_fee;

/// Bootstrap the protocol: create the global PDAs and the stablecoin definition.
pub mod initialize_program;

/// Open a new collateral-only position for a calling owner.
pub mod open_position;

/// Permissionless combined poke: advance both globals, best-effort.
pub mod refresh_globals;

/// Repay outstanding stablecoin debt against an existing position.
pub mod repay_debt;

/// Permissionless poke: run one redemption-rate controller tick.
pub mod update_redemption_rate;

/// Withdraw collateral from an existing position back to a user-controlled holding.
pub mod withdraw_collateral;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
