pub use nssa_core::program::PdaSeed;
use nssa_core::{
    account::{AccountId, AccountWithMetadata},
    program::ProgramId,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum Instruction {
    /// Create the Associated Token Account for (token program, owner, definition).
    /// Idempotent: no-op if the account already exists.
    ///
    /// Required accounts (3):
    /// - Owner account
    /// - Token definition account
    /// - Associated token account (default/uninitialized, or already initialized)
    ///
    /// `token_program_id` is explicit so callers can support multiple token programs without
    /// letting account metadata choose downstream code.
    Create { token_program_id: ProgramId },

    /// Transfer tokens FROM owner's ATA to a recipient token holding account.
    /// Uses ATA PDA seeds to authorize the chained Token::Transfer call.
    ///
    /// Required accounts (3):
    /// - Owner account (authorized)
    /// - Sender ATA (owner's token holding)
    /// - Recipient token holding. Must be:
    ///   - already initialized (not a default account),
    ///   - owned by the same token program as the sender ATA,
    ///   - and point at the same token definition as the sender.
    ///
    /// `token_program_id` is explicit so callers can support multiple token programs without
    /// letting account metadata choose downstream code.
    Transfer {
        token_program_id: ProgramId,
        amount: u128,
    },

    /// Burn tokens FROM owner's ATA.
    /// Uses PDA seeds to authorize the ATA in the chained Token::Burn call.
    ///
    /// Required accounts (3):
    /// - Owner account (authorized)
    /// - Owner's ATA (the holding to burn from)
    /// - Token definition account
    ///
    /// `token_program_id` is explicit so callers can support multiple token programs without
    /// letting account metadata choose downstream code.
    Burn {
        token_program_id: ProgramId,
        amount: u128,
    },
}

pub fn compute_ata_seed(
    token_program_id: ProgramId,
    owner_id: AccountId,
    definition_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256};
    let mut bytes = [0u8; 96];
    for (index, word) in token_program_id.iter().enumerate() {
        let offset = index * 4;
        bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes[32..64].copy_from_slice(&owner_id.to_bytes());
    bytes[64..96].copy_from_slice(&definition_id.to_bytes());
    PdaSeed::new(
        Impl::hash_bytes(&bytes)
            .as_bytes()
            .try_into()
            .expect("Hash output must be exactly 32 bytes long"),
    )
}

pub fn get_associated_token_account_id(ata_program_id: &ProgramId, seed: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(ata_program_id, seed)
}

/// Verify the ATA's address matches `(ata_program_id, token_program_id, owner, definition)` and
/// return the [`PdaSeed`] for use in chained calls.
pub fn verify_ata_and_get_seed(
    ata_account: &AccountWithMetadata,
    owner: &AccountWithMetadata,
    token_program_id: ProgramId,
    definition_id: AccountId,
    ata_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_ata_seed(token_program_id, owner.account_id, definition_id);
    let expected_id = get_associated_token_account_id(&ata_program_id, &seed);
    assert_eq!(
        ata_account.account_id, expected_id,
        "ATA account ID does not match expected derivation"
    );
    seed
}
