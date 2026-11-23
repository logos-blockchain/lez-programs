//! A position: the stablecoin's actor state on the owner's own account.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};

/// One owner's collateral and debt. One position per account.
///
/// The position trusts its own `collateral_amount`: the vault is never read, so tokens sent
/// straight to a vault are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Position {
    /// The token program the vault lives in, pinned when the position opened.
    pub token_program_id: AccountId,
    /// The collateral token's definition, pinned when the position opened. A deposit
    /// initializes the vault's holding of it, which pins the vault into the transaction.
    pub collateral_definition_id: AccountId,
    /// Collateral held in the position's vault, in raw collateral units.
    pub collateral_amount: u128,
    /// Debt divided by the stability fee accumulator at the time it was taken, so the debt
    /// grows with the accumulator without touching the position.
    pub normalized_debt_amount: u128,
}
