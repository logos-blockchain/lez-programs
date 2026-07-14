use std::collections::HashMap;

use ata_core::{compute_ata_seed, get_associated_token_account_id};
use integration_tests::{
    private_authorized_init_identity, private_unauthorized_identity, GroupOwner,
};
use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::ProgramWithDependencies, Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, SharedSecretKey, V03State,
};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    encryption::{EphemeralPublicKey, ViewingPublicKey},
    Commitment, EncryptedAccountData, InputAccountIdentity, NullifierPublicKey, NullifierSecretKey,
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
        let seed = compute_ata_seed(
            Self::token_program(),
            Self::owner(),
            Self::token_definition(),
        );
        get_associated_token_account_id(&Self::ata_program(), &seed)
    }

    fn recipient_ata() -> AccountId {
        let seed = compute_ata_seed(
            Self::token_program(),
            Self::recipient(),
            Self::token_definition(),
        );
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
                authority: None,
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

    fn recipient_ata_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
            nonce: Nonce(0),
        }
    }

    fn foreign_owned_token_definition() -> Account {
        Account {
            program_owner: [99; 8],
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Foreign Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: None,
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
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::owner_ata(), Accounts::owner_ata_init());
    state
}

fn state_for_ata_tests_with_precreated_recipient_ata() -> V03State {
    let mut state = state_for_ata_tests();
    state.force_insert_account(Ids::recipient_ata(), Accounts::recipient_ata_init());
    state
}

#[test]
fn ata_create() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
        token_program_id: Ids::token_program(),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
fn ata_create_rejects_definition_owned_by_unexpected_token_program() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::token_definition(),
        Accounts::foreign_owned_token_definition(),
    );

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());
    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account::default()
    );
}

#[test]
fn ata_create_rejects_existing_ata_owned_by_unexpected_token_program() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let mut foreign_ata = Accounts::owner_ata_init();
    foreign_ata.program_owner = [99; 8];
    state.force_insert_account(Ids::owner_ata(), foreign_ata.clone());

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());
    assert_eq!(state.get_account_by_id(Ids::owner_ata()), foreign_ata);
}

#[test]
fn ata_create_rejects_existing_ata_with_mismatched_definition() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let mut mismatched_ata = Accounts::owner_ata_init();
    mismatched_ata.data = Data::from(&TokenHolding::Fungible {
        definition_id: Ids::recipient(),
        balance: 1_000_000_u128,
    });
    state.force_insert_account(Ids::owner_ata(), mismatched_ata.clone());

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());
    assert_eq!(state.get_account_by_id(Ids::owner_ata()), mismatched_ata);
}

#[test]
fn ata_transfer() {
    let mut state = state_for_ata_tests_with_precreated_recipient_ata();

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
fn ata_transfer_rejects_default_recipient() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        amount: 1_u128,
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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Accounts::owner_ata_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        Account::default()
    );
}

#[test]
fn ata_transfer_rejects_mismatched_definition_recipient() {
    let mut state = state_for_ata_tests_with_precreated_recipient_ata();

    // Replace the recipient ATA with a token holding pointing at a different definition.
    let foreign_definition_id = AccountId::from(&PublicKey::new_from_private_key(
        &PrivateKey::try_new([42; 32]).expect("valid private key"),
    ));
    let mismatched_recipient = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: foreign_definition_id,
            balance: 0_u128,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(Ids::recipient_ata(), mismatched_recipient.clone());

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        amount: 1_u128,
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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Accounts::owner_ata_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        mismatched_recipient
    );
}

#[test]
fn ata_burn() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Burn {
        token_program_id: Ids::token_program(),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
                authority: None,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn ata_create_from_private_owner() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    // Private owner key material
    let owner_nsk: NullifierSecretKey = [13u8; 32];
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    // `ViewingPublicKey::from_seed` needs two 32-byte halves `(d, z)`.
    let owner_vpk = ViewingPublicKey::from_seed(&[31u8; 32], &[32u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, 0);

    // ATA derived from the private owner
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let owner_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    // Pre-states: private uninitialized owner, public token definition, public uninitialized ATA.
    let owner_pre = AccountWithMetadata::new(Account::default(), false, owner_id);
    let def_pre = AccountWithMetadata::new(
        Accounts::token_definition_init(),
        false,
        Ids::token_definition(),
    );
    let ata_pre = AccountWithMetadata::new(Account::default(), false, owner_ata_id);

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
    };
    let instruction_data = Program::serialize_instruction(instruction).unwrap();

    // Encapsulate a shared secret against the owner's viewing key; the circuit fills the EPK.
    let shared_secret = SharedSecretKey::encapsulate_deterministic(&owner_vpk, &[0u8; 32], 0).0;

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, def_pre, ata_pre],
        instruction_data,
        vec![
            // owner: new private account, not owned/spent by the caller (no nsk, no proof).
            InputAccountIdentity::PrivateUnauthorized {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&owner_npk, &owner_vpk),
                npk: owner_npk,
                ssk: shared_secret,
                identifier: 0,
            },
            // token_definition: public
            InputAccountIdentity::Public,
            // ata: public
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition(), owner_ata_id],
        vec![],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
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

/// ATA cannot be created as a private account.
#[test]
fn ata_create_private_ata_holding_is_not_expressible() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let owner_id = Ids::owner();
    let owner_account = state.get_account_by_id(owner_id);

    // Fresh personal npk/vpk for the ATA holding's privacy identity — distinct from the
    // owner's plain public keypair, which only supplies the seed input.
    let ata_nsk: NullifierSecretKey = [21u8; 32];
    let ata_npk = NullifierPublicKey::from(&ata_nsk);
    let ata_vpk = ViewingPublicKey::from_seed(&[41u8; 32], &[42u8; 32]);

    // Address stays the *standard* public-PDA formula ATA always uses — only its state
    // privacy is under test, not its address derivation.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    let owner_pre = AccountWithMetadata::new(owner_account, true, owner_id);
    let def_pre = AccountWithMetadata::new(
        Accounts::token_definition_init(),
        false,
        Ids::token_definition(),
    );
    let ata_pre = AccountWithMetadata::new(Account::default(), false, ata_id);

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
    };
    let instruction_data = Program::serialize_instruction(instruction).unwrap();

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&ata_vpk, &[0u8; 32], 0).0;

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let result = execute_and_prove(
        vec![owner_pre, def_pre, ata_pre],
        instruction_data,
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivatePdaInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&ata_npk, &ata_vpk),
                npk: ata_npk,
                ssk: shared_secret,
                identifier: 0,
                seed: None,
            },
        ],
        &program_with_deps,
    );

    let err = result.expect_err(
        "a private-PDA ATA holding must be rejected: its account id can never satisfy both \
         ATA's own for_public_pda address check and PrivatePdaInit's for_private_pda binding \
         requirement simultaneously",
    );
    let message = format!("{err:?}");
    assert!(
        message.contains("has no proven (seed, npk) binding via Claim::Pda or caller pda_seeds"),
        "expected the private-PDA binding rejection, got a different error: {message}"
    );
}

/// Verifies ATA account can be used to transfer to a private account.
#[test]
fn ata_transfer_to_existing_private_recipient() {
    let mut state = state_for_ata_tests();

    // A throwaway public holder to shield from directly via Token — bypassing ATA entirely,
    // since ATA::Transfer cannot originate a fresh private recipient (see doc comment above).
    let shield_source_key = PrivateKey::try_new([77u8; 32]).expect("valid private key");
    let shield_source_id = AccountId::from(&PublicKey::new_from_private_key(&shield_source_key));
    state.force_insert_account(
        shield_source_id,
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
            nonce: Nonce(0),
        },
    );

    let recipient_nsk: NullifierSecretKey = [51u8; 32];
    let recipient_npk = NullifierPublicKey::from(&recipient_nsk);
    let recipient_vpk = ViewingPublicKey::from_seed(&[61u8; 32], &[62u8; 32]);
    let recipient_id = AccountId::for_regular_private_account(&recipient_npk, 0);

    let shield_amount = 500_000_u128;
    let source_pre = AccountWithMetadata::new(
        state.get_account_by_id(shield_source_id),
        true,
        shield_source_id,
    );
    let fresh_recipient_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);
    let shield_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let token_program_for_shield = Program::new(token_methods::TOKEN_ELF.to_vec().into())
        .expect("valid token ELF")
        .into();
    let (shield_output, shield_proof) = execute_and_prove(
        vec![source_pre, fresh_recipient_pre],
        Program::serialize_instruction(token_core::Instruction::Transfer {
            amount_to_transfer: shield_amount,
        })
        .unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateUnauthorized {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                npk: recipient_npk,
                ssk: shield_secret,
                identifier: 0,
            },
        ],
        &token_program_for_shield,
    )
    .unwrap();
    let shield_message =
        Message::try_from_circuit_output(vec![shield_source_id], vec![Nonce(0)], shield_output)
            .unwrap();
    let shield_witness =
        WitnessSet::for_message(&shield_message, shield_proof, &[&shield_source_key]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(shield_message, shield_witness),
            0,
            0,
        )
        .unwrap();

    let recipient_after_shield = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    assert!(
        state
            .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_after_shield))
            .is_some(),
        "shield setup must land before the ATA transfer under test"
    );

    // Now the actual test: owner's ATA sends more into that now-existing private recipient.
    let owner_id = Ids::owner();
    let owner_account = state.get_account_by_id(owner_id);
    let sender_ata_id = Ids::owner_ata();
    let sender_ata_account = state.get_account_by_id(sender_ata_id);

    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_after_shield))
        .expect("recipient's commitment must be in the set");
    let transfer_secret =
        SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let owner_pre = AccountWithMetadata::new(owner_account, true, owner_id);
    let sender_ata_pre = AccountWithMetadata::new(sender_ata_account, false, sender_ata_id);
    let recipient_pre =
        AccountWithMetadata::new(recipient_after_shield.clone(), true, recipient_id);

    let ata_transfer_amount = 200_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        amount: ata_transfer_amount,
    };
    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, sender_ata_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: transfer_secret,
                nsk: recipient_nsk,
                membership_proof,
                identifier: 0,
            },
        ],
        &program_with_deps,
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![owner_id, sender_ata_id], vec![Nonce(0)], output)
            .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner_key()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(sender_ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - ata_transfer_amount,
            }),
            nonce: Nonce(0),
        }
    );

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let recipient_after_ata_transfer = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount + ata_transfer_amount,
        }),
        nonce: recipient_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(
            &recipient_id,
            &recipient_after_ata_transfer
        ))
        .is_some());
}

/// Private account owner can sign transactions.
#[test]
fn ata_burn_with_private_owner_signing() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let owner_nsk: NullifierSecretKey = [91u8; 32];
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    let owner_vpk = ViewingPublicKey::from_seed(&[93u8; 32], &[94u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, 0);

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let ata_account = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(ata_id, ata_account.clone());

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let ata_pre = AccountWithMetadata::new(ata_account, false, ata_id);
    let def_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );

    let burn_amount = 300_000_u128;
    let instruction = ata_core::Instruction::Burn {
        token_program_id: Ids::token_program(),
        amount: burn_amount,
    };

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&owner_vpk, &[0u8; 32], 0).0;

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, ata_pre, def_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::PrivateAuthorizedInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&owner_npk, &owner_vpk),
                ssk: shared_secret,
                nsk: owner_nsk,
                identifier: 0,
            },
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![ata_id, Ids::token_definition()], vec![], output)
            .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - burn_amount,
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
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    );

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}

/// Group-owned variant of `ata_burn_with_private_owner_signing`: the GMS is distributed through
/// the real seal/unseal handshake, and it's Bob — not Alice, who created the group — who
/// self-initializes and signs the owner identity in the same transaction via
/// `PrivateAuthorizedInit`, then burns from the ATA holding through it.
#[test]
fn ata_group_owned_owner_signing() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let alice = GroupOwner::new([97_u8; 32]);
    let owner_id = alice.id;
    let bob_nsk = alice.admit_member();

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let ata_account = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(ata_id, ata_account.clone());

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let ata_pre = AccountWithMetadata::new(ata_account, false, ata_id);
    let def_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );

    let burn_amount = 300_000_u128;
    let instruction = ata_core::Instruction::Burn {
        token_program_id: Ids::token_program(),
        amount: burn_amount,
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, ata_pre, def_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_authorized_init_identity(bob_nsk, &alice.vpk, 0),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![ata_id, Ids::token_definition()], vec![], output)
            .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - burn_amount,
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
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    );

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}

/// Private owner
#[test]
fn ata_transfer_with_private_owner_signing() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::recipient_ata(), Accounts::recipient_ata_init());

    let owner_nsk: NullifierSecretKey = [95u8; 32];
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    let owner_vpk = ViewingPublicKey::from_seed(&[96u8; 32], &[97u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, 0);

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let sender_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let sender_ata_account = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(sender_ata_id, sender_ata_account.clone());

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let sender_ata_pre = AccountWithMetadata::new(sender_ata_account, false, sender_ata_id);
    let recipient_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::recipient_ata()),
        false,
        Ids::recipient_ata(),
    );

    let transfer_amount = 400_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        amount: transfer_amount,
    };

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&owner_vpk, &[0u8; 32], 0).0;

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, sender_ata_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::PrivateAuthorizedInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&owner_npk, &owner_vpk),
                ssk: shared_secret,
                nsk: owner_nsk,
                identifier: 0,
            },
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![sender_ata_id, Ids::recipient_ata()], vec![], output)
            .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(sender_ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - transfer_amount,
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
                balance: transfer_amount,
            }),
            nonce: Nonce(0),
        }
    );

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}

/// Group transfer is possible with group members added after the ATA is initialized.
#[test]
fn ata_transfer_with_group_owned_owner_signing() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::recipient_ata(), Accounts::recipient_ata_init());

    let alice = GroupOwner::new([19_u8; 32]);
    let owner_id = alice.id;

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let sender_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let sender_ata_account = Account {
        program_owner: Ids::token_program(),
        balance: 0_u128,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(sender_ata_id, sender_ata_account.clone());

    let bob_nsk = alice.admit_member();

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let sender_ata_pre = AccountWithMetadata::new(sender_ata_account, false, sender_ata_id);
    let recipient_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::recipient_ata()),
        false,
        Ids::recipient_ata(),
    );

    let transfer_amount = 400_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        amount: transfer_amount,
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, sender_ata_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_authorized_init_identity(bob_nsk, &alice.vpk, 0),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![sender_ata_id, Ids::recipient_ata()], vec![], output)
            .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(sender_ata_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - transfer_amount,
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
                balance: transfer_amount,
            }),
            nonce: Nonce(0),
        }
    );

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}

#[test]
fn ata_create_from_group_owned_owner() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let alice = GroupOwner::new([23_u8; 32]);
    let owner_id = alice.id;

    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let owner_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    let owner_pre = AccountWithMetadata::new(Account::default(), false, owner_id);
    let def_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let ata_pre = AccountWithMetadata::new(Account::default(), false, owner_ata_id);

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        vec![owner_pre, def_pre, ata_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_unauthorized_identity(alice.npk, &alice.vpk, 0),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &program_with_deps,
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition(), owner_ata_id],
        vec![],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
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
