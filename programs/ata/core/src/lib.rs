//! Associated holdings for fungible tokens and NFTs on LEZ's actor interface.
//!
//! Public PDA derivation is unchanged. Use deployed program account ids, not image ids.
//! Message tags and state layouts are append-only within this protocol.

use lee_core::account::AccountId;
pub use lee_core::program::PdaSeed;

mod message;
mod state;
pub use message::*;
pub use state::*;

pub fn compute_ata_seed(
    token_program_id: AccountId,
    owner_id: AccountId,
    definition_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};
    let mut bytes = [0u8; 96];
    let (program_id_bytes, rest) = bytes.split_at_mut(32);
    let (owner_bytes, definition_bytes) = rest.split_at_mut(32);
    program_id_bytes.copy_from_slice(&token_program_id.to_bytes());
    owner_bytes.copy_from_slice(&owner_id.to_bytes());
    definition_bytes.copy_from_slice(&definition_id.to_bytes());
    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn get_associated_token_account_id(ata_program_id: &AccountId, seed: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(ata_program_id, seed)
}
