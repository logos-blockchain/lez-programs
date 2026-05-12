//! The Stablecoin Program implementation.

pub use stablecoin_core as core;

/// Open a new collateral-only position for a calling owner.
pub mod open_position;

#[cfg(test)]
mod tests;
