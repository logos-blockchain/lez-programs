use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::account::{AccountId, Data};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// TWAP Oracle Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// No-op instruction. Does nothing and returns no state changes.
    Noop,
}

/// Canonical oracle price account consumed by LEZ programs.
///
/// Oracle producers own how this account is written; consumers only read and validate it.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct OraclePriceAccount {
    /// Canonical identifier for the priced asset.
    pub base_asset: AccountId,
    /// Canonical identifier for the quote asset that denominates `price`.
    pub quote_asset: AccountId,
    /// Amount of `quote_asset` one unit of `base_asset` is worth.
    ///
    /// `u128` keeps the consumer-side interface non-negative; zero is rejected on read.
    pub price: u128,
    /// Price observation timestamp. Consumers choose the time unit by matching this with
    /// `max_age`.
    pub timestamp: u64,
    /// Identifier of the source that populated this account, such as a TWAP or external adaptor.
    pub source_id: String,
    /// Source-provided confidence interval, or zero when the source does not provide one.
    pub confidence_interval: u128,
}

impl TryFrom<&Data> for OraclePriceAccount {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&OraclePriceAccount> for Data {
    fn from(price_account: &OraclePriceAccount) -> Self {
        let serialized_len =
            borsh::object_length(price_account).expect("Oracle price account length must be known");
        let mut data = Vec::with_capacity(serialized_len);
        BorshSerialize::serialize(price_account, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Oracle price account encoded data should fit into Data")
    }
}
