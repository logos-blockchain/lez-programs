//! Committed AMM operations. Token movements also emit the token program's events.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::ProgramEvent};

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Event {
    ConfigInitialized {
        config: AccountId,
    },
    AuthorityChanged {
        authority: AccountId,
    },
    PoolCreated {
        amount_a: u128,
        amount_b: u128,
        liquidity: u128,
    },
    LiquidityAdded {
        amount_a: u128,
        amount_b: u128,
        liquidity: u128,
    },
    LiquidityRemoved {
        amount_a: u128,
        amount_b: u128,
        liquidity: u128,
    },
    Swapped {
        token_in: AccountId,
        amount_in: u128,
        amount_out: u128,
        protocol_fee: u128,
    },
    ReservesSynced {
        reserve_a: u128,
        reserve_b: u128,
    },
    ProtocolFeesWithdrawn {
        token: AccountId,
        destination: AccountId,
        amount: u128,
    },
}

impl Event {
    #[must_use]
    pub fn to_program_event(&self) -> ProgramEvent {
        use risc0_zkvm::sha::{Impl, Sha256};
        let hash = Impl::hash_bytes(b"amm::Event");
        let (selector, _) = hash.as_bytes().split_at(8);
        ProgramEvent {
            selector: selector.try_into().expect("eight selector bytes"),
            data: borsh::to_vec(self).expect("borsh serialization is infallible"),
        }
    }
}
