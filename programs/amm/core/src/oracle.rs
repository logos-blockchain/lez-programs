//! AMM-facing oracle message contract. The oracle actor lives at `(pool, oracle_program)`;
//! the AMM authorizes that pool address with its seed. The integration suite supplies a
//! recording mock. This module does not implement a TWAP oracle or publish trusted prices.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;

/// Retains the AMM's minimum oracle window until the full oracle is ported.
pub const MIN_WINDOW_DURATION: u64 = 2048;

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    CreateCurrentTick {
        price: u128,
    },
    UpdateCurrentTick {
        price: u128,
    },
    CreatePriceObservations {
        window_duration: u64,
    },
    CreateOraclePriceAccount {
        base_asset: AccountId,
        quote_asset: AccountId,
        initial_price: u128,
        window_duration: u64,
    },
}
