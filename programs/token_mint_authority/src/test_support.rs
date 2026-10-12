//! Shared fixtures for the `faucet_mint` unit tests.
//!
//! The plan phase sees only account metadata, so a fixture is now an id, an
//! authorization flag and the program whose shard the slot names. Shard
//! *contents* are separate `ShardData` builders, since each apply half sees
//! exactly one of them.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{TokenDefinition, TokenHolding};
use token_mint_authority_core::{
    compute_mint_allowance_pda, compute_mint_authority_pda, MintAllowance,
};

pub(crate) const TOKEN_MINT_AUTHORITY_PROGRAM_ID: AccountId = AccountId::new([11u8; 32]);
pub(crate) const TOKEN_PROGRAM_ID: AccountId = AccountId::new([2u8; 32]);
/// The clock program's own account id: the clock's timestamp lives in
/// `shards[clock_account_id()]`, and `plan_faucet_mint` asserts the slot selects
/// exactly that shard.
pub(crate) fn clock_program_id() -> AccountId {
    clock_core::clock_account_id()
}

/// The clock timestamp used as "now" in every test (Unix milliseconds).
pub(crate) const NOW: u64 = 1_700_000_000_000;

pub(crate) fn recipient_id() -> AccountId {
    AccountId::new([0xCA; 32])
}
pub(crate) fn user_holding_id() -> AccountId {
    AccountId::new([0xCB; 32])
}
pub(crate) fn definition_id() -> AccountId {
    AccountId::new([0x40; 32])
}
pub(crate) fn mint_authority_id() -> AccountId {
    compute_mint_authority_pda(TOKEN_MINT_AUTHORITY_PROGRAM_ID)
}
pub(crate) fn mint_allowance_id() -> AccountId {
    compute_mint_allowance_pda(
        TOKEN_MINT_AUTHORITY_PROGRAM_ID,
        recipient_id(),
        definition_id(),
    )
}

// ── account slots (what the plan phase sees) ────────────────────────────────

pub(crate) fn recipient_account(is_authorized: bool) -> AccountMeta {
    AccountMeta::new(recipient_id(), is_authorized, AccountId::new([0u8; 32]))
}

/// The allowance is this program's own shard — the only one it writes.
pub(crate) fn allowance_slot() -> AccountMeta {
    AccountMeta::new(mint_allowance_id(), false, TOKEN_MINT_AUTHORITY_PROGRAM_ID)
}

pub(crate) fn allowance_slot_at(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, TOKEN_MINT_AUTHORITY_PROGRAM_ID)
}

pub(crate) fn user_holding_slot() -> AccountMeta {
    AccountMeta::new(user_holding_id(), false, TOKEN_PROGRAM_ID)
}

pub(crate) fn definition_slot() -> AccountMeta {
    AccountMeta::new(definition_id(), false, TOKEN_PROGRAM_ID)
}

pub(crate) fn authority_slot() -> AccountMeta {
    AccountMeta::new(mint_authority_id(), false, TOKEN_PROGRAM_ID)
}

pub(crate) fn authority_slot_at(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, TOKEN_PROGRAM_ID)
}

pub(crate) fn clock_slot() -> AccountMeta {
    AccountMeta::new(CLOCK_01_PROGRAM_ACCOUNT_ID, false, clock_program_id())
}

pub(crate) fn clock_slot_at(account_id: AccountId) -> AccountMeta {
    AccountMeta::new(account_id, false, clock_program_id())
}

// ── shard contents (what an apply half sees) ────────────────────────────────

pub(crate) fn empty() -> ShardData {
    ShardData::default()
}

pub(crate) fn clock_data(timestamp: u64) -> ShardData {
    ShardData::try_from(
        ClockAccountData {
            block_id: 1,
            timestamp,
        }
        .to_bytes(),
    )
    .expect("a clock record fits in a shard")
}

pub(crate) fn allowance_data(last_mint_ms: u64) -> ShardData {
    ShardData::from(&MintAllowance {
        recipient_id: recipient_id(),
        definition_id: definition_id(),
        last_mint_ms,
    })
}

pub(crate) fn faucet_definition(authority: Option<AccountId>) -> ShardData {
    ShardData::from(&TokenDefinition::Fungible {
        name: "Faucet".to_owned(),
        total_supply: 1_000_000,
        metadata_id: None,
        authority,
    })
}

pub(crate) fn non_fungible_definition() -> ShardData {
    ShardData::from(&TokenDefinition::NonFungible {
        name: "NFT".to_owned(),
        printable_supply: 1,
        metadata_id: definition_id(),
    })
}

pub(crate) fn holding_data() -> ShardData {
    ShardData::from(&TokenHolding::Fungible {
        definition_id: definition_id(),
        balance: 0,
    })
}
