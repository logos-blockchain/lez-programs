use ata_core::{compute_ata_seed, get_associated_token_account_id};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ChainedCall, Claim, ProgramId},
};
use token_core::{TokenDefinition, TokenHolding};

const ATA_PROGRAM_ID: ProgramId = [1u32; 8];
const TOKEN_PROGRAM_ID: ProgramId = [2u32; 8];
const FOREIGN_TOKEN_PROGRAM_ID: ProgramId = [3u32; 8];

fn owner_id() -> AccountId {
    AccountId::new([0x01u8; 32])
}

fn definition_id() -> AccountId {
    AccountId::new([0x02u8; 32])
}

fn other_definition_id() -> AccountId {
    AccountId::new([0x03u8; 32])
}

fn ata_id() -> AccountId {
    get_associated_token_account_id(
        &ATA_PROGRAM_ID,
        &compute_ata_seed(owner_id(), definition_id()),
    )
}

fn recipient_id() -> AccountId {
    AccountId::new([0x04u8; 32])
}

fn owner_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: owner_id(),
    }
}

fn definition_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenDefinition::Fungible {
                name: "TEST".to_string(),
                total_supply: 1000,
                metadata_id: None,
            }),
            nonce: nssa_core::account::Nonce(0),
        },
        is_authorized: false,
        account_id: definition_id(),
    }
}

fn foreign_owned_definition_account() -> AccountWithMetadata {
    let mut account = definition_account();
    account.account.program_owner = FOREIGN_TOKEN_PROGRAM_ID;
    account
}

fn uninitialized_ata_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: ata_id(),
    }
}

fn initialized_ata_account_for_definition(definition_id: AccountId) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id,
                balance: 100,
            }),
            nonce: nssa_core::account::Nonce(0),
        },
        is_authorized: false,
        account_id: ata_id(),
    }
}

fn initialized_ata_account() -> AccountWithMetadata {
    initialized_ata_account_for_definition(definition_id())
}

fn foreign_owned_initialized_ata_account() -> AccountWithMetadata {
    let mut account = initialized_ata_account();
    account.account.program_owner = FOREIGN_TOKEN_PROGRAM_ID;
    account
}

fn malformed_initialized_ata_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::try_from(vec![0xFF]).expect("test data fits"),
            nonce: nssa_core::account::Nonce(0),
        },
        is_authorized: false,
        account_id: ata_id(),
    }
}

fn recipient_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition_id(),
                balance: 0,
            }),
            nonce: nssa_core::account::Nonce(0),
        },
        is_authorized: false,
        account_id: recipient_id(),
    }
}

#[test]
fn create_emits_chained_call_for_uninitialized_ata() {
    let (post_states, chained_calls) = crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        uninitialized_ata_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );

    assert_eq!(post_states.len(), 3);
    assert_eq!(post_states[0].required_claim(), Some(Claim::Authorized));

    let mut authorized_ata = uninitialized_ata_account();
    authorized_ata.is_authorized = true;
    let expected_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![definition_account(), authorized_ata],
        &token_core::Instruction::InitializeAccount,
    )
    .with_pda_seeds(vec![compute_ata_seed(owner_id(), definition_id())]);

    assert_eq!(chained_calls, vec![expected_call]);
}

#[test]
fn create_is_idempotent_for_initialized_ata() {
    let (post_states, chained_calls) = crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        initialized_ata_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );

    assert_eq!(post_states.len(), 3);
    assert!(
        chained_calls.is_empty(),
        "Should emit no chained call for already-initialized ATA"
    );
}

#[test]
#[should_panic(expected = "Token definition must be owned by token program")]
fn create_rejects_foreign_owned_token_definition() {
    crate::create::create_associated_token_account(
        owner_account(),
        foreign_owned_definition_account(),
        uninitialized_ata_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ATA account must hold a valid token")]
fn create_rejects_malformed_initialized_ata() {
    crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        malformed_initialized_ata_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ATA account must be owned by token program")]
fn create_rejects_foreign_owned_initialized_ata() {
    crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        foreign_owned_initialized_ata_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "Existing ATA token definition mismatch")]
fn create_rejects_initialized_ata_for_different_definition() {
    crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        initialized_ata_account_for_definition(other_definition_id()),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );
}

#[test]
#[should_panic(expected = "ATA account must be owned by token program")]
fn transfer_rejects_foreign_owned_sender_ata() {
    crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        foreign_owned_initialized_ata_account(),
        recipient_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        10,
    );
}

#[test]
#[should_panic(expected = "ATA account must be owned by token program")]
fn burn_rejects_foreign_owned_holder_ata() {
    crate::burn::burn_from_associated_token_account(
        owner_account(),
        foreign_owned_initialized_ata_account(),
        definition_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        10,
    );
}

#[test]
#[should_panic(expected = "Token definition must be owned by token program")]
fn burn_rejects_foreign_owned_token_definition() {
    crate::burn::burn_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        foreign_owned_definition_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        10,
    );
}

#[test]
#[should_panic(expected = "Holder ATA token definition mismatch")]
fn burn_rejects_holder_definition_mismatch() {
    crate::burn::burn_from_associated_token_account(
        owner_account(),
        initialized_ata_account_for_definition(other_definition_id()),
        definition_account(),
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        10,
    );
}

#[test]
#[should_panic(expected = "ATA account ID does not match expected derivation")]
fn create_panics_on_wrong_ata_address() {
    let wrong_ata = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([0xFFu8; 32]),
    };

    crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        wrong_ata,
        ATA_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
    );
}

#[test]
fn get_associated_token_account_id_is_deterministic() {
    let seed = compute_ata_seed(owner_id(), definition_id());
    let id1 = get_associated_token_account_id(&ATA_PROGRAM_ID, &seed);
    let id2 = get_associated_token_account_id(&ATA_PROGRAM_ID, &seed);
    assert_eq!(id1, id2);
}

#[test]
fn get_associated_token_account_id_differs_by_owner() {
    let other_owner = AccountId::new([0x99u8; 32]);
    let id1 = get_associated_token_account_id(
        &ATA_PROGRAM_ID,
        &compute_ata_seed(owner_id(), definition_id()),
    );
    let id2 = get_associated_token_account_id(
        &ATA_PROGRAM_ID,
        &compute_ata_seed(other_owner, definition_id()),
    );
    assert_ne!(id1, id2);
}

#[test]
fn get_associated_token_account_id_differs_by_definition() {
    let other_def = AccountId::new([0x99u8; 32]);
    let id1 = get_associated_token_account_id(
        &ATA_PROGRAM_ID,
        &compute_ata_seed(owner_id(), definition_id()),
    );
    let id2 =
        get_associated_token_account_id(&ATA_PROGRAM_ID, &compute_ata_seed(owner_id(), other_def));
    assert_ne!(id1, id2);
}
