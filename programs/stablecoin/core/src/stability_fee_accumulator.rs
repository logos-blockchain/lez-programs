//! Global lazy accumulator for stability fees, advanced by every action that accrues fees and
//! read by every debt-touching action to compute nominal debt from normalized debt.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Compounded stability-fee multiplier, lazy form.
///
/// The current accumulator at time `now` is
/// `accumulated_rate_at_last_accrual * compound_rate(stability_fee_per_millisecond, now -
/// last_accrued_at) / FIXED_POINT_ONE`. See spec §5.3 for the read-side projection.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct StabilityFeeAccumulator {
    /// Accumulator value at [`Self::last_accrued_at`], fixed-point. Initialized
    /// to `FIXED_POINT_ONE` and monotonically non-decreasing.
    pub accumulated_rate_at_last_accrual: u128,
    /// Unix milliseconds of the last accrual.
    pub last_accrued_at: u64,
    /// Sum of every position's `normalized_debt_amount` (RAI's `debtAmount`).
    ///
    /// Each accrual mints `total_normalized_debt × Δaccumulator / FIXED_POINT_ONE`
    /// stablecoin to the protocol's surplus holding, so the supply keeps pace with
    /// the total nominal debt (spec §6.6, RFP R1). `generate_debt` and
    /// `repay_debt` keep it in step with the positions.
    pub total_normalized_debt: u128,
}
