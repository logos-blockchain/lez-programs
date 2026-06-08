//! The Stablecoin Program implementation.

pub use stablecoin_core as core;

/// Open a new collateral-only position for a calling owner.
pub mod open_position;

/// Initialize and update redemption-rate feedback controller state.
pub mod redemption_controller;

/// Repay outstanding stablecoin debt against an existing position.
pub mod repay_debt;

/// Withdraw collateral from an existing position back to a user-controlled holding.
pub mod withdraw_collateral;

#[cfg(test)]
mod tests;
