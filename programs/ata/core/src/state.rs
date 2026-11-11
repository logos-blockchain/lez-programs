use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;

use crate::TransferRequest;

/// Transient state on the recipient's ATA actor. Private holding snapshots stay in
/// private execution. Successful transactions restore empty state; the self-only
/// idle assertion enforces completion even when a selected asset program never replies.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AtaState {
    ReadingSource(TransferRequest),
    ReadingRecipient {
        request: TransferRequest,
        source: Holding,
    },
    VerifyingRecipient {
        asset_program: AccountId,
        definition: AccountId,
        expected: Holding,
    },
}

impl AtaState {
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            None
        } else {
            Some(borsh::from_slice(bytes).expect("ATA actor state must decode"))
        }
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("borsh serialization is infallible")
    }
}

/// A snapshot from an authenticated asset-program reply, after checking its definition.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Holding {
    Token(u128),
    Master(u128),
    Copy(bool),
}
