//! The Stablecoin Program implementation.
//!
//! Each instruction is split the way v0.3.0 splits execution. A `plan_*` function
//! runs in the plan phase, where there is no account data at all: it checks ids
//! and authorization, derives the addresses the transaction needs, enforces the
//! §8 parameter bands, and builds the chained Token calls. The remaining
//! functions are apply halves — each sees exactly one shard's bytes and returns
//! their replacement, or nothing when it is only reading.
//!
//! Fourteen of the nineteen instructions used to read `ProtocolParameters`, and
//! most of them a second or third global on top. Those reads became
//! declarations: the caller states the record and a guard on the shard it came
//! from pins the statement ([`shared::check_parameters`] and its siblings). A
//! declared value a guard pins is one the plan may branch on, which is how
//! `!is_frozen` and the bound-definition checks stay plan-phase checks even
//! though no plan can read them.
//!
//! The pins here are *exact*, not bounded, and the reason is the clock: nine of
//! these instructions gate on oracle freshness or scale accrued debt by elapsed
//! time, and a timestamp has no safe rounding direction. `now` must therefore be
//! exact, which already means those instructions are rebuilt when the block
//! advances — so exact-pinning the anchors it projects against costs no failure
//! mode that is not already paid for, and avoids choosing a safe direction per
//! field. Got wrong, that choice is a solvency bug rather than a slippage one.
//!
//! One check from v0.2.5 is *stronger* rather than relocated. A shard is keyed by
//! the program that wrote it, so "which Token Program" is no longer implied by an
//! account — it is chosen per slot by the caller. `ProtocolParameters` therefore
//! records `token_program_id` and `market_price_oracle_program_id` at bootstrap,
//! and every token and oracle slot is checked against them
//! ([`shared::assert_token_program`], [`shared::assert_bound_oracle`]).

pub use stablecoin_core as core;

/// Permissionless poke: advance the global stability-fee accumulator.
pub mod accrue_stability_fee;

/// Admin parameter updates.
pub mod admin;

/// Numerical bands and the collateralization invariant.
pub mod checks;

/// Clear a fully-settled position's shard.
pub mod close_position;

/// Deposit additional collateral into an existing position.
pub mod deposit_collateral;

/// Emergency kill switch: `freeze` / `unfreeze` the risk-increasing instructions.
pub mod freeze;

/// Mint stablecoins against a position, increasing its debt.
pub mod generate_debt;

/// Bootstrap the protocol: create the global PDAs and the stablecoin definition.
pub mod initialize_program;

/// Open a new collateral-only position for a calling owner.
pub mod open_position;

/// Permissionless combined poke: advance both globals, best-effort.
pub mod refresh_globals;

/// Repay outstanding stablecoin debt against an existing position.
pub mod repay_debt;

/// Plan-phase address checks and apply-phase guards shared across the set.
pub mod shared;

/// Permissionless poke: run one redemption-rate controller tick.
pub mod update_redemption_rate;

/// Withdraw collateral from an existing position back to a user-controlled holding.
pub mod withdraw_collateral;

#[cfg(test)]
mod tests;
