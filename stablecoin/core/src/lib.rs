//! Core data structures and utilities for the Stablecoin Program.

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    account::{AccountId, AccountWithMetadata, Data},
    program::{PdaSeed, ProgramId},
};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

const POSITION_PDA_DOMAIN: [u8; 32] = [0; 32];
const POSITION_VAULT_PDA_DOMAIN: [u8; 32] = [1; 32];

/// Stablecoin Program Instruction.
#[derive(Debug, Serialize, Deserialize)]
pub enum Instruction {
    /// Open a new collateral-only [`Position`] for the calling owner.
    ///
    /// Required accounts (5):
    /// - Owner account (authorized)
    /// - Position account (uninitialized, address must match
    ///   `compute_position_pda(self_program_id, owner, token_definition)`)
    /// - Position vault token holding account (uninitialized, address must match
    ///   `compute_position_vault_pda(self_program_id, position_id)`)
    /// - Owner's source token holding for the collateral (authorized, initialized)
    /// - Token definition account for the collateral (matches the user holding's `definition_id`;
    ///   its `program_owner` determines the Token Program used by the chained `InitializeAccount`
    ///   / `Transfer` calls)
    OpenPosition {
        /// Amount of collateral tokens to deposit into the position vault.
        collateral_amount: u128,
    },
}

/// Persistent state held by a Stablecoin [`Position`] account.
///
/// `debt_amount` is included for forward compatibility with `generate_debt`; until that
/// instruction lands `open_position` always initializes it to `0`.
#[account_type]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct Position {
    /// Token holding account (vault PDA) that custodies the collateral backing this position.
    pub collateral_vault_id: AccountId,
    /// Token definition for the collateral held in `collateral_vault_id`.
    pub collateral_definition_id: AccountId,
    /// Amount of collateral tokens deposited.
    pub collateral_amount: u128,
    /// Outstanding stablecoin debt against this position.
    pub debt_amount: u128,
}

impl TryFrom<&Data> for Position {
    type Error = std::io::Error;

    fn try_from(data: &Data) -> Result<Self, Self::Error> {
        Self::try_from_slice(data.as_ref())
    }
}

impl From<&Position> for Data {
    fn from(position: &Position) -> Self {
        let mut data = Vec::with_capacity(std::mem::size_of_val(position));
        BorshSerialize::serialize(position, &mut data)
            .expect("Serialization to Vec should not fail");
        Self::try_from(data).expect("Position encoded data should fit into Data")
    }
}

/// PDA seed for the [`Position`] account owned by `owner_id` for `collateral_definition_id`.
///
/// Derived from the owner and collateral definition addresses with a domain-separation tag
/// so one owner can hold separate positions for separate collateral definitions.
pub fn compute_position_pda_seed(
    owner_id: AccountId,
    collateral_definition_id: AccountId,
) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = [0u8; 96];
    bytes[0..32].copy_from_slice(&owner_id.to_bytes());
    bytes[32..64].copy_from_slice(&collateral_definition_id.to_bytes());
    bytes[64..96].copy_from_slice(&POSITION_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the [`Position`] PDA owned by `owner_id` under `stablecoin_program_id`.
pub fn compute_position_pda(
    stablecoin_program_id: ProgramId,
    owner_id: AccountId,
    collateral_definition_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_pda_seed(owner_id, collateral_definition_id),
    )
}

/// PDA seed for the collateral vault token holding bound to a [`Position`].
///
/// Derived from the position's address with a distinct domain-separation tag so the vault
/// id cannot collide with the position id even though both PDAs share the same program.
pub fn compute_position_vault_pda_seed(position_id: AccountId) -> PdaSeed {
    use risc0_zkvm::sha::{Impl, Sha256 as _};

    let mut bytes = [0u8; 64];
    bytes[0..32].copy_from_slice(&position_id.to_bytes());
    bytes[32..64].copy_from_slice(&POSITION_VAULT_PDA_DOMAIN);

    let mut out = [0u8; 32];
    out.copy_from_slice(Impl::hash_bytes(&bytes).as_bytes());
    PdaSeed::new(out)
}

/// Account id of the collateral vault PDA for `position_id` under `stablecoin_program_id`.
pub fn compute_position_vault_pda(
    stablecoin_program_id: ProgramId,
    position_id: AccountId,
) -> AccountId {
    AccountId::for_public_pda(
        &stablecoin_program_id,
        &compute_position_vault_pda_seed(position_id),
    )
}

/// Verify the position account's address matches
/// `(stablecoin_program_id, owner, collateral_definition_id)` and return the [`PdaSeed`] for
/// use in post-state claims.
///
/// # Panics
/// If `position.account_id` does not match the address derived from `owner`,
/// `collateral_definition_id`, and `stablecoin_program_id`.
pub fn verify_position_and_get_seed(
    position: &AccountWithMetadata,
    owner: &AccountWithMetadata,
    collateral_definition_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_pda_seed(owner.account_id, collateral_definition_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        position.account_id, expected_id,
        "Position account ID does not match expected derivation"
    );
    seed
}

/// Verify the vault account's address matches `(stablecoin_program_id, position)` and
/// return the [`PdaSeed`] for use in chained calls.
///
/// # Panics
/// If `vault.account_id` does not match the address derived from `position_id` and
/// `stablecoin_program_id`.
pub fn verify_position_vault_and_get_seed(
    vault: &AccountWithMetadata,
    position_id: AccountId,
    stablecoin_program_id: ProgramId,
) -> PdaSeed {
    let seed = compute_position_vault_pda_seed(position_id);
    let expected_id = AccountId::for_public_pda(&stablecoin_program_id, &seed);
    assert_eq!(
        vault.account_id, expected_id,
        "Position vault account ID does not match expected derivation"
    );
    seed
}
