//! This crate contains core data structures and utilities for the Token Program.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, ShardData};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// Token Program Instruction.
///
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
/// Account-data types below keep serde; the FFI renders them as JSON. This applies to the
/// instruction enum alone.
///
/// Borsh encodes the variant as a leading tag byte, so variants are append-only:
/// inserting one shifts the encoding of every variant after it.
#[derive(BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    /// Transfer tokens from sender to recipient.
    ///
    /// Required accounts:
    /// - Sender's Token Holding account (initialized, authorized),
    /// - Recipient's Token Holding account (initialized, or empty).
    ///
    /// `descriptor` names the asset being moved. It is a pinned read, not a free
    /// input: each side's apply asserts its own holding matches it, so a wrong
    /// descriptor aborts rather than moving something else. See
    /// [`TokenDescriptor`].
    Transfer {
        amount_to_transfer: u128,
        descriptor: TokenDescriptor,
    },

    /// Create a new fungible token definition without metadata.
    ///
    /// `mint_authority` decides the supply model:
    /// - `Some(id)` — `id` may mint additional supply and rotate/renounce the authority,
    /// - `None` — supply is permanently fixed at `total_supply`.
    ///
    /// Required accounts:
    /// - Token Definition account (uninitialized, authorized),
    /// - Token Holding account (uninitialized, authorized).
    NewFungibleDefinition {
        name: String,
        total_supply: u128,
        mint_authority: Option<AccountId>,
    },

    /// Create a new fungible or non-fungible token definition with metadata.
    ///
    /// Required accounts:
    /// - Token Definition account (uninitialized, authorized),
    /// - Token Holding account (uninitialized, authorized),
    /// - Token Metadata account (uninitialized, authorized).
    NewDefinitionWithMetadata {
        new_definition: NewTokenDefinition,
        /// Boxed to avoid large enum variant size
        metadata: Box<NewTokenMetadata>,
    },

    /// Initialize a token holding account for a given token definition.
    ///
    /// Required accounts:
    /// - Token Definition account (initialized),
    /// - Token Holding account (empty, or authorized),
    ///
    /// `kind` is a pinned read of the definition: the holding's apply never sees
    /// the definition, so the definition's own apply guards the claim.
    InitializeAccount { kind: TokenKind },

    /// Burn tokens from the holder's account.
    ///
    /// Required accounts:
    /// - Token Definition account (initialized),
    /// - Token Holding account (authorized).
    ///
    /// `kind` picks which supply the definition decrements, so it crosses between
    /// the two shards; each side asserts it against its own contents.
    Burn {
        amount_to_burn: u128,
        kind: TokenKind,
    },

    /// Mint new tokens to the holder's account under **self/PDA authority**: the
    /// Token Definition account itself is the current mint authority and must be
    /// authorized in this transaction (signer, or a PDA authorized under its
    /// seeds). A definition with no authority has a fixed supply and rejects
    /// minting.
    ///
    /// Required accounts:
    /// - Token Definition account (initialized, authorized as the current mint authority),
    /// - Token Holding account (uninitialized or authorized and initialized).
    Mint { amount_to_mint: u128 },

    /// Mint new tokens under an **external authority**: a distinct authority
    /// account (the account the mint authority was rotated to) authorizes the
    /// mint by signing, while the Token Definition account is mutated but does
    /// not sign. Its account id must match the definition's stored authority.
    ///
    /// Required accounts:
    /// - Token Definition account (initialized),
    /// - Token Holding account (uninitialized or authorized and initialized),
    /// - Authority account (authorized as the current mint authority).
    MintWithAuthority { amount_to_mint: u128 },

    /// Print a new NFT from the master copy.
    ///
    /// Required accounts:
    /// - NFT Master Token Holding account (authorized),
    /// - NFT Printed Copy Token Holding account (empty).
    ///
    /// `definition_id` is a pinned read of the master holding, which the printed
    /// copy's apply never sees.
    PrintNft { definition_id: AccountId },

    /// Rotate or renounce the mint authority under **self/PDA authority**: the
    /// Token Definition account itself is the current authority and must be
    /// authorized in this transaction. Pass `new_authority: None` to permanently
    /// renounce minting (fixed supply).
    ///
    /// Required accounts:
    /// - Token Definition account (initialized, authorized as the current mint authority).
    SetAuthority { new_authority: Option<AccountId> },

    /// Rotate or renounce the mint authority under an **external authority**: a
    /// distinct authority account (the account the authority was rotated to)
    /// authorizes the change by signing, while the Token Definition account is
    /// mutated but does not sign. Pass `new_authority: None` to permanently renounce.
    ///
    /// Required accounts:
    /// - Token Definition account (initialized),
    /// - Authority account (authorized as the current mint authority).
    SetAuthorityWithAuthority { new_authority: Option<AccountId> },
}

#[derive(Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum NewTokenDefinition {
    Fungible {
        name: String,
        total_supply: u128,
        /// Mint authority. `Some(id)` makes the token mintable by `id`; `None`
        /// fixes the supply.
        mint_authority: Option<AccountId>,
    },
    NonFungible {
        name: String,
        printable_supply: u128,
    },
}

#[account_type]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum TokenDefinition {
    Fungible {
        name: String,
        total_supply: u128,
        metadata_id: Option<AccountId>,
        /// Mint authority slot. `Some(id)` may mint and rotate/renounce;
        /// `None` means the supply is permanently fixed.
        ///
        /// Stored directly as `Option<AccountId>` (Borsh-identical to a custom
        /// authority newtype) so account state stays decodable by `spel inspect`.
        /// The require/rotate/renounce guard logic lives inline in the `mint` and
        /// `set_authority` handlers.
        authority: Option<AccountId>,
    },
    NonFungible {
        name: String,
        printable_supply: u128,
        metadata_id: AccountId,
    },
}

impl TryFrom<&ShardData> for TokenDefinition {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        TokenDefinition::try_from_slice(data.as_ref())
    }
}

impl From<&TokenDefinition> for ShardData {
    fn from(definition: &TokenDefinition) -> Self {
        // Using size_of_val as size hint for Vec allocation
        let mut data = Vec::with_capacity(std::mem::size_of_val(definition));

        BorshSerialize::serialize(definition, &mut data)
            .expect("Serialization to Vec should not fail");

        ShardData::try_from(data).expect("Token definition encoded data should fit into ShardData")
    }
}

#[account_type]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum TokenHolding {
    Fungible {
        definition_id: AccountId,
        balance: u128,
    },
    NftMaster {
        definition_id: AccountId,
        /// The amount of printed copies left - 1 (1 reserved for master copy itself).
        print_balance: u128,
    },
    NftPrintedCopy {
        definition_id: AccountId,
        /// Whether nft is owned by the holder.
        owned: bool,
    },
}

/// Payload of the sender-side effect of a [`Instruction::Transfer`].
///
/// One struct per effect, rather than one enum: `#[apply]` takes the payload as
/// its first argument and SPEL derives the program's `Effect` enum from the set
/// of apply functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Withdraw {
    pub descriptor: TokenDescriptor,
    pub amount: u128,
}

/// Payload of the recipient-side effect of a [`Instruction::Transfer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Deposit {
    pub descriptor: TokenDescriptor,
    pub amount: u128,
}

/// Payload for writing a whole record into an empty shard.
///
/// Used by the `New*Definition` instructions, which read nothing: every field of
/// the record comes from the instruction or from an account id the plan can see,
/// so the plan builds the bytes and the apply only has to refuse to overwrite.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Create {
    pub data: ShardData,
}

/// Payload of the definition-side guard of [`Instruction::InitializeAccount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CheckHoldingKind {
    pub kind: TokenKind,
}

/// Payload of the holding-side effect of [`Instruction::InitializeAccount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct InitializeHolding {
    pub descriptor: TokenDescriptor,
    pub is_authorized: bool,
}

/// Payload of the definition-side effect of the two `Mint` instructions.
///
/// `authority_id` and `authority_is_authorized` are what the plan saw of the
/// account claiming authority. The stored authority lives in this shard and
/// nowhere else, so the comparison can only happen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct MintSupply {
    pub amount: u128,
    pub authority_id: AccountId,
    pub authority_is_authorized: bool,
}

/// Payload of the definition-side effect of [`Instruction::Burn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BurnSupply {
    pub kind: TokenKind,
    pub amount: u128,
}

/// Payload of the holding-side effect of [`Instruction::Burn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BurnHolding {
    pub descriptor: TokenDescriptor,
    pub amount: u128,
}

/// Payload of the master-side effect of [`Instruction::PrintNft`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PrintCopy {
    pub definition_id: AccountId,
}

/// Payload of the definition-side effect of the two `SetAuthority` instructions.
///
/// Carries the same plan-observed authority pair as [`MintSupply`], for the same
/// reason: the current authority is stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SetMintAuthority {
    pub new_authority: Option<AccountId>,
    pub authority_id: AccountId,
    pub authority_is_authorized: bool,
}

/// Which shape of [`TokenHolding`] a shard is expected to hold.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum TokenKind {
    Fungible,
    NftMaster,
    NftPrintedCopy,
}

impl TokenKind {
    #[must_use]
    pub const fn from_definition(definition: &TokenDefinition) -> Self {
        match definition {
            TokenDefinition::Fungible { .. } => Self::Fungible,
            TokenDefinition::NonFungible { .. } => Self::NftPrintedCopy,
        }
    }
}

/// The asset an instruction claims to be moving: which definition, and which
/// shape of holding.
///
/// This is the declare-and-pin device the two-phase model needs. An apply sees
/// exactly one shard, so the sender's apply cannot look at the recipient's
/// holding to check they hold the same token. Instead the *caller* declares the
/// descriptor and each side's apply asserts its own holding matches it; two
/// independent assertions against one declared value give transitively what the
/// single-function cross-check used to give directly. A caller who lies about
/// the descriptor fails both assertions rather than moving the wrong asset.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct TokenDescriptor {
    pub definition_id: AccountId,
    pub kind: TokenKind,
}

impl TokenDescriptor {
    /// The empty holding this descriptor describes, for crediting a shard that
    /// holds nothing yet.
    #[must_use]
    pub const fn zeroized(&self) -> TokenHolding {
        match self.kind {
            TokenKind::Fungible => TokenHolding::Fungible {
                definition_id: self.definition_id,
                balance: 0,
            },
            TokenKind::NftMaster => TokenHolding::NftMaster {
                definition_id: self.definition_id,
                print_balance: 0,
            },
            TokenKind::NftPrintedCopy => TokenHolding::NftPrintedCopy {
                definition_id: self.definition_id,
                owned: false,
            },
        }
    }
}

impl TokenHolding {
    /// Which shape this holding is.
    #[must_use]
    pub const fn kind(&self) -> TokenKind {
        match self {
            Self::Fungible { .. } => TokenKind::Fungible,
            Self::NftMaster { .. } => TokenKind::NftMaster,
            Self::NftPrintedCopy { .. } => TokenKind::NftPrintedCopy,
        }
    }

    /// Assert this holding is the asset `descriptor` names.
    ///
    /// # Panics
    /// If the definition id or the kind differs.
    pub fn assert_matches(&self, descriptor: &TokenDescriptor) {
        assert_eq!(
            self.definition_id(),
            descriptor.definition_id,
            "Mismatched Token Definition and Token Holding"
        );
        assert_eq!(
            self.kind(),
            descriptor.kind,
            "Mismatched Token Definition and Token Holding types"
        );
    }
}

impl TokenHolding {
    pub fn zeroized_clone_from(other: &Self) -> Self {
        match other {
            TokenHolding::Fungible { definition_id, .. } => TokenHolding::Fungible {
                definition_id: *definition_id,
                balance: 0,
            },
            TokenHolding::NftMaster { definition_id, .. } => TokenHolding::NftMaster {
                definition_id: *definition_id,
                print_balance: 0,
            },
            TokenHolding::NftPrintedCopy { definition_id, .. } => TokenHolding::NftPrintedCopy {
                definition_id: *definition_id,
                owned: false,
            },
        }
    }

    pub fn zeroized_from_definition(
        definition_id: AccountId,
        definition: &TokenDefinition,
    ) -> Self {
        match definition {
            TokenDefinition::Fungible { .. } => TokenHolding::Fungible {
                definition_id,
                balance: 0,
            },
            TokenDefinition::NonFungible { .. } => TokenHolding::NftPrintedCopy {
                definition_id,
                owned: false,
            },
        }
    }

    pub fn definition_id(&self) -> AccountId {
        match self {
            TokenHolding::Fungible { definition_id, .. } => *definition_id,
            TokenHolding::NftMaster { definition_id, .. } => *definition_id,
            TokenHolding::NftPrintedCopy { definition_id, .. } => *definition_id,
        }
    }
}

impl TryFrom<&ShardData> for TokenHolding {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        TokenHolding::try_from_slice(data.as_ref())
    }
}

impl From<&TokenHolding> for ShardData {
    fn from(holding: &TokenHolding) -> Self {
        // Using size_of_val as size hint for Vec allocation
        let mut data = Vec::with_capacity(std::mem::size_of_val(holding));

        BorshSerialize::serialize(holding, &mut data)
            .expect("Serialization to Vec should not fail");

        ShardData::try_from(data).expect("Token holding encoded data should fit into ShardData")
    }
}

#[derive(Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct NewTokenMetadata {
    /// Metadata standard.
    pub standard: MetadataStandard,
    /// Pointer to off-chain metadata
    pub uri: String,
    /// Creators of the token.
    pub creators: String,
}

#[account_type]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct TokenMetadata {
    /// Token Definition account id.
    pub definition_id: AccountId,
    /// Metadata standard .
    pub standard: MetadataStandard,
    /// Pointer to off-chain metadata.
    pub uri: String,
    /// Creators of the token.
    pub creators: String,
    /// Block id of primary sale.
    pub primary_sale_date: u64,
}

/// Metadata standard defining the expected format of JSON located off-chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum MetadataStandard {
    Simple,
    Expanded,
}

impl TryFrom<&ShardData> for TokenMetadata {
    type Error = std::io::Error;

    fn try_from(data: &ShardData) -> Result<Self, Self::Error> {
        TokenMetadata::try_from_slice(data.as_ref())
    }
}

impl From<&TokenMetadata> for ShardData {
    fn from(metadata: &TokenMetadata) -> Self {
        // Using size_of_val as size hint for Vec allocation
        let mut data = Vec::with_capacity(std::mem::size_of_val(metadata));

        BorshSerialize::serialize(metadata, &mut data)
            .expect("Serialization to Vec should not fail");

        ShardData::try_from(data).expect("Token metadata encoded data should fit into ShardData")
    }
}
