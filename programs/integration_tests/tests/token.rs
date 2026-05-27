use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{Message, PrivacyPreservingTransaction, WitnessSet},
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

    fn holder_key() -> PrivateKey {
        PrivateKey::try_new([11; 32]).expect("valid private key")
    }

    fn recipient_key() -> PrivateKey {
        PrivateKey::try_new([12; 32]).expect("valid private key")
    }

    fn authority_key() -> PrivateKey {
        PrivateKey::try_new([13; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn foreign_token_program() -> nssa_core::program::ProgramId {
        [0xfeed_u32; 8]
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

    fn authority() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::authority_key()))
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
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(0),
        }
    }

    fn token_definition_foreign_owner() -> Account {
        Account {
            program_owner: Ids::foreign_token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
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

    fn recipient_init() -> Account {
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

    fn authority_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::default(),
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
    let mut state = V03State::new();
    deploy_token(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::holder(), Accounts::holder_init());
    state.force_insert_account(Ids::recipient(), Accounts::recipient_init());
    state.force_insert_account(Ids::authority(), Accounts::authority_init());
    state
}

fn state_for_token_tests_without_recipient() -> V03State {
    let mut state = V03State::new();
    deploy_token(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());
    state.force_insert_account(Ids::holder(), Accounts::holder_init());
    state.force_insert_account(Ids::authority(), Accounts::authority_init());
    state
}

#[test]
fn token_new_fungible_definition() {
    let mut state = V03State::new();
    deploy_token(&mut state);

    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("Gold"),
        total_supply: 1_000_000_u128,
        mint_authority: None,
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: None,
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
fn token_initialize_account_succeeds_for_canonical_definition() {
    let mut state = state_for_token_tests_without_recipient();

    let instruction = token_core::Instruction::InitializeAccount;

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::recipient()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::recipient_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Accounts::token_definition_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
            nonce: Nonce(1),
        }
    );
}

#[test]
fn token_initialize_account_rejects_foreign_owned_definition() {
    let mut state = state_for_token_tests_without_recipient();
    state.force_insert_account(
        Ids::token_definition(),
        Accounts::token_definition_foreign_owner(),
    );

    let instruction = token_core::Instruction::InitializeAccount;

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::recipient()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::recipient_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Accounts::token_definition_foreign_owner()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account::default()
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
fn token_transfer_fresh_public_recipient_requires_authorization() {
    let mut state = state_for_token_tests_without_recipient();

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
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Accounts::holder_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account::default()
    );
}

#[test]
fn token_transfer_fresh_authorized_public_recipient() {
    let mut state = state_for_token_tests_without_recipient();

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::holder(), Ids::recipient()],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::holder_key(), &Keys::recipient_key()],
    );

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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
            nonce: Nonce(1),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 800_000_u128,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_500_000_u128,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
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

#[test]
fn token_mint_rejects_foreign_owned_definition() {
    let mut state = state_for_token_tests_without_recipient();
    state.force_insert_account(
        Ids::token_definition(),
        Accounts::token_definition_foreign_owner(),
    );

    let instruction = token_core::Instruction::Mint {
        amount_to_mint: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::recipient()],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::def_key(), &Keys::recipient_key()],
    );

    let tx = PublicTransaction::new(message, witness_set);
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Accounts::token_definition_foreign_owner()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account::default()
    );
}

#[test]
fn token_mint_fresh_public_recipient_requires_authorization() {
    let mut state = state_for_token_tests_without_recipient();

    let instruction = token_core::Instruction::Mint {
        amount_to_mint: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::recipient()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::def_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Accounts::token_definition_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Account::default()
    );
}

#[test]
fn token_mint_fresh_authorized_public_recipient() {
    let mut state = state_for_token_tests_without_recipient();

    let instruction = token_core::Instruction::Mint {
        amount_to_mint: 500_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::recipient()],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::def_key(), &Keys::recipient_key()],
    );

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_500_000_u128,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
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
            nonce: Nonce(1),
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

    // `ViewingPublicKey::from_seed` needs two 32-byte halves `(d, z)`. We reuse the
    // legacy viewing scalar as `d` and pick a fixed distinct `z`.
    fn holder_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[73; 32], &[74; 32])
    }

    fn holder_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::holder_npk(), 0)
    }

    fn recipient_nsk() -> NullifierSecretKey {
        [84; 32]
    }

    fn recipient_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::recipient_nsk())
    }

    fn recipient_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[48; 32], &[49; 32])
    }

    fn recipient_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::recipient_npk(), 0)
    }
}

fn token_program() -> Program {
    Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF")
}

/// Performs a shielded transfer (public → private) of `amount` tokens from
/// `Ids::holder()` to a new private account keyed by `PrivateKeys::recipient_*`.
/// Returns the resulting private recipient account.
#[cfg(test)]
fn shielded_token_transfer(amount: u128, state: &mut V03State) -> Account {
    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;

    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let sender = AccountWithMetadata::new(sender_account, true, sender_id);
    let recipient = AccountWithMetadata::new(Account::default(), false, recipient_id);

    // Sender encapsulates a shared secret against the recipient's viewing key. The
    // circuit fills the real EPK, so we pass an empty placeholder in the identity.
    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender, recipient],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateUnauthorized {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                npk: recipient_npk,
                ssk: shared_secret,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![sender_id], vec![sender_nonce], output).unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::holder_key()]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: amount,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
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

    let recipient_commitment = Commitment::new(&PrivateKeys::recipient_id(), &recipient_account);
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
    let sender_id = PrivateKeys::recipient_id();

    let new_recipient_npk = PrivateKeys::holder_npk();
    let new_recipient_vpk = PrivateKeys::holder_vpk();
    let new_recipient_id = PrivateKeys::holder_id();

    let sender_commitment = Commitment::new(&sender_id, &sender_account);
    let membership_proof = state
        .get_proof_for_commitment(&sender_commitment)
        .expect("sender's commitment must be in the set");

    // Distinct `output_index` per private output keeps the encapsulated secrets reproducible.
    let shared_secret_1 = SharedSecretKey::encapsulate_deterministic(&sender_vpk, &[0u8; 32], 0).0;
    let shared_secret_2 =
        SharedSecretKey::encapsulate_deterministic(&new_recipient_vpk, &[0u8; 32], 1).0;

    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, sender_id);
    let new_recipient_pre = AccountWithMetadata::new(Account::default(), false, new_recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, new_recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&sender_npk, &sender_vpk),
                ssk: shared_secret_1,
                nsk: sender_nsk,
                membership_proof,
                identifier: 0,
            },
            InputAccountIdentity::PrivateUnauthorized {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(
                    &new_recipient_npk,
                    &new_recipient_vpk,
                ),
                npk: new_recipient_npk,
                ssk: shared_secret_2,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(vec![], vec![], output).unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    let sender_nonce_after =
        Nonce::private_account_nonce_init(&sender_id).private_account_nonce_increment(&sender_nsk);
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
        .get_proof_for_commitment(&Commitment::new(&sender_id, &new_sender_account))
        .is_some());

    let new_recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: transfer_amount,
        }),
        nonce: Nonce::private_account_nonce_init(&new_recipient_id),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&new_recipient_id, &new_recipient_account))
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
    let sender_id = PrivateKeys::recipient_id();

    let public_recipient_id = Ids::recipient();
    let sender_commitment = Commitment::new(&sender_id, &sender_account);
    let membership_proof = state
        .get_proof_for_commitment(&sender_commitment)
        .expect("sender's commitment must be in the set");

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&sender_vpk, &[0u8; 32], 0).0;

    let public_recipient_pre = AccountWithMetadata::new(
        state.get_account_by_id(public_recipient_id),
        false,
        public_recipient_id,
    );
    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, sender_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: deshield_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, public_recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&sender_npk, &sender_vpk),
                ssk: shared_secret,
                nsk: sender_nsk,
                membership_proof,
                identifier: 0,
            },
            InputAccountIdentity::Public,
        ],
        &token_program().into(),
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![public_recipient_id], vec![], output).unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
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
        Nonce::private_account_nonce_init(&sender_id).private_account_nonce_increment(&sender_nsk);
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
        .get_proof_for_commitment(&Commitment::new(&sender_id, &new_sender_account))
        .is_some());
}

#[test]
fn token_new_fungible_definition_with_authority() {
    let mut state = V03State::new();
    deploy_token(&mut state);
    let authority_key: [u8; 32] = Ids::token_definition()
        .as_ref()
        .try_into()
        .expect("AccountId is always 32 bytes");
    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("AuthCoin"),
        total_supply: 1_000_000_u128,
        mint_authority: Some(AccountId::new(authority_key)),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("AuthCoin"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: Some(AccountId::new(authority_key)),
            }),
            nonce: Nonce(1),
        }
    );
}

#[test]
fn token_set_authority_revoke() {
    let mut state = V03State::new();
    deploy_token(&mut state);
    let authority_key: [u8; 32] = Ids::token_definition()
        .as_ref()
        .try_into()
        .expect("AccountId is always 32 bytes");
    // Create token with authority
    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("AuthCoin"),
        total_supply: 1_000_000_u128,
        mint_authority: Some(AccountId::new(authority_key)),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Seed the authority account so it can sign the revoke
    state.force_insert_account(Ids::authority(), Accounts::authority_init());

    // Revoke authority
    let instruction = token_core::Instruction::SetAuthority {
        new_authority: None,
    };
    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition()],
        vec![Nonce(1)],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::def_key()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("AuthCoin"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(2),
        }
    );
}

/// After the authority is rotated to an external key, that external key can rotate
/// or revoke again via `SetAuthorityWithAuthority` — signing as a distinct authority
/// account while the definition account does not sign.
#[test]
fn token_set_authority_with_authority_revokes() {
    let mut state = V03State::new();
    deploy_token(&mut state);

    // Create with self-authority (definition is the initial mint authority).
    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("RotCoin"),
        total_supply: 1_000_000_u128,
        mint_authority: Some(Ids::token_definition()),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Rotate to the external authority via self-authority (def_key signs).
    let instruction = token_core::Instruction::SetAuthority {
        new_authority: Some(Ids::authority()),
    };
    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition()],
        vec![Nonce(1)],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::def_key()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Seed the external authority so it can sign.
    state.force_insert_account(Ids::authority(), Accounts::authority_init());

    // The external authority revokes via SetAuthorityWithAuthority. Accounts:
    // [definition, authority]; only the authority signs.
    let instruction = token_core::Instruction::SetAuthorityWithAuthority {
        new_authority: None,
    };
    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::authority()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();
    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::authority_key()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    let def = state.get_account_by_id(Ids::token_definition());
    let stored = match TokenDefinition::try_from(&def.data).unwrap() {
        TokenDefinition::Fungible { authority, .. } => authority,
        _ => None,
    };
    assert_eq!(stored, None, "authority must be permanently revoked");
}

/// Integration test for RFP-001 authority rotation flow:
/// 1. Create a token where `Ids::token_definition()` is the initial mint authority
///    (self-authority).
/// 2. Rotate the mint authority to `Ids::authority()` (an external key).
/// 3. Verify that the new external authority can mint by presenting itself as a rest account.
/// 4. Verify that the OLD authority (def key) can no longer mint after rotation.
#[test]
fn token_rotate_authority_then_new_authority_can_mint() {
    let mut state = V03State::new();
    deploy_token(&mut state);

    let authority_key: [u8; 32] = Ids::authority()
        .as_ref()
        .try_into()
        .expect("AccountId is always 32 bytes");

    // Step 1: Create token with self-authority (def account is initial mint authority).
    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("RotCoin"),
        total_supply: 1_000_000_u128,
        mint_authority: Some(AccountId::new(
            Ids::token_definition()
                .as_ref()
                .try_into()
                .expect("AccountId is always 32 bytes"),
        )),
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
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Step 2: Rotate mint authority from def_key to Ids::authority() (external key).
    // Self-authority path: no rest accounts; def_key signs.
    let instruction = token_core::Instruction::SetAuthority {
        new_authority: Some(AccountId::new(authority_key)),
    };
    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition()],
        vec![Nonce(1)],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::def_key()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Verify the authority slot now holds Ids::authority().
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("RotCoin"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: Some(AccountId::new(authority_key)),
            }),
            nonce: Nonce(2),
        }
    );

    // Seed the external authority account and the holder so they exist in state.
    state.force_insert_account(Ids::authority(), Accounts::authority_init());
    state.force_insert_account(Ids::holder(), Accounts::holder_init());

    // Step 3: New external authority mints via MintWithAuthority, signing as a
    // distinct authority account. Accounts: [definition, holder, authority].
    let instruction = token_core::Instruction::MintWithAuthority {
        amount_to_mint: 500_000_u128,
    };
    let message = public_transaction::Message::try_new(
        Ids::token_program(),
        vec![Ids::token_definition(), Ids::holder(), Ids::authority()],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();
    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::authority_key()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    // Verify total_supply increased and holder balance reflects the mint.
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("RotCoin"),
                total_supply: 1_500_000_u128,
                metadata_id: None,
                authority: Some(AccountId::new(authority_key)),
            }),
            nonce: Nonce(2),
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

    // Step 4: OLD authority (def_key self-authority path) must be rejected after rotation.
    let instruction = token_core::Instruction::Mint {
        amount_to_mint: 1_u128,
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
    let result = state.transition_from_public_transaction(&tx, 0, 0);
    assert!(
        result.is_err(),
        "Old authority must be rejected after rotation"
    );
}
