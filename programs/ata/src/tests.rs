#![cfg(test)]

//! Unit tests for the ATA program's two phases.
//!
//! This program owns no shard, so it has no state transitions — every apply half
//! is a guard that reads a Token Program shard and returns `None`. What is
//! covered here is address derivation, the `AtaContents` classification the
//! `Create` branch now hangs on, and each guard.
//!
//! Six v0.2.5 tests have no successor, all of the same kind: they asserted
//! `<account>.program_owner == token_program_id` for the definition, the existing
//! ATA, the sender ATA and the recipient. There is no program-level check left to
//! test. A shard is keyed by the program that wrote it, and the transaction's
//! `ProgramShardSelector`s name which program's shard each account slot refers
//! to; `guard(account, token_program_id, …)` asserts that naming. The assertion
//! lives in SPEL now and is tested there.

use ata_core::AtaContents;
use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{TokenDefinition, TokenDescriptor, TokenHolding, TokenKind};

use crate::{burn, create, transfer};

const ATA_PROGRAM_ID: AccountId = AccountId::new([1u8; 32]);
const TOKEN_PROGRAM_ID: AccountId = AccountId::new([2u8; 32]);
const OWNER_ID: AccountId = AccountId::new([3u8; 32]);
const DEFINITION_ID: AccountId = AccountId::new([4u8; 32]);
const OTHER_DEFINITION_ID: AccountId = AccountId::new([5u8; 32]);
const WRONG_ATA_ID: AccountId = AccountId::new([7u8; 32]);
const METADATA_ID: AccountId = AccountId::new([8u8; 32]);

const AMOUNT: u128 = 1_000;

const FUNGIBLE: TokenDescriptor = TokenDescriptor {
    definition_id: DEFINITION_ID,
    kind: TokenKind::Fungible,
};
const OTHER_FUNGIBLE: TokenDescriptor = TokenDescriptor {
    definition_id: OTHER_DEFINITION_ID,
    kind: TokenKind::Fungible,
};

/// An account slot as the plan phase sees it, naming the token program's shard.
fn token_shard(account_id: AccountId, is_authorized: bool) -> AccountMeta {
    AccountMeta::new(account_id, is_authorized, TOKEN_PROGRAM_ID)
}

/// The ATA address for `(token program, owner, definition)`.
fn ata_id(definition_id: AccountId) -> AccountId {
    let seed = ata_core::compute_ata_seed(TOKEN_PROGRAM_ID, OWNER_ID, definition_id);
    ata_core::get_associated_token_account_id(&ATA_PROGRAM_ID, &seed)
}

fn holding(definition_id: AccountId, balance: u128) -> ShardData {
    ShardData::from(&TokenHolding::Fungible {
        definition_id,
        balance,
    })
}

fn master_holding() -> ShardData {
    ShardData::from(&TokenHolding::NftMaster {
        definition_id: DEFINITION_ID,
        print_balance: 10,
    })
}

fn fungible_definition() -> ShardData {
    ShardData::from(&TokenDefinition::Fungible {
        name: "Token".to_owned(),
        total_supply: 100_000,
        metadata_id: None,
        authority: None,
    })
}

fn non_fungible_definition() -> ShardData {
    ShardData::from(&TokenDefinition::NonFungible {
        name: "NFT".to_owned(),
        printable_supply: 10,
        metadata_id: METADATA_ID,
    })
}

fn empty() -> ShardData {
    ShardData::default()
}

// ── address derivation ──────────────────────────────────────────────────────

#[test]
fn create_accepts_the_derived_ata_address() {
    let (_, needs_initialize) = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Empty,
    );
    assert!(needs_initialize, "an empty ATA must be initialized");
}

#[test]
#[should_panic(expected = "ATA account ID does not match expected derivation")]
fn create_rejects_an_address_that_is_not_the_derived_ata() {
    let _ = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(WRONG_ATA_ID, false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Empty,
    );
}

#[test]
#[should_panic(expected = "ATA account ID does not match expected derivation")]
fn create_rejects_an_ata_derived_from_another_definition() {
    let _ = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(OTHER_DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Empty,
    );
}

// ── Create: the contents branch ─────────────────────────────────────────────

/// `Create` is idempotent: an ATA that already holds the intended token needs no
/// chained initialize.
#[test]
fn create_is_a_no_op_for_an_intended_ata() {
    let (_, needs_initialize) = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Intended,
    );
    assert!(!needs_initialize, "an intended ATA is already there");
}

/// Replacing a squatted ATA destroys whatever was there, so it is the one branch
/// that needs the owner to say so.
#[test]
fn create_replaces_a_squatted_ata_when_the_owner_authorizes() {
    let (_, needs_initialize) = create::plan_create(
        &token_shard(OWNER_ID, true),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Squatted,
    );
    assert!(needs_initialize);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn create_refuses_to_replace_a_squatted_ata_without_the_owner() {
    let _ = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Squatted,
    );
}

/// An empty ATA needs no authorization: writing an empty shard denies that
/// address to nobody.
#[test]
fn create_initializes_an_empty_ata_without_owner_authorization() {
    let (_, needs_initialize) = create::plan_create(
        &token_shard(OWNER_ID, false),
        &token_shard(DEFINITION_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        AtaContents::Empty,
    );
    assert!(needs_initialize);
}

// ── classify / check_contents ───────────────────────────────────────────────

#[test]
fn classify_reads_an_empty_shard_as_empty() {
    assert_eq!(ata_core::classify(&empty(), &FUNGIBLE), AtaContents::Empty);
}

#[test]
fn classify_reads_the_intended_holding_as_intended() {
    assert_eq!(
        ata_core::classify(&holding(DEFINITION_ID, AMOUNT), &FUNGIBLE),
        AtaContents::Intended
    );
}

#[test]
fn classify_reads_another_definitions_holding_as_squatted() {
    assert_eq!(
        ata_core::classify(&holding(OTHER_DEFINITION_ID, AMOUNT), &FUNGIBLE),
        AtaContents::Squatted
    );
}

#[test]
fn classify_reads_undecodable_bytes_as_squatted() {
    assert_eq!(
        ata_core::classify(&fungible_definition(), &FUNGIBLE),
        AtaContents::Squatted
    );
}

/// A non-fungible definition classifies both the master and its printed copies,
/// so a master holding is `Intended` for a printed-copy descriptor.
#[test]
fn classify_accepts_either_nft_shape_for_a_non_fungible_definition() {
    let printed = TokenDescriptor {
        definition_id: DEFINITION_ID,
        kind: TokenKind::NftPrintedCopy,
    };
    assert_eq!(
        ata_core::classify(&master_holding(), &printed),
        AtaContents::Intended
    );
}

#[test]
fn check_contents_accepts_a_truthful_claim() {
    create::check_contents(&empty(), &FUNGIBLE, AtaContents::Empty);
}

/// The hole this closes: `Empty` claimed over a funded ATA would hand the token
/// program a seed-authorized `InitializeAccount` and zeroize the holding, with no
/// owner signature anywhere.
#[test]
#[should_panic(expected = "Associated token account does not hold what the instruction claims")]
fn check_contents_rejects_empty_claimed_over_a_funded_ata() {
    create::check_contents(
        &holding(DEFINITION_ID, AMOUNT),
        &FUNGIBLE,
        AtaContents::Empty,
    );
}

#[test]
#[should_panic(expected = "Associated token account does not hold what the instruction claims")]
fn check_contents_rejects_intended_claimed_over_a_squatted_ata() {
    create::check_contents(
        &holding(OTHER_DEFINITION_ID, AMOUNT),
        &FUNGIBLE,
        AtaContents::Intended,
    );
}

// ── check_definition_kind ───────────────────────────────────────────────────

#[test]
fn check_definition_kind_accepts_the_definitions_own_kind() {
    create::check_definition_kind(&fungible_definition(), TokenKind::Fungible);
}

#[test]
#[should_panic(expected = "Token Definition does not initialize this Token Holding kind")]
fn check_definition_kind_rejects_a_mismatched_kind() {
    create::check_definition_kind(&fungible_definition(), TokenKind::NftMaster);
}

#[test]
fn check_definition_kind_classifies_a_non_fungible_as_a_printed_copy() {
    create::check_definition_kind(&non_fungible_definition(), TokenKind::NftPrintedCopy);
}

#[test]
#[should_panic(expected = "Token Definition account must be valid")]
fn check_definition_kind_rejects_a_shard_that_is_not_a_definition() {
    create::check_definition_kind(&holding(DEFINITION_ID, AMOUNT), TokenKind::Fungible);
}

// ── Transfer ────────────────────────────────────────────────────────────────

#[test]
fn transfer_accepts_an_authorized_owner_and_derived_ata() {
    let _ = transfer::plan_transfer(
        &token_shard(OWNER_ID, true),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &FUNGIBLE,
    );
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn transfer_without_owner_authorization_should_fail() {
    let _ = transfer::plan_transfer(
        &token_shard(OWNER_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &FUNGIBLE,
    );
}

/// The declared descriptor is what the ATA address derives from, so a lie about
/// it cannot reach the chained call — the derivation stops it here.
#[test]
#[should_panic(expected = "ATA account ID does not match expected derivation")]
fn transfer_with_a_descriptor_for_another_definition_should_fail() {
    let _ = transfer::plan_transfer(
        &token_shard(OWNER_ID, true),
        &token_shard(ata_id(DEFINITION_ID), false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &OTHER_FUNGIBLE,
    );
}

#[test]
fn check_recipient_accepts_an_initialized_matching_holding() {
    transfer::check_recipient(&holding(DEFINITION_ID, AMOUNT), &FUNGIBLE);
}

/// ATA's own contract, deliberately stricter than token's: token's `deposit`
/// would materialize an empty recipient from the descriptor.
#[test]
#[should_panic(expected = "Recipient token holding must be initialized")]
fn check_recipient_rejects_an_empty_recipient() {
    transfer::check_recipient(&empty(), &FUNGIBLE);
}

#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding")]
fn check_recipient_rejects_a_holding_of_another_definition() {
    transfer::check_recipient(&holding(OTHER_DEFINITION_ID, AMOUNT), &FUNGIBLE);
}

#[test]
#[should_panic(expected = "Recipient must hold a valid token")]
fn check_recipient_rejects_a_shard_that_is_not_a_holding() {
    transfer::check_recipient(&fungible_definition(), &FUNGIBLE);
}

// ── Burn ────────────────────────────────────────────────────────────────────

#[test]
fn burn_accepts_an_authorized_owner_and_matching_definition() {
    let _ = burn::plan_burn(
        &token_shard(OWNER_ID, true),
        &token_shard(ata_id(DEFINITION_ID), false),
        &token_shard(DEFINITION_ID, false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &FUNGIBLE,
    );
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn burn_without_owner_authorization_should_fail() {
    let _ = burn::plan_burn(
        &token_shard(OWNER_ID, false),
        &token_shard(ata_id(DEFINITION_ID), false),
        &token_shard(DEFINITION_ID, false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &FUNGIBLE,
    );
}

#[test]
#[should_panic(expected = "Holder ATA token definition does not match")]
fn burn_with_a_descriptor_naming_another_definition_should_fail() {
    let _ = burn::plan_burn(
        &token_shard(OWNER_ID, true),
        &token_shard(ata_id(OTHER_DEFINITION_ID), false),
        &token_shard(DEFINITION_ID, false),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        &OTHER_FUNGIBLE,
    );
}
