//! The engine actor: global parameters, fee accumulator, redemption price state, and the one
//! operation it is waiting on.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use serde::{Deserialize, Serialize};

use crate::{ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator};

/// The engine's state, on one PDA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Engine {
    pub parameters: ProtocolParameters,
    pub fee: StabilityFeeAccumulator,
    pub redemption: RedemptionPriceState,
    /// The operation waiting for a reply; `None` between transactions.
    pub pending: Option<Pending>,
}

/// What the engine is waiting on. One at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum Pending {
    // Waiting for the clock.
    Initialize,
    AccrueStabilityFee,
    UpdateRedemptionRate,
    RefreshGlobals {
        consult_oracle: bool,
    },
    SetStabilityFee {
        new_rate: u128,
    },
    ResetController,
    GenerateDebt {
        position: AccountId,
        amount: u128,
        to: AccountId,
        collateral_amount: u128,
        normalized_debt_amount: u128,
    },
    RepayDebt {
        position: AccountId,
        maximum_amount: u128,
        from_holding: AccountId,
        normalized_debt_amount: u128,
    },
    Withdraw {
        position: AccountId,
        amount: u128,
        to: AccountId,
        collateral_after: u128,
        normalized_debt_amount: u128,
    },
    // Waiting for the oracle; the clock time is already known.
    OraclePrice {
        now: u64,
        hard: bool,
    },
}
