use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;

/// Public ATA messages. Creation may run on any admitted ATA actor; spending runs on
/// the owner's authorized ATA actor. Holdings are public PDAs, even for private owners.
///
/// Transfers require the recipient's ATA and asset actors, plus the source asset actor.
/// Private recipients must execute the asset credit through runtime promotion in the
/// same transaction. A deferred credit fails verification and rolls everything back.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Message {
    /// Permissionlessly initialize the derived holding. Declare its token actor and
    /// the definition's token actor. Repeated creation preserves an existing balance.
    Create {
        token_program_id: AccountId,
        owner: AccountId,
        definition: AccountId,
    },
    /// Transfer from the receiver owner's derived holding. Zero amounts validate both
    /// holdings and complete without an asset transfer or event.
    Transfer {
        token_program_id: AccountId,
        definition: AccountId,
        to: AccountId,
        amount: u128,
    },
    /// Burn from the receiver owner's holding. Declare holding and definition token actors.
    Burn {
        token_program_id: AccountId,
        definition: AccountId,
        amount: u128,
    },
    /// Permissionlessly initialize an NFT holding, declaring holding and definition NFT actors.
    CreateNft {
        nft_program_id: AccountId,
        owner: AccountId,
        definition: AccountId,
    },
    /// Move the whole master or copy to an initialized holding of the same definition
    /// that holds nothing. NFT printing is not exposed by ATA.
    TransferNft {
        nft_program_id: AccountId,
        definition: AccountId,
        to: AccountId,
    },
    /// Burn master units or exactly one owned copy. Declare holding and definition NFT actors.
    BurnNft {
        nft_program_id: AccountId,
        definition: AccountId,
        amount: u128,
    },
}

/// Emitted only by this deployment: owner authorization is checked before forwarding.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Internal {
    Transfer(TransferRequest),
    /// Self-only assertion, scheduled after all descendants of the first state read.
    AssertIdle,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TransferRequest {
    /// Captured from the authorized receiver, never from a public message argument.
    pub owner: AccountId,
    pub asset_program: AccountId,
    pub definition: AccountId,
    pub kind: TransferKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TransferKind {
    Token { amount: u128 },
    Nft,
}
