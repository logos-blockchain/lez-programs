use ata_core::{compute_ata_seed, get_associated_token_account_id};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::{ChainedCall, Claim},
};
use token_core::{TokenDefinition, TokenHolding};

const ATA_PROGRAM_ID: nssa_core::program::ProgramId = [1u32; 8];
const TOKEN_PROGRAM_ID: nssa_core::program::ProgramId = [2u32; 8];

fn owner_id() -> AccountId {
    AccountId::new([0x01u8; 32])
}

fn definition_id() -> AccountId {
    AccountId::new([0x02u8; 32])
}

fn ata_id() -> AccountId {
    get_associated_token_account_id(
        &ATA_PROGRAM_ID,
        &compute_ata_seed(owner_id(), definition_id()),
    )
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

fn uninitialized_ata_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: ata_id(),
    }
}

fn initialized_ata_account() -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: definition_id(),
                balance: 100,
            }),
            nonce: nssa_core::account::Nonce(0),
        },
        is_authorized: false,
        account_id: ata_id(),
    }
}

#[test]
fn create_emits_chained_call_for_uninitialized_ata() {
    let (post_states, chained_calls) = crate::create::create_associated_token_account(
        owner_account(),
        definition_account(),
        uninitialized_ata_account(),
        ATA_PROGRAM_ID,
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
    );

    assert_eq!(post_states.len(), 3);
    assert!(
        chained_calls.is_empty(),
        "Should emit no chained call for already-initialized ATA"
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

fn recipient_id() -> AccountId {
    AccountId::new([0x03u8; 32])
}

fn initialized_recipient_account() -> AccountWithMetadata {
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
fn transfer_emits_chained_call_for_initialized_recipient() {
    let (post_states, chained_calls) = crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        initialized_recipient_account(),
        ATA_PROGRAM_ID,
        25,
    );

    assert_eq!(post_states.len(), 3);
    assert_eq!(chained_calls.len(), 1);

    let mut sender_auth = initialized_ata_account();
    sender_auth.is_authorized = true;
    let expected_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![sender_auth, initialized_recipient_account()],
        &token_core::Instruction::Transfer {
            amount_to_transfer: 25,
        },
    )
    .with_pda_seeds(vec![compute_ata_seed(owner_id(), definition_id())]);

    assert_eq!(chained_calls, vec![expected_call]);
}

#[test]
#[should_panic(expected = "Owner authorization is missing")]
fn transfer_panics_when_owner_not_authorized() {
    let mut unauthorized_owner = owner_account();
    unauthorized_owner.is_authorized = false;

    crate::transfer::transfer_from_associated_token_account(
        unauthorized_owner,
        initialized_ata_account(),
        initialized_recipient_account(),
        ATA_PROGRAM_ID,
        1,
    );
}

#[test]
#[should_panic(expected = "Recipient token holding must be initialized")]
fn transfer_panics_when_recipient_is_default() {
    let default_recipient = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: recipient_id(),
    };

    crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        default_recipient,
        ATA_PROGRAM_ID,
        1,
    );
}

#[test]
#[should_panic(expected = "Recipient must be owned by the same token program as the sender ATA")]
fn transfer_panics_when_recipient_is_foreign_owned() {
    let mut foreign_recipient = initialized_recipient_account();
    foreign_recipient.account.program_owner = [9u32; 8];

    crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        foreign_recipient,
        ATA_PROGRAM_ID,
        1,
    );
}

#[test]
#[should_panic(expected = "Recipient must hold a valid token")]
fn transfer_panics_when_recipient_data_is_malformed() {
    let mut malformed_recipient = initialized_recipient_account();
    malformed_recipient.account.data = Data::try_from(vec![0xFFu8, 0xFE, 0xFD]).unwrap();

    crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        malformed_recipient,
        ATA_PROGRAM_ID,
        1,
    );
}

#[test]
#[should_panic(expected = "Recipient and sender token definitions do not match")]
fn transfer_panics_when_recipient_definition_mismatches_sender() {
    let mut mismatched_recipient = initialized_recipient_account();
    mismatched_recipient.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: AccountId::new([0xAAu8; 32]),
        balance: 0,
    });

    crate::transfer::transfer_from_associated_token_account(
        owner_account(),
        initialized_ata_account(),
        mismatched_recipient,
        ATA_PROGRAM_ID,
        1,
    );
}
