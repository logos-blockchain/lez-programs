#![cfg(test)]

//! Unit tests for the token program's two phases.
//!
//! The plan halves here are the small assertion helpers; effect *emission* lives
//! in the guest, where the macro-generated `Effect` enum exists, and is covered
//! end-to-end by `programs/integration_tests`. What this file covers is every
//! apply half — the state transitions — plus the plan asserts, and the
//! composition of the two applies an instruction emits.
//!
//! Three v0.2.5 tests have no successor here, and deliberately so:
//!
//! * `test_{token_initialize_account,mint,set_authority}_rejects_foreign_owned_definition` asserted
//!   `program_owner == token_program_id`. There is no program-level check to test any more: a shard
//!   is keyed by the program that wrote it, and declaring an effect on one asserts it is ours. The
//!   assertion moved into SPEL (`spel-framework-core`'s `effect`/`guard` builders) and is tested
//!   there.
//! * `test_print_nft_print_account_must_be_authorized` asserted the *printed copy* authorized its
//!   own creation. Writing an empty shard denies that address to nobody under v0.3.0, so requiring
//!   authorization bought nothing and is gone. The copy is still guarded: `create` refuses a
//!   non-empty shard.

use std::collections::HashMap;

use lee_core::{
    account::{AccountId, ShardData},
    program::AccountMeta,
};
use token_core::{
    MetadataStandard, NewTokenDefinition, NewTokenMetadata, TokenDefinition, TokenDescriptor,
    TokenHolding, TokenKind, TokenMetadata,
};

use crate::{burn, initialize, mint, new_definition, print_nft, set_authority, transfer};

const TOKEN_PROGRAM_ID: AccountId = AccountId::new([5u8; 32]);
const DEFINITION_ID: AccountId = AccountId::new([15u8; 32]);
const OTHER_DEFINITION_ID: AccountId = AccountId::new([16u8; 32]);
const HOLDING_ID: AccountId = AccountId::new([17u8; 32]);
const HOLDING_ID_2: AccountId = AccountId::new([42u8; 32]);
const METADATA_ID: AccountId = AccountId::new([43u8; 32]);
const AUTHORITY_ID: AccountId = AccountId::new([44u8; 32]);

const INIT_SUPPLY: u128 = 100_000;
const HOLDING_BALANCE: u128 = 1_000;
const TRANSFER_AMOUNT: u128 = 5_000;
const PRINTABLE_COPIES: u128 = 10;

const FUNGIBLE: TokenDescriptor = TokenDescriptor {
    definition_id: DEFINITION_ID,
    kind: TokenKind::Fungible,
};
const MASTER: TokenDescriptor = TokenDescriptor {
    definition_id: DEFINITION_ID,
    kind: TokenKind::NftMaster,
};
const PRINTED: TokenDescriptor = TokenDescriptor {
    definition_id: DEFINITION_ID,
    kind: TokenKind::NftPrintedCopy,
};
const OTHER_FUNGIBLE: TokenDescriptor = TokenDescriptor {
    definition_id: OTHER_DEFINITION_ID,
    kind: TokenKind::Fungible,
};

// ── fixtures ────────────────────────────────────────────────────────────────

const fn fungible(balance: u128) -> TokenHolding {
    TokenHolding::Fungible {
        definition_id: DEFINITION_ID,
        balance,
    }
}

const fn master(print_balance: u128) -> TokenHolding {
    TokenHolding::NftMaster {
        definition_id: DEFINITION_ID,
        print_balance,
    }
}

const fn printed(owned: bool) -> TokenHolding {
    TokenHolding::NftPrintedCopy {
        definition_id: DEFINITION_ID,
        owned,
    }
}

fn fungible_definition(total_supply: u128, authority: Option<AccountId>) -> TokenDefinition {
    TokenDefinition::Fungible {
        name: "Token".to_owned(),
        total_supply,
        metadata_id: None,
        authority,
    }
}

fn non_fungible_definition(printable_supply: u128) -> TokenDefinition {
    TokenDefinition::NonFungible {
        name: "NFT".to_owned(),
        printable_supply,
        metadata_id: METADATA_ID,
    }
}

fn new_metadata() -> NewTokenMetadata {
    NewTokenMetadata {
        standard: MetadataStandard::Simple,
        uri: "ipfs://metadata".to_owned(),
        creators: "creator".to_owned(),
    }
}

/// An account slot as the plan phase sees it: an id, an authorization flag, and
/// whose shard is named. No data — the plan has none.
fn handle(account_id: AccountId, is_authorized: bool) -> AccountMeta {
    AccountMeta::new(account_id, is_authorized, TOKEN_PROGRAM_ID)
}

fn holding_data(holding: &TokenHolding) -> ShardData {
    ShardData::from(holding)
}

fn definition_data(definition: &TokenDefinition) -> ShardData {
    ShardData::from(definition)
}

fn empty() -> ShardData {
    ShardData::default()
}

fn as_holding(data: &ShardData) -> TokenHolding {
    TokenHolding::try_from(data).expect("apply wrote a holding")
}

fn as_definition(data: &ShardData) -> TokenDefinition {
    TokenDefinition::try_from(data).expect("apply wrote a definition")
}

/// Run a transfer's two applies the way the runtime would: in emission order,
/// each against its own shard, accumulating into one state map.
///
/// This is what proves the two independent `assert_matches` calls add up to the
/// cross-check the single pre-v0.3.0 function did directly.
fn settle_transfer(
    sender: (AccountId, ShardData),
    recipient: (AccountId, ShardData),
    descriptor: TokenDescriptor,
    amount: u128,
) -> HashMap<AccountId, ShardData> {
    let mut state: HashMap<AccountId, ShardData> = HashMap::new();
    state.insert(sender.0, transfer::withdraw(&sender.1, &descriptor, amount));
    state.insert(
        recipient.0,
        transfer::deposit(&recipient.1, &descriptor, amount),
    );
    state
}

fn settled(state: &HashMap<AccountId, ShardData>, account_id: AccountId) -> TokenHolding {
    as_holding(state.get(&account_id).expect("the account was settled"))
}

// ── new definitions ─────────────────────────────────────────────────────────

#[test]
fn new_definition_with_valid_inputs_succeeds() {
    let (definition, holding) = new_definition::fungible_records(
        DEFINITION_ID,
        "Token".to_owned(),
        INIT_SUPPLY,
        Some(AUTHORITY_ID),
    );

    assert_eq!(
        as_definition(&new_definition::create(&empty(), definition)),
        fungible_definition(INIT_SUPPLY, Some(AUTHORITY_ID))
    );
    assert_eq!(
        as_holding(&new_definition::create(&empty(), holding)),
        fungible(INIT_SUPPLY)
    );
}

#[test]
#[should_panic(expected = "Target account must not already hold data")]
fn new_definition_refuses_a_non_empty_definition_target() {
    let (definition, _) =
        new_definition::fungible_records(DEFINITION_ID, "Token".to_owned(), INIT_SUPPLY, None);
    // Formerly "Definition target account must have default values", asserted in
    // the plan phase against a whole pre-state account. Emptiness is only visible
    // to the apply half now.
    let _ = new_definition::create(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        definition,
    );
}

#[test]
#[should_panic(expected = "Target account must not already hold data")]
fn new_definition_refuses_a_non_empty_holding_target() {
    let (_, holding) =
        new_definition::fungible_records(DEFINITION_ID, "Token".to_owned(), INIT_SUPPLY, None);
    let _ = new_definition::create(&holding_data(&fungible(HOLDING_BALANCE)), holding);
}

#[test]
#[should_panic(expected = "Target account must be authorized to be created")]
fn new_definition_requires_authorized_definition_target() {
    new_definition::authorize_targets(&[&handle(DEFINITION_ID, false), &handle(HOLDING_ID, true)]);
}

#[test]
#[should_panic(expected = "Target account must be authorized to be created")]
fn new_definition_requires_authorized_holding_target() {
    new_definition::authorize_targets(&[&handle(DEFINITION_ID, true), &handle(HOLDING_ID, false)]);
}

#[test]
#[should_panic(expected = "Mint authority must be a valid non-zero account ID")]
fn new_definition_rejects_an_all_zero_mint_authority() {
    let _ = new_definition::fungible_records(
        DEFINITION_ID,
        "Token".to_owned(),
        INIT_SUPPLY,
        Some(AccountId::new([0u8; 32])),
    );
}

#[test]
fn new_definition_with_metadata_success() {
    let (definition, holding, metadata) = new_definition::records_with_metadata(
        DEFINITION_ID,
        METADATA_ID,
        NewTokenDefinition::Fungible {
            name: "Token".to_owned(),
            total_supply: INIT_SUPPLY,
            mint_authority: None,
        },
        new_metadata(),
    );

    assert_eq!(
        as_definition(&definition),
        TokenDefinition::Fungible {
            name: "Token".to_owned(),
            total_supply: INIT_SUPPLY,
            metadata_id: Some(METADATA_ID),
            authority: None,
        }
    );
    assert_eq!(as_holding(&holding), fungible(INIT_SUPPLY));
    assert_eq!(
        TokenMetadata::try_from(&metadata).expect("metadata decodes"),
        TokenMetadata {
            definition_id: DEFINITION_ID,
            standard: MetadataStandard::Simple,
            uri: "ipfs://metadata".to_owned(),
            creators: "creator".to_owned(),
            primary_sale_date: 0,
        }
    );
}

#[test]
fn new_definition_with_metadata_creates_a_master_copy_for_a_non_fungible() {
    let (definition, holding, _) = new_definition::records_with_metadata(
        DEFINITION_ID,
        METADATA_ID,
        NewTokenDefinition::NonFungible {
            name: "NFT".to_owned(),
            printable_supply: PRINTABLE_COPIES,
        },
        new_metadata(),
    );

    assert_eq!(
        as_definition(&definition),
        non_fungible_definition(PRINTABLE_COPIES)
    );
    assert_eq!(as_holding(&holding), master(PRINTABLE_COPIES));
}

#[test]
fn metadata_fungible_with_authority_is_mintable() {
    let (definition, _, _) = new_definition::records_with_metadata(
        DEFINITION_ID,
        METADATA_ID,
        NewTokenDefinition::Fungible {
            name: "Token".to_owned(),
            total_supply: INIT_SUPPLY,
            mint_authority: Some(AUTHORITY_ID),
        },
        new_metadata(),
    );

    let minted = mint::mint_supply(&definition, 1, AUTHORITY_ID, true);
    let TokenDefinition::Fungible { total_supply, .. } = as_definition(&minted) else {
        panic!("expected a fungible definition");
    };
    assert_eq!(total_supply, INIT_SUPPLY + 1);
}

#[test]
#[should_panic(expected = "Target account must be authorized to be created")]
fn new_definition_with_metadata_requires_every_target_authorized() {
    new_definition::authorize_targets(&[
        &handle(DEFINITION_ID, true),
        &handle(HOLDING_ID, true),
        &handle(METADATA_ID, false),
    ]);
}

// ── transfer ────────────────────────────────────────────────────────────────

#[test]
fn transfer_with_valid_inputs_succeeds() {
    let state = settle_transfer(
        (HOLDING_ID, holding_data(&fungible(INIT_SUPPLY))),
        (HOLDING_ID_2, holding_data(&fungible(INIT_SUPPLY))),
        FUNGIBLE,
        TRANSFER_AMOUNT,
    );

    assert_eq!(
        settled(&state, HOLDING_ID),
        fungible(INIT_SUPPLY - TRANSFER_AMOUNT)
    );
    assert_eq!(
        settled(&state, HOLDING_ID_2),
        fungible(INIT_SUPPLY + TRANSFER_AMOUNT)
    );
}

#[test]
#[should_panic(expected = "Sender authorization is missing")]
fn transfer_without_sender_authorization_should_fail() {
    transfer::authorize(&handle(HOLDING_ID, false));
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn transfer_with_insufficient_balance_should_fail() {
    let _ = transfer::withdraw(
        &holding_data(&fungible(HOLDING_BALANCE)),
        &FUNGIBLE,
        HOLDING_BALANCE + 1,
    );
}

/// Formerly `test_transfer_with_different_definition_ids_should_fail`, which
/// relied on one function holding both holdings. Each side now checks itself
/// against the declared descriptor, so a mismatch is caught on whichever side
/// disagrees — here the recipient.
#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding")]
fn transfer_into_a_holding_of_another_definition_should_fail() {
    let _ = transfer::deposit(
        &holding_data(&fungible(HOLDING_BALANCE)),
        &OTHER_FUNGIBLE,
        TRANSFER_AMOUNT,
    );
}

#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding types")]
fn transfer_with_a_descriptor_of_the_wrong_kind_should_fail() {
    let _ = transfer::withdraw(&holding_data(&fungible(HOLDING_BALANCE)), &MASTER, 1);
}

#[test]
fn transfer_into_an_empty_shard_initializes_it_from_the_descriptor() {
    let state = settle_transfer(
        (HOLDING_ID, holding_data(&fungible(INIT_SUPPLY))),
        (HOLDING_ID_2, empty()),
        FUNGIBLE,
        TRANSFER_AMOUNT,
    );

    assert_eq!(
        settled(&state, HOLDING_ID),
        fungible(INIT_SUPPLY - TRANSFER_AMOUNT)
    );
    assert_eq!(settled(&state, HOLDING_ID_2), fungible(TRANSFER_AMOUNT));
}

#[test]
fn transfer_with_master_nft_success() {
    let state = settle_transfer(
        (HOLDING_ID, holding_data(&master(PRINTABLE_COPIES))),
        (HOLDING_ID_2, holding_data(&master(0))),
        MASTER,
        PRINTABLE_COPIES,
    );

    assert_eq!(settled(&state, HOLDING_ID), master(0));
    assert_eq!(settled(&state, HOLDING_ID_2), master(PRINTABLE_COPIES));
}

#[test]
#[should_panic(expected = "Invalid balance for NFT Master transfer")]
fn transfer_with_master_nft_invalid_balance() {
    let _ = transfer::withdraw(
        &holding_data(&master(PRINTABLE_COPIES)),
        &MASTER,
        PRINTABLE_COPIES - 1,
    );
}

#[test]
#[should_panic(expected = "Invalid balance in recipient account for NFT transfer")]
fn transfer_with_master_nft_invalid_recipient_balance() {
    let _ = transfer::deposit(
        &holding_data(&master(PRINTABLE_COPIES)),
        &MASTER,
        PRINTABLE_COPIES,
    );
}

#[test]
fn transfer_of_a_printed_copy_moves_ownership() {
    let state = settle_transfer(
        (HOLDING_ID, holding_data(&printed(true))),
        (HOLDING_ID_2, holding_data(&printed(false))),
        PRINTED,
        1,
    );

    assert_eq!(settled(&state, HOLDING_ID), printed(false));
    assert_eq!(settled(&state, HOLDING_ID_2), printed(true));
}

#[test]
#[should_panic(expected = "Sender does not own the NFT Printed Copy")]
fn transfer_of_an_unowned_printed_copy_should_fail() {
    let _ = transfer::withdraw(&holding_data(&printed(false)), &PRINTED, 1);
}

#[test]
#[should_panic(expected = "Recipient already owns the NFT Printed Copy")]
fn transfer_into_an_owned_printed_copy_should_fail() {
    let _ = transfer::deposit(&holding_data(&printed(true)), &PRINTED, 1);
}

// ── initialize_account ──────────────────────────────────────────────────────

#[test]
fn initialize_account_succeeds() {
    let definition = definition_data(&fungible_definition(INIT_SUPPLY, None));
    // The definition-side guard reads and asserts, and must not write.
    initialize::check_holding_kind(&definition, TokenKind::Fungible);
    assert_eq!(
        as_holding(&initialize::initialize_holding(&empty(), &FUNGIBLE, false)),
        fungible(0)
    );
}

#[test]
#[should_panic(expected = "Token Definition does not initialize this Token Holding kind")]
fn initialize_account_rejects_a_kind_the_definition_does_not_describe() {
    initialize::check_holding_kind(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        TokenKind::NftMaster,
    );
}

/// Replaces `test_token_initialize_account_requires_authorization`. An *empty*
/// shard needs no authorization — writing it denies nothing to anyone. What still
/// needs it is overwriting a shard that already holds something.
#[test]
#[should_panic(expected = "Only uninitialized or authorized accounts can be initialized")]
fn initialize_account_over_existing_data_requires_authorization() {
    let _ =
        initialize::initialize_holding(&holding_data(&fungible(HOLDING_BALANCE)), &FUNGIBLE, false);
}

#[test]
fn initialize_account_over_existing_data_is_allowed_when_authorized() {
    assert_eq!(
        as_holding(&initialize::initialize_holding(
            &holding_data(&fungible(HOLDING_BALANCE)),
            &FUNGIBLE,
            true
        )),
        fungible(0)
    );
}

// ── burn ────────────────────────────────────────────────────────────────────

#[test]
fn burn_success() {
    let definition = burn::burn_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        TokenKind::Fungible,
        500,
    );
    let holding = burn::burn_holding(&holding_data(&fungible(HOLDING_BALANCE)), &FUNGIBLE, 500);

    assert_eq!(
        as_definition(&definition),
        fungible_definition(INIT_SUPPLY - 500, None)
    );
    assert_eq!(as_holding(&holding), fungible(HOLDING_BALANCE - 500));
}

#[test]
#[should_panic(expected = "Authorization is missing")]
fn burn_missing_authorization() {
    burn::authorize(&handle(HOLDING_ID, false));
}

#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding")]
fn burn_mismatch_def() {
    let _ = burn::burn_holding(
        &holding_data(&fungible(HOLDING_BALANCE)),
        &OTHER_FUNGIBLE,
        1,
    );
}

#[test]
#[should_panic(expected = "Insufficient balance to burn")]
fn burn_insufficient_balance() {
    let _ = burn::burn_holding(
        &holding_data(&fungible(HOLDING_BALANCE)),
        &FUNGIBLE,
        HOLDING_BALANCE + 1,
    );
}

#[test]
#[should_panic(expected = "Total supply underflow")]
fn burn_total_supply_underflow() {
    let _ = burn::burn_supply(
        &definition_data(&fungible_definition(100, None)),
        TokenKind::Fungible,
        101,
    );
}

#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding types")]
fn burn_supply_rejects_a_kind_the_definition_does_not_describe() {
    let _ = burn::burn_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        TokenKind::NftMaster,
        1,
    );
}

#[test]
#[should_panic(expected = "Cannot burn unowned NFT Printed Copy")]
fn burn_of_an_unowned_printed_copy_should_fail() {
    let _ = burn::burn_holding(&holding_data(&printed(false)), &PRINTED, 1);
}

#[test]
fn burn_of_a_printed_copy_decrements_printable_supply() {
    let definition = burn::burn_supply(
        &definition_data(&non_fungible_definition(PRINTABLE_COPIES)),
        TokenKind::NftPrintedCopy,
        1,
    );
    assert_eq!(
        as_definition(&definition),
        non_fungible_definition(PRINTABLE_COPIES - 1)
    );
    assert_eq!(
        as_holding(&burn::burn_holding(
            &holding_data(&printed(true)),
            &PRINTED,
            1
        )),
        printed(false)
    );
}

// ── mint ────────────────────────────────────────────────────────────────────

#[test]
fn mint_success() {
    let definition = mint::mint_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        50_000,
        DEFINITION_ID,
        true,
    );
    let holding = transfer::deposit(&holding_data(&fungible(HOLDING_BALANCE)), &FUNGIBLE, 50_000);

    assert_eq!(
        as_definition(&definition),
        fungible_definition(INIT_SUPPLY + 50_000, Some(DEFINITION_ID))
    );
    assert_eq!(as_holding(&holding), fungible(HOLDING_BALANCE + 50_000));
}

#[test]
fn mint_into_an_empty_holding_succeeds() {
    assert_eq!(
        as_holding(&transfer::deposit(&empty(), &FUNGIBLE, 50_000)),
        fungible(50_000)
    );
}

#[test]
#[should_panic(expected = "Definition account must be valid")]
fn mint_not_valid_definition_account() {
    let _ = mint::mint_supply(&holding_data(&fungible(1)), 1, DEFINITION_ID, true);
}

#[test]
#[should_panic(expected = "Invalid recipient data")]
fn mint_not_valid_holding_account() {
    let _ = transfer::deposit(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        &FUNGIBLE,
        1,
    );
}

#[test]
#[should_panic(expected = "Mint authority must authorize the transaction")]
fn mint_missing_authorization() {
    let _ = mint::mint_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        1,
        DEFINITION_ID,
        false,
    );
}

#[test]
#[should_panic(expected = "Mint authority check failed: signer is not the current authority")]
fn mint_by_a_non_authority_should_fail() {
    let _ = mint::mint_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(AUTHORITY_ID))),
        1,
        HOLDING_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "Mismatched Token Definition and Token Holding")]
fn mint_mismatched_token_definition() {
    let _ = transfer::deposit(
        &holding_data(&fungible(HOLDING_BALANCE)),
        &OTHER_FUNGIBLE,
        1,
    );
}

#[test]
#[should_panic(expected = "Total supply overflow")]
fn mint_total_supply_overflow() {
    let _ = mint::mint_supply(
        &definition_data(&fungible_definition(u128::MAX, Some(DEFINITION_ID))),
        1,
        DEFINITION_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "Recipient balance overflow")]
fn mint_holding_account_overflow() {
    let _ = transfer::deposit(&holding_data(&fungible(u128::MAX)), &FUNGIBLE, 1);
}

#[test]
#[should_panic(expected = "Cannot mint additional supply for Non-Fungible Tokens")]
fn mint_cannot_mint_unmintable_tokens() {
    let _ = mint::mint_supply(
        &definition_data(&non_fungible_definition(PRINTABLE_COPIES)),
        1,
        DEFINITION_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "Mint authority check failed: authority revoked, supply is fixed")]
fn mint_on_a_renounced_definition_should_fail() {
    let _ = mint::mint_supply(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        1,
        DEFINITION_ID,
        true,
    );
}

// ── print_nft ───────────────────────────────────────────────────────────────

#[test]
fn print_nft_success() {
    let master_after =
        print_nft::print_copy(&holding_data(&master(PRINTABLE_COPIES)), DEFINITION_ID);
    let copy = new_definition::create(&empty(), holding_data(&printed(true)));

    assert_eq!(as_holding(&master_after), master(PRINTABLE_COPIES - 1));
    assert_eq!(as_holding(&copy), printed(true));
}

#[test]
#[should_panic(expected = "Master NFT Account must be authorized")]
fn print_nft_master_account_must_be_authorized() {
    print_nft::authorize(&handle(HOLDING_ID, false));
}

#[test]
#[should_panic(expected = "Target account must not already hold data")]
fn print_nft_print_account_initialized() {
    // Formerly "Printed Account must be uninitialized".
    let _ = new_definition::create(&holding_data(&printed(false)), holding_data(&printed(true)));
}

#[test]
#[should_panic(expected = "Invalid Token Holding data")]
fn print_nft_master_nft_invalid_token_holding() {
    let _ = print_nft::print_copy(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        DEFINITION_ID,
    );
}

#[test]
#[should_panic(expected = "Invalid Token Holding provided as NFT Master Account")]
fn print_nft_master_nft_not_nft_master_account() {
    let _ = print_nft::print_copy(&holding_data(&fungible(HOLDING_BALANCE)), DEFINITION_ID);
}

#[test]
#[should_panic(expected = "Insufficient balance to print another NFT copy")]
fn print_nft_master_nft_insufficient_balance() {
    let _ = print_nft::print_copy(&holding_data(&master(1)), DEFINITION_ID);
}

/// The printed copy's `definition_id` is a declared value; the master's apply is
/// what makes it true.
#[test]
#[should_panic(expected = "Printed copy does not belong to the master's Token Definition")]
fn print_nft_rejects_a_foreign_definition_id() {
    let _ = print_nft::print_copy(
        &holding_data(&master(PRINTABLE_COPIES)),
        OTHER_DEFINITION_ID,
    );
}

// ── set_authority ───────────────────────────────────────────────────────────

#[test]
fn set_authority_rotates_the_authority() {
    let rotated = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        Some(AUTHORITY_ID),
        DEFINITION_ID,
        true,
    );
    assert_eq!(
        as_definition(&rotated),
        fungible_definition(INIT_SUPPLY, Some(AUTHORITY_ID))
    );
}

#[test]
fn set_authority_renounces_with_none() {
    let renounced = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        None,
        DEFINITION_ID,
        true,
    );
    assert_eq!(
        as_definition(&renounced),
        fungible_definition(INIT_SUPPLY, None)
    );
}

/// A rotated authority can rotate again without the definition's key: the only
/// thing that matters is that the claiming account matches the stored authority.
#[test]
fn set_authority_with_an_external_authority_succeeds() {
    let rotated = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(AUTHORITY_ID))),
        Some(HOLDING_ID),
        AUTHORITY_ID,
        true,
    );
    assert_eq!(
        as_definition(&rotated),
        fungible_definition(INIT_SUPPLY, Some(HOLDING_ID))
    );
}

#[test]
#[should_panic(expected = "Mint authority must authorize the transaction")]
fn set_authority_requires_the_authority_to_authorize() {
    let _ = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        None,
        DEFINITION_ID,
        false,
    );
}

#[test]
#[should_panic(expected = "SetAuthority failed: signer is not the current authority")]
fn set_authority_by_a_non_authority_should_fail() {
    let _ = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(AUTHORITY_ID))),
        None,
        HOLDING_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "SetAuthority failed: authority already revoked")]
fn set_authority_on_a_renounced_definition_should_fail() {
    let _ = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, None)),
        None,
        DEFINITION_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "New mint authority must be a valid non-zero account ID")]
fn set_authority_rejects_an_all_zero_new_authority() {
    let _ = set_authority::set_mint_authority(
        &definition_data(&fungible_definition(INIT_SUPPLY, Some(DEFINITION_ID))),
        Some(AccountId::new([0u8; 32])),
        DEFINITION_ID,
        true,
    );
}

#[test]
#[should_panic(expected = "SetAuthority is not supported for Non-Fungible Tokens")]
fn set_authority_is_not_supported_for_non_fungibles() {
    let _ = set_authority::set_mint_authority(
        &definition_data(&non_fungible_definition(PRINTABLE_COPIES)),
        None,
        DEFINITION_ID,
        true,
    );
}
