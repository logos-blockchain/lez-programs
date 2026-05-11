//! The Stablecoin Program implementation.

pub use stablecoin_core as core;

/// No-op instruction used as a heartbeat / sanity-check entry point.
pub mod noop;
/// Open a new collateral-only position for a calling owner.
pub mod open_position;

#[cfg(test)]
mod tests;
