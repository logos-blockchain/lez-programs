//! Events, emitted by the actor whose state changed. The indexer records the emitting
//! program, not the account, so every event names its accounts.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::ProgramEvent};
use risc0_zkvm::sha::{Impl, Sha256 as _};

use crate::PriceSource;

#[cfg_attr(feature = "idl", spel_attrs::event)]
pub trait OracleEvent: BorshSerialize {
    const NAME: &'static str;

    #[must_use]
    fn to_program_event(&self) -> ProgramEvent {
        ProgramEvent {
            selector: selector(Self::NAME),
            data: borsh::to_vec(self).expect("borsh serialization is infallible"),
        }
    }
}

/// `sha256("twap_oracle::<name>")[..8]`, the LEZ selector convention.
#[must_use]
pub fn selector(name: &str) -> [u8; 8] {
    let digest = Impl::hash_bytes(format!("twap_oracle::{name}").as_bytes());
    *digest
        .as_bytes()
        .first_chunk::<8>()
        .expect("a sha256 digest has 32 bytes")
}

/// Emitted by the new current-tick actor.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CurrentTickAccountCreated {
    pub current_tick: AccountId,
    pub source: PriceSource,
    pub tick: i32,
}

impl OracleEvent for CurrentTickAccountCreated {
    const NAME: &'static str = "CurrentTickAccountCreated";
}

/// Emitted by the current-tick actor once it sent both creations.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PriceAccountsCreated {
    pub current_tick: AccountId,
    pub window_duration: u64,
    pub price_observations: AccountId,
    pub oracle_price: AccountId,
}

impl OracleEvent for PriceAccountsCreated {
    const NAME: &'static str = "PriceAccountsCreated";
}

/// Emitted by the observations actor when it appends an entry.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Recorded {
    pub price_observations: AccountId,
    pub window_duration: u64,
    pub timestamp: u64,
    /// The clamped tick, as recorded.
    pub tick: i32,
}

impl OracleEvent for Recorded {
    const NAME: &'static str = "Recorded";
}

/// Emitted by the price actor when it takes a new price.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Published {
    pub oracle_price: AccountId,
    pub window_duration: u64,
    pub price: u128,
    pub timestamp: u64,
}

impl OracleEvent for Published {
    const NAME: &'static str = "Published";
}
