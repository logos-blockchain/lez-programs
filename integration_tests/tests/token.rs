use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{Message, WitnessSet},
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivacyPreservingTransaction, PrivateKey, PublicKey, PublicTransaction,
    SharedSecretKey, V03State,
};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    encryption::{EphemeralPublicKey, ViewingPublicKey},
    Commitment, NullifierPublicKey, NullifierSecretKey,
};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Accounts;

impl Keys {
    fn def_key() -> PrivateKey {
        PrivateKey::try_new([10; 32]).expect("valid private key")
    }

    fn holder_key() -> PrivateKey {
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

    fn token_definition() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::def_key()))
    }

    fn holder() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::holder_key()))
    }

    fn recipient() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::recipient_key()))
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

    fn holder_init() -> Account {
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

fn deploy_token(state: &mut V03State) {
    let message = program_deployment_transaction::Message::new(token_methods::TOKEN_ELF.to_vec());
    let tx = ProgramDeploymentTransaction::new(message);
    state
        .transition_from_program_deployment_transaction(&tx)
        .expect("token program deployment must succeed");
}

fn state_for_token_tests() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_token(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::holder(), Accounts::holder_init());
    state
}

#[test]
fn token_new_fungible_definition() {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_token(&mut state);

    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("Gold"),
        total_supply: 1_000_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::holder()],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::def_key(), &Keys::holder_key()],
    );

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
            }),
            nonce: Nonce(1),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
            nonce: Nonce(1),
        }
    );
}

#[test]
fn token_transfer() {
    let mut state = state_for_token_tests();

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::holder(), Ids::recipient()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::holder_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 500_000_u128,
            }),
            nonce: Nonce(1),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 500_000_u128,
            }),
            nonce: Nonce(0),
        }
    );
}

#[test]
fn token_burn() {
    let mut state = state_for_token_tests();

    let instruction = token_core::Instruction::Burn {
        amount_to_burn: 200_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::holder()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::holder_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 800_000_u128,
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 800_000_u128,
            }),
            nonce: Nonce(1),
        }
    );
}

#[test]
fn token_mint() {
    let mut state = state_for_token_tests();

    let instruction = token_core::Instruction::Mint {
        amount_to_mint: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::holder()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::def_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_500_000_u128,
                metadata_id: None,
            }),
            nonce: Nonce(1),
        }
    );

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_500_000_u128,
            }),
            nonce: Nonce(0),
        }
    );
}

struct PrivateKeys;

impl PrivateKeys {
    fn holder_nsk() -> NullifierSecretKey {
        [42; 32]
    }

    fn holder_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::holder_nsk())
    }

    fn holder_vsk() -> [u8; 32] {
        [73; 32]
    }

    fn holder_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_scalar(Self::holder_vsk())
    }

    fn recipient_nsk() -> NullifierSecretKey {
        [84; 32]
    }

    fn recipient_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::recipient_nsk())
    }

    fn recipient_vsk() -> [u8; 32] {
        [48; 32]
    }

    fn recipient_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_scalar(Self::recipient_vsk())
    }
}

fn token_program() -> Program {
    Program::new(token_methods::TOKEN_ELF.to_vec()).expect("valid token ELF")
}

/// Performs a shielded transfer (public → private) of `amount` tokens from
/// `Ids::holder()` to a new private account keyed by `PrivateKeys::recipient_*`.
/// Returns the resulting private recipient account.
fn shielded_token_transfer(amount: u128, state: &mut V03State) -> Account {
    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;

    let sender = AccountWithMetadata::new(sender_account, true, sender_id);
    let recipient =
        AccountWithMetadata::new(Account::default(), false, &PrivateKeys::recipient_npk());

    let esk = [99u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &PrivateKeys::recipient_vpk());
    let epk = EphemeralPublicKey::from_scalar(esk);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender, recipient],
        Program::serialize_instruction(instruction).unwrap(),
        vec![0, 2],
        vec![(PrivateKeys::recipient_npk(), shared_secret)],
        vec![],
        vec![None],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![sender_id],
        vec![sender_nonce],
        vec![(
            PrivateKeys::recipient_npk(),
            PrivateKeys::recipient_vpk(),
            epk,
        )],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::holder_key()]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0)
        .unwrap();

    Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: amount,
        }),
        nonce: Nonce::private_account_nonce_init(&PrivateKeys::recipient_npk()),
    }
}

#[test]
fn token_shielded_transfer() {
    let mut state = state_for_token_tests();
    let amount = 500_000_u128;

    let recipient_account = shielded_token_transfer(amount, &mut state);

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000 - amount,
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_commitment = Commitment::new(&PrivateKeys::recipient_npk(), &recipient_account);
    assert!(state
        .get_proof_for_commitment(&recipient_commitment)
        .is_some());
}

#[test]
fn token_private_transfer() {
    let mut state = state_for_token_tests();
    let shielded_amount = 500_000_u128;
    let transfer_amount = 200_000_u128;

    // Shield tokens into a private account (becomes the sender for the private transfer).
    let sender_account = shielded_token_transfer(shielded_amount, &mut state);
    let sender_npk = PrivateKeys::recipient_npk();
    let sender_nsk = PrivateKeys::recipient_nsk();
    let sender_vpk = PrivateKeys::recipient_vpk();

    let new_recipient_npk = PrivateKeys::holder_npk();
    let new_recipient_vpk = PrivateKeys::holder_vpk();

    let sender_commitment = Commitment::new(&sender_npk, &sender_account);

    let esk_1 = [11u8; 32];
    let shared_secret_1 = SharedSecretKey::new(&esk_1, &sender_vpk);
    let epk_1 = EphemeralPublicKey::from_scalar(esk_1);

    let esk_2 = [22u8; 32];
    let shared_secret_2 = SharedSecretKey::new(&esk_2, &new_recipient_vpk);
    let epk_2 = EphemeralPublicKey::from_scalar(esk_2);

    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, &sender_npk);
    let new_recipient_pre = AccountWithMetadata::new(Account::default(), false, &new_recipient_npk);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, new_recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![1, 2],
        vec![
            (sender_npk.clone(), shared_secret_1),
            (new_recipient_npk.clone(), shared_secret_2),
        ],
        vec![sender_nsk],
        vec![state.get_proof_for_commitment(&sender_commitment), None],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![],
        vec![],
        vec![
            (sender_npk.clone(), sender_vpk, epk_1),
            (new_recipient_npk.clone(), new_recipient_vpk, epk_2),
        ],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0)
        .unwrap();

    let sender_nonce_after =
        Nonce::private_account_nonce_init(&sender_npk).private_account_nonce_increment(&sender_nsk);
    let new_sender_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shielded_amount - transfer_amount,
        }),
        nonce: sender_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&sender_npk, &new_sender_account))
        .is_some());

    let new_recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: transfer_amount,
        }),
        nonce: Nonce::private_account_nonce_init(&new_recipient_npk),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&new_recipient_npk, &new_recipient_account))
        .is_some());
}

#[test]
fn token_deshielded_transfer() {
    let mut state = state_for_token_tests();
    let shielded_amount = 500_000_u128;
    let deshield_amount = 300_000_u128;

    // Shield tokens into a private account, then deshield some back to a public account.
    let sender_account = shielded_token_transfer(shielded_amount, &mut state);
    let sender_npk = PrivateKeys::recipient_npk();
    let sender_nsk = PrivateKeys::recipient_nsk();
    let sender_vpk = PrivateKeys::recipient_vpk();

    let public_recipient_id = Ids::recipient();
    let sender_commitment = Commitment::new(&sender_npk, &sender_account);

    let esk = [55u8; 32];
    let shared_secret = SharedSecretKey::new(&esk, &sender_vpk);
    let epk = EphemeralPublicKey::from_scalar(esk);

    let public_recipient_pre = AccountWithMetadata::new(
        state.get_account_by_id(public_recipient_id),
        false,
        public_recipient_id,
    );
    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, &sender_npk);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: deshield_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, public_recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![1, 0],
        vec![(sender_npk.clone(), shared_secret)],
        vec![sender_nsk],
        vec![state.get_proof_for_commitment(&sender_commitment)],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![public_recipient_id],
        vec![],
        vec![(sender_npk.clone(), sender_vpk, epk)],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(public_recipient_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: deshield_amount,
            }),
            nonce: Nonce(0),
        }
    );

    let sender_nonce_after =
        Nonce::private_account_nonce_init(&sender_npk).private_account_nonce_increment(&sender_nsk);
    let new_sender_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shielded_amount - deshield_amount,
        }),
        nonce: sender_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&sender_npk, &new_sender_account))
        .is_some());
}
