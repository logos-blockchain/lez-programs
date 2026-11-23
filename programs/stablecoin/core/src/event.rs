//! Events, emitted by the actor whose state changed. The indexer records the emitting
//! program, not the account, so every event names its accounts.
//!
//! Collateral and open/close events come from the position actor; debt events and the rest
//! from the engine.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::ProgramEvent};
use risc0_zkvm::sha::{Impl, Sha256 as _};

use crate::ProtocolParameters;

pub trait StablecoinEvent: BorshSerialize {
    const NAME: &'static str;

    #[must_use]
    fn to_program_event(&self) -> ProgramEvent {
        ProgramEvent {
            selector: selector(Self::NAME),
            data: borsh::to_vec(self).expect("borsh serialization is infallible"),
        }
    }
}

/// `sha256("stablecoin::<name>")[..8]`, the LEZ selector convention.
#[must_use]
pub fn selector(name: &str) -> [u8; 8] {
    let digest = Impl::hash_bytes(format!("stablecoin::{name}").as_bytes());
    *digest
        .as_bytes()
        .first_chunk::<8>()
        .expect("a sha256 digest has 32 bytes")
}

/// Emitted by the engine once initialized.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Initialized {
    pub engine: AccountId,
    pub stablecoin_definition: AccountId,
}

impl StablecoinEvent for Initialized {
    const NAME: &'static str = "Initialized";
}

/// Emitted by the position actor when it opens.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PositionOpened {
    pub owner: AccountId,
    pub vault: AccountId,
    pub collateral_amount: u128,
}

impl StablecoinEvent for PositionOpened {
    const NAME: &'static str = "PositionOpened";
}

/// Emitted by the position actor.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralDeposited {
    pub owner: AccountId,
    pub amount: u128,
}

impl StablecoinEvent for CollateralDeposited {
    const NAME: &'static str = "CollateralDeposited";
}

/// Emitted by the position actor.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CollateralWithdrawn {
    pub owner: AccountId,
    pub to: AccountId,
    pub amount: u128,
}

impl StablecoinEvent for CollateralWithdrawn {
    const NAME: &'static str = "CollateralWithdrawn";
}

/// Emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DebtGenerated {
    pub owner: AccountId,
    pub to: AccountId,
    pub amount: u128,
    pub normalized_debt_delta: u128,
}

impl StablecoinEvent for DebtGenerated {
    const NAME: &'static str = "DebtGenerated";
}

/// Emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DebtRepaid {
    pub owner: AccountId,
    pub amount_burned: u128,
    pub normalized_debt_delta: u128,
}

impl StablecoinEvent for DebtRepaid {
    const NAME: &'static str = "DebtRepaid";
}

/// Emitted by the position actor when it closes.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PositionClosed {
    pub owner: AccountId,
}

impl StablecoinEvent for PositionClosed {
    const NAME: &'static str = "PositionClosed";
}

/// Emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StabilityFeeAccrued {
    pub accumulated_rate: u128,
    pub accrued_fees: u128,
    pub now: u64,
}

impl StablecoinEvent for StabilityFeeAccrued {
    const NAME: &'static str = "StabilityFeeAccrued";
}

/// Emitted by the engine.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct RedemptionRateUpdated {
    pub redemption_price: u128,
    pub redemption_rate_per_millisecond: u128,
    pub now: u64,
}

impl StablecoinEvent for RedemptionRateUpdated {
    const NAME: &'static str = "RedemptionRateUpdated";
}

/// Emitted by the engine after any parameter change.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ParametersChanged {
    pub parameters: ProtocolParameters,
}

impl StablecoinEvent for ParametersChanged {
    const NAME: &'static str = "ParametersChanged";
}

/// Emitted by the engine on freeze and unfreeze.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FrozenChanged {
    pub is_frozen: bool,
}

impl StablecoinEvent for FrozenChanged {
    const NAME: &'static str = "FrozenChanged";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_the_documented_hash() {
        let digest = Impl::hash_bytes(b"stablecoin::Initialized");
        assert_eq!(
            digest.as_bytes().first_chunk::<8>(),
            Some(&selector(Initialized::NAME))
        );
        let event = Initialized {
            engine: AccountId::new([1; 32]),
            stablecoin_definition: AccountId::new([2; 32]),
        };
        let program_event = event.to_program_event();
        assert_eq!(program_event.selector, selector("Initialized"));
        assert_eq!(program_event.data, borsh::to_vec(&event).unwrap());
    }

    #[test]
    fn every_event_has_its_own_selector() {
        let names = [
            Initialized::NAME,
            PositionOpened::NAME,
            CollateralDeposited::NAME,
            CollateralWithdrawn::NAME,
            DebtGenerated::NAME,
            DebtRepaid::NAME,
            PositionClosed::NAME,
            StabilityFeeAccrued::NAME,
            RedemptionRateUpdated::NAME,
            ParametersChanged::NAME,
            FrozenChanged::NAME,
        ];
        let selectors: std::collections::HashSet<_> = names.iter().map(|n| selector(n)).collect();
        assert_eq!(selectors.len(), names.len());
    }
}
