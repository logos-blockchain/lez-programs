use std::collections::HashMap;

use ata_core::{compute_ata_seed, get_associated_token_account_id};
use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::ProgramWithDependencies, Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, EphemeralPublicKey, PrivateKey, PublicKey, PublicTransaction,
    SharedSecretKey, V03State,
};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    encryption::{Scalar, ViewingPublicKey},
    NullifierPublicKey, NullifierSecretKey,
};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Accounts;

impl Keys {
    fn def_key() -> PrivateKey {
        PrivateKey::try_new([10; 32]).expect("valid private key")
    }

    fn owner_key() -> PrivateKey {
        PrivateKey::try_new([11; 32]).expect("valid private key")
    }

    fn recipient_key() -> PrivateKey {
        PrivateKey::try_new([12; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn ata_program() -> nssa_core::program::ProgramId {
        ata_methods::ATA_ID
    }

    fn token_definition() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::def_key()))
    }

    fn owner() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::owner_key()))
    }

    fn recipient() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::recipient_key()))
    }

    fn owner_ata() -> AccountId {
        let seed = compute_ata_seed(Self::owner(), Self::token_definition());
        get_associated_token_account_id(&Self::ata_program(), &seed)
    }

    fn recipient_ata() -> AccountId {
        let seed = compute_ata_seed(Self::recipient(), Self::token_definition());
        get_associated_token_account_id(&Self::ata_program(), &seed)
    }
}

impl Accounts {
    fn token_definition_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn owner_ata_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
            nonce: Nonce(0),
        }
    }
}

fn deploy_programs(state: &mut V03State) {
    let token_message =
        program_deployment_transaction::Message::new(token_methods::TOKEN_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            token_message,
        ))
        .expect("token program deployment must succeed");

    let ata_message = program_deployment_transaction::Message::new(ata_methods::ATA_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            ata_message,
        ))
        .expect("ata program deployment must succeed");
}

fn state_for_ata_tests() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::owner_ata(), Accounts::owner_ata_init());
    state
}

#[test]
fn ata_create() {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let instruction = ata_core::Instruction::Create {
        ata_program_id: Ids::ata_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![Ids::owner(), Ids::token_definition(), Ids::owner_ata()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn ata_create_is_idempotent() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Create {
        ata_program_id: Ids::ata_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![Ids::owner(), Ids::token_definition(), Ids::owner_ata()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    // Already initialized — should remain unchanged
    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn ata_transfer() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Transfer {
        ata_program_id: Ids::ata_program(),
        amount: 400_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![Ids::owner(), Ids::owner_ata(), Ids::recipient_ata()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 600_000_u128,
            }),
            nonce: Nonce(0),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 400_000_u128,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn ata_burn() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Burn {
        ata_program_id: Ids::ata_program(),
        amount: 300_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![Ids::owner(), Ids::owner_ata(), Ids::token_definition()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 700_000_u128,
            }),
            nonce: Nonce(0),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 700_000_u128,
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn ata_create_from_private_owner() {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    // Private owner key material
    let owner_nsk: NullifierSecretKey = [13u8; 32];
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    let owner_vsk: Scalar = [31u8; 32];
    let owner_vpk = ViewingPublicKey::from_scalar(owner_vsk);
    let owner_id = AccountId::from(&owner_npk);

    // ATA derived from the private owner
    let seed = compute_ata_seed(owner_id, Ids::token_definition());
    let owner_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    // Pre-states: private uninitialized owner (mask=2), public token definition (mask=0), public
    // uninitialized ATA (mask=0)
    let owner_pre = AccountWithMetadata::new(Account::default(), false, owner_id);
    let def_pre = AccountWithMetadata::new(
        Accounts::token_definition_init(),
        false,
        Ids::token_definition(),
    );
    let ata_pre = AccountWithMetadata::new(Account::default(), false, owner_ata_id);

    let instruction = ata_core::Instruction::Create {
        ata_program_id: Ids::ata_program(),
    };
    let instruction_data = Program::serialize_instruction(instruction).unwrap();

    // Ephemeral key for encrypting the private owner's post-state
    let esk: Scalar = [3u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &owner_vpk);
    let epk = EphemeralPublicKey::from_scalar(esk);

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, def_pre, ata_pre],
        instruction_data,
        // owner=new private (2), token_definition=public (0), ata=public (0)
        vec![2, 0, 0],
        vec![(owner_npk.clone(), shared_secret)],
        vec![],     // no NSKs: new private accounts don't require one
        vec![None], // no membership proof: owner is being created, not spending
        &program_with_deps,
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition(), owner_ata_id],
        vec![],
        vec![(owner_npk, owner_vpk, epk)],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(owner_ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
            nonce: Nonce(0),
        }
    );
}
