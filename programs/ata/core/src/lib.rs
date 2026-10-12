use borsh::{BorshDeserialize, BorshSerialize};
pub use lee_core::program::PdaSeed;
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{TokenDescriptor, TokenHolding, TokenKind};

/// Borsh is the instruction wire format LEZ reads, and deliberately the *only* encoding
/// this type derives.
///
/// `#[lez_program]` generates its guest-side counterpart with serde derives alongside
/// Borsh, "for IDL/tooling". Nothing uses them: `idl-gen` parses the guest source, the
/// framework's runtime IDL path deserializes its own types, and `spel-cli` encodes Borsh
/// straight from the IDL. Carrying them here would only mean a type that accepts either
/// encoder, which is how the v0.2.5 port silently rotted every instruction-encoding site in
/// `modules/*/ffi` -- `risc0_zkvm::serde::to_vec` kept compiling and kept producing bytes
/// the runtime rejects. Without a serde impl that is a compile error instead.
///
/// Borsh encodes the variant as a leading tag byte, so variants are append-only:
/// inserting one shifts the encoding of every variant after it.
#[derive(BorshSerialize, BorshDeserialize)]
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
    /// `kind` and `contents` are pinned reads, not free inputs: `contents` is
    /// checked against the ATA's actual bytes by a guard before any branch is
    /// taken on it, and `kind` against the definition.
    Create {
        token_program_id: AccountId,
        kind: TokenKind,
        contents: AtaContents,
    },

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
        token_program_id: AccountId,
        /// The asset moved. Its `definition_id` is what the ATA address derives
        /// from, so the plan needs it up front and can no longer read it out of
        /// the sender's holding.
        descriptor: TokenDescriptor,
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
        token_program_id: AccountId,
        /// See [`Instruction::Transfer`]: the ATA address derives from
        /// `definition_id`, so it is an input now.
        descriptor: TokenDescriptor,
        amount: u128,
    },
}

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

/// Payload of the guard pinning [`Instruction::Create`]'s `contents` claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckContents {
    pub descriptor: TokenDescriptor,
    pub contents: AtaContents,
}

/// Payload of the guard pinning `kind` to the token definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckDefinitionKind {
    pub kind: TokenKind,
}

/// Payload of the guard requiring an already-initialized recipient holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckRecipient {
    pub descriptor: TokenDescriptor,
}

/// What an ATA's shard actually holds, relative to the asset it is meant to hold.
///
/// `Create` is idempotent, which under v0.3.0 is awkward: the plan phase decides
/// whether to chain an `InitializeAccount` and cannot see whether the shard is
/// already populated. So the caller declares the answer and a guard checks it
/// before any branch is taken on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum AtaContents {
    /// Nothing there yet.
    Empty,
    /// Already the holding this ATA is for.
    Intended,
    /// Holds something else — foreign bytes, or a different token.
    Squatted,
}

/// Classify a shard's bytes against the asset `descriptor` names.
#[must_use]
pub fn classify(pre_data: &ShardData, descriptor: &TokenDescriptor) -> AtaContents {
    if pre_data.is_empty() {
        return AtaContents::Empty;
    }
    let Ok(holding) = TokenHolding::try_from(pre_data) else {
        return AtaContents::Squatted;
    };
    if holding.definition_id() == descriptor.definition_id
        && holds_definition_kind(holding.kind(), descriptor.kind)
    {
        AtaContents::Intended
    } else {
        AtaContents::Squatted
    }
}

/// A non-fungible definition classifies both the master and its printed copies.
const fn holds_definition_kind(holding: TokenKind, definition: TokenKind) -> bool {
    match definition {
        TokenKind::Fungible => matches!(holding, TokenKind::Fungible),
        TokenKind::NftMaster | TokenKind::NftPrintedCopy => {
            matches!(holding, TokenKind::NftMaster | TokenKind::NftPrintedCopy)
        }
    }
}

pub fn get_associated_token_account_id(ata_program_id: &AccountId, seed: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(ata_program_id, seed)
}

/// Verify the ATA's address matches `(ata_program_id, token_program_id, owner, definition)` and
/// return the [`PdaSeed`] for use in chained calls.
pub fn verify_ata_and_get_seed(
    ata_account: &AccountMeta,
    owner: &AccountMeta,
    token_program_id: AccountId,
    definition_id: AccountId,
    ata_program_id: AccountId,
) -> PdaSeed {
    let seed = compute_ata_seed(token_program_id, owner.account_id, definition_id);
    let expected_id = get_associated_token_account_id(&ata_program_id, &seed);
    assert_eq!(
        ata_account.account_id, expected_id,
        "ATA account ID does not match expected derivation"
    );
    seed
}
