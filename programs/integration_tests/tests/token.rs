use key_protocol::key_management::{
    group_key_holder::{GroupKeyHolder, SealingPublicKey},
    secret_holders::SecretSpendingKey,
};
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
    Commitment, EncryptedAccountData, InputAccountIdentity, Nullifier, NullifierPublicKey,
    NullifierSecretKey,
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

/// Shielded transaction to a private account using the account's `nsk`.
/// `token_shielded_transfer` only uses the account's `npk`; thus, `PrivateUnauthorized` private.
#[test]
fn token_shielded_transfer_authorized_private_init() {
    let mut state = state_for_token_tests();
    let amount = 500_000_u128;

    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;

    let recipient_nsk = PrivateKeys::recipient_nsk();
    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);
    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: shared_secret,
                nsk: recipient_nsk,
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

    assert_eq!(
        state.get_account_by_id(sender_id),
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

    let recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: amount,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
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

/// Mints directly to a new recipient private holding (`PrivateUnauthorized`).
/// The recipient's cooperation is unnecessary; only known of the recipient's `npk`, `vpk`.
#[test]
fn token_mint_shielded_to_private_unauthorized() {
    let mut state = state_for_token_tests_without_recipient();
    let amount_to_mint = 500_000_u128;

    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let recipient_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Mint { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, recipient_pre],
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

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition()],
        vec![definition_nonce],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::def_key()]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 + amount_to_mint,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: amount_to_mint,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
        .is_some());
}

/// Mints directly to a new recipient private holding (`PrivateAuthorizedInit`).
/// This requires the recipient's secret key `nsk`.
#[test]
fn token_mint_authorized_private_init() {
    let mut state = state_for_token_tests_without_recipient();
    let amount_to_mint = 500_000_u128;

    let recipient_nsk = PrivateKeys::recipient_nsk();
    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Mint { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: shared_secret,
                nsk: recipient_nsk,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition()],
        vec![definition_nonce],
        output,
    )
    .unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::def_key()]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 + amount_to_mint,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: amount_to_mint,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
        .is_some());
}

/// Mints directly to a pre-existing recipient private holding
/// This requires the recipient's secret key `nsk`.
#[test]
fn token_mint_into_existing_private_holding() {
    let mut state = state_for_token_tests_without_recipient();
    let pre_balance = 500_000_u128;
    let amount_to_mint = 250_000_u128;

    let recipient_nsk = PrivateKeys::recipient_nsk();
    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let recipient_pre = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: pre_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    state = state.with_private_accounts([(
        Commitment::new(&recipient_id, &recipient_pre),
        Nullifier::for_account_initialization(&recipient_id),
    )]);
    assert!(
        state
            .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_pre))
            .is_some(),
        "seeded balance must land before the existing-holding mint under test"
    );

    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_pre))
        .expect("recipient's commitment must be in the set");

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let existing_recipient_pre =
        AccountWithMetadata::new(recipient_pre.clone(), true, recipient_id);

    let shared_secret =
        SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let (output, second_proof) = execute_and_prove(
        vec![definition_pre, existing_recipient_pre],
        Program::serialize_instruction(token_core::Instruction::Mint {
            amount_to_mint,
        })
        .unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: shared_secret,
                nsk: recipient_nsk,
                membership_proof,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition()],
        vec![definition_nonce],
        output,
    )
    .unwrap();
    let witness =
        WitnessSet::for_message(&message, second_proof, &[&Keys::def_key()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 + amount_to_mint,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let recipient_after_second_mint = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: pre_balance + amount_to_mint,
        }),
        nonce: recipient_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(
            &recipient_id,
            &recipient_after_second_mint
        ))
        .is_some());
}

/// Burns from an existing private holding (`PrivateAuthorizedUpdate`).
#[test]
fn token_private_burn() {
    let mut state = state_for_token_tests();
    let holding_balance = 500_000_u128;
    let burn_amount = 200_000_u128;

    let holder_npk = PrivateKeys::recipient_npk();
    let holder_nsk = PrivateKeys::recipient_nsk();
    let holder_vpk = PrivateKeys::recipient_vpk();
    let holder_id = PrivateKeys::recipient_id();

    // Predefined holding account to burn from.
    let holder_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: holding_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&holder_id),
    };
    let holder_commitment = Commitment::new(&holder_id, &holder_account);
    state = state.with_private_accounts([(
        holder_commitment.clone(),
        Nullifier::for_account_initialization(&holder_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&holder_commitment)
        .expect("holder's commitment must be in the set");

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&holder_vpk, &[0u8; 32], 0).0;

    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let holder_pre = AccountWithMetadata::new(holder_account.clone(), true, holder_id);

    let instruction = token_core::Instruction::Burn {
        amount_to_burn: burn_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, holder_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&holder_npk, &holder_vpk),
                ssk: shared_secret,
                nsk: holder_nsk,
                membership_proof,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![Ids::token_definition()], vec![], output).unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(0),
        }
    );

    let new_holder_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: holding_balance - burn_amount,
        }),
        nonce: Nonce::private_account_nonce_init(&holder_id)
            .private_account_nonce_increment(&holder_nsk),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&holder_id, &new_holder_account))
        .is_some());
}

/// Token transfer into a pre-existing Token holding account. This requires
/// the account's `nsk`; `PrivateAuthorizedUpdate`.
#[test]
fn token_transfer_into_existing_private_holding() {
    let mut state = state_for_token_tests();
    let init_balance = 500_000_u128;
    let second_amount = 100_000_u128;

    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_nsk = PrivateKeys::recipient_nsk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: init_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    let recipient_commitment = Commitment::new(&recipient_id, &recipient_account);
    state = state.with_private_accounts([(
        recipient_commitment.clone(),
        Nullifier::for_account_initialization(&recipient_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&recipient_commitment)
        .expect("recipient's commitment must be in the set after seeding");

    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);
    let recipient_pre = AccountWithMetadata::new(recipient_account.clone(), true, recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: second_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: shared_secret,
                nsk: recipient_nsk,
                membership_proof,
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

    assert_eq!(
        state.get_account_by_id(sender_id),
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                // `first_amount` was seeded directly into the recipient, never debited from
                // the sender — only the real transfer (`second_amount`) actually happened.
                balance: 1_000_000 - second_amount,
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let new_recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: init_balance + second_amount,
        }),
        nonce: recipient_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &new_recipient_account))
        .is_some());
}

// Marvin-todo
/// Fully private counterpart to `token_transfer_into_existing_private_holding`: instead of a
/// *public* sender crediting an existing private recipient, both legs are private and the
/// recipient already exists (not fresh, unlike `token_private_transfer`'s new recipient). This
/// is a new combination — two distinct private accounts, both driven by
/// `PrivateAuthorizedUpdate` (spend + credit-existing) in the same transaction — that neither
/// existing test covers. `Token::Transfer` has no definition-account parameter at all, so with
/// both legs private there is no public account anywhere in this transaction: no signer, no
/// public message ids.
#[test]
fn token_private_transfer_into_existing_private_holding() {
    let mut state = state_for_token_tests();
    let sender_initial_balance = 500_000_u128;
    let recipient_initial_balance = 300_000_u128;
    let transfer_amount = 200_000_u128;

    let sender_npk = PrivateKeys::recipient_npk();
    let sender_nsk = PrivateKeys::recipient_nsk();
    let sender_vpk = PrivateKeys::recipient_vpk();
    let sender_id = PrivateKeys::recipient_id();

    let recipient_npk = PrivateKeys::holder_npk();
    let recipient_nsk = PrivateKeys::holder_nsk();
    let recipient_vpk = PrivateKeys::holder_vpk();
    let recipient_id = PrivateKeys::holder_id();

    // Seed both sides directly — neither needs a real prior transaction to exist.
    let sender_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: sender_initial_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&sender_id),
    };
    let recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: recipient_initial_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&recipient_id),
    };
    state = state.with_private_accounts([
        (
            Commitment::new(&sender_id, &sender_account),
            Nullifier::for_account_initialization(&sender_id),
        ),
        (
            Commitment::new(&recipient_id, &recipient_account),
            Nullifier::for_account_initialization(&recipient_id),
        ),
    ]);

    let sender_membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&sender_id, &sender_account))
        .expect("sender's commitment must be in the set");
    let recipient_membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
        .expect("recipient's commitment must be in the set");

    let sender_shared_secret =
        SharedSecretKey::encapsulate_deterministic(&sender_vpk, &[0u8; 32], 0).0;
    let recipient_shared_secret =
        SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 1).0;

    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, sender_id);
    let recipient_pre = AccountWithMetadata::new(recipient_account.clone(), true, recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&sender_npk, &sender_vpk),
                ssk: sender_shared_secret,
                nsk: sender_nsk,
                membership_proof: sender_membership_proof,
                identifier: 0,
            },
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                ssk: recipient_shared_secret,
                nsk: recipient_nsk,
                membership_proof: recipient_membership_proof,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(vec![], vec![], output).unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let sender_nonce_after =
        Nonce::private_account_nonce_init(&sender_id).private_account_nonce_increment(&sender_nsk);
    let new_sender_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: sender_initial_balance - transfer_amount,
        }),
        nonce: sender_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&sender_id, &new_sender_account))
        .is_some());

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let new_recipient_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: recipient_initial_balance + transfer_amount,
        }),
        nonce: recipient_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &new_recipient_account))
        .is_some());
}

/// Initializes a private holding account directly (private account secret keys are known).
/// `InitializeAccount` requires `account_to_initialize` to be authorized. E.g., for private
/// accounts must be `PrivateAuthorizedInit` and not `PrivateUnauthorized`; the account owner
/// must supply their own `nsk`.
#[test]
fn token_initialize_private_account_succeeds_for_canonical_definition() {
    let mut state = state_for_token_tests_without_recipient();

    let owner_nsk = PrivateKeys::recipient_nsk();
    let owner_npk = PrivateKeys::recipient_npk();
    let owner_vpk = PrivateKeys::recipient_vpk();
    let owner_id = PrivateKeys::recipient_id();

    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let account_to_init_pre = AccountWithMetadata::new(Account::default(), true, owner_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&owner_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::InitializeAccount;
    let (output, proof) = execute_and_prove(
        vec![definition_pre, account_to_init_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&owner_npk, &owner_vpk),
                ssk: shared_secret,
                nsk: owner_nsk,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![Ids::token_definition()], vec![], output).unwrap();

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    let expected_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 0,
        }),
        nonce: Nonce::private_account_nonce_init(&owner_id),
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &expected_account))
        .is_some());
}

// TODO: think this is unnecessary; double check.
/// Confirms `InitializeAccount` is self-service-only: unlike `Transfer`/`Mint`, whose recipient
/// host logic never asserts `is_authorized`, the guest's `#[account(init, signer)]` on
/// `account_to_initialize` requires `is_authorized == true` — enforced by the SPEL macro's own
/// account validation before `token_program::initialize::initialize_account`'s host logic
/// (which carries the same assert as defense in depth) ever runs. The only private identity
/// variant satisfying that for a fresh account is `PrivateAuthorizedInit`, which requires
/// supplying `nsk` directly — so a third party cannot initialize a private holding on behalf of
/// an `(npk, vpk, identifier)` whose `nsk` they don't possess. Attempting it via
/// `PrivateUnauthorized` (the variant that *would* allow third-party setup elsewhere) is
/// rejected at the framework's signer check, since that variant forces `is_authorized: false`.
#[test]
fn token_initialize_private_account_without_nsk_is_not_expressible() {
    let state = state_for_token_tests_without_recipient();

    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let account_to_init_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let result = execute_and_prove(
        vec![definition_pre, account_to_init_pre],
        Program::serialize_instruction(token_core::Instruction::InitializeAccount).unwrap(),
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
    );

    let err = result.expect_err(
        "initializing a private holding without its nsk must be rejected: InitializeAccount \
         requires is_authorized == true, but PrivateUnauthorized forces is_authorized == false",
    );
    let message = format!("{err:?}");
    assert!(
        message.contains("must be a signer"),
        "expected the self-service-only rejection, got a different error: {message}"
    );
}

/// Two independent parties share control of one private Token holding via a `GroupKeyHolder`
/// Group Master Secret (GMS), distributed through the real seal/unseal handshake — not by
/// reusing key material directly — so the test proves actual sharing, not code reuse. "Alice"
/// creates the group and shields tokens into the shared holding; "Bob" only ever receives the
/// *sealed* GMS, independently re-derives the identical nsk/npk from it, and successfully
/// burns from the same holding neither of them personally owns. Validates the `GROUP` Q2
/// dimension: sharing a private account (group-owned) used as a program account.
/// TODO: add a function for spending
#[test]
fn token_group_owned_holding_shared_control_burn() {
    let mut state = state_for_token_tests();
    let shield_amount = 500_000_u128;
    let burn_amount = 200_000_u128;

    // Alice creates the group and derives the shared account's keys.
    let alice_holder = GroupKeyHolder::new();
    let derivation_seed = [7_u8; 32];
    let alice_keys = alice_holder.derive_keys_for_shared_account(&derivation_seed);
    let group_npk = alice_keys.generate_nullifier_public_key();
    let group_vpk = alice_keys.generate_viewing_public_key();
    let group_id = AccountId::for_regular_private_account(&group_npk, 0);

    // Alice distributes the GMS to Bob via the real seal/unseal handshake, not by handing
    // over key material directly.
    let bob_sealing_keys = SecretSpendingKey([9_u8; 32]).produce_private_key_holder(None);
    let bob_sealing_vpk = bob_sealing_keys.generate_viewing_public_key();
    let bob_sealing_vsk = bob_sealing_keys.viewing_secret_key;
    let sealed_gms = alice_holder.seal_for(&SealingPublicKey::from_bytes(
        bob_sealing_vpk.to_bytes().to_vec(),
    ));
    let bob_holder =
        GroupKeyHolder::unseal(&sealed_gms, &bob_sealing_vsk).expect("Bob must unseal the GMS");

    // Bob independently re-derives the same shared-account keys from the unsealed GMS.
    let bob_keys = bob_holder.derive_keys_for_shared_account(&derivation_seed);
    let bob_nsk = bob_keys.nullifier_secret_key;
    assert_eq!(
        bob_keys.generate_nullifier_public_key(),
        group_npk,
        "Bob must derive the identical npk as Alice from the shared GMS"
    );

    // Alice shields tokens into the group-owned holding (mirrors `shielded_token_transfer`,
    // parameterized by the group's npk/vpk instead of a personal one).
    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;
    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);
    let group_pre_shield = AccountWithMetadata::new(Account::default(), false, group_id);

    let shield_secret = SharedSecretKey::encapsulate_deterministic(&group_vpk, &[0u8; 32], 0).0;
    let shield_instruction = token_core::Instruction::Transfer {
        amount_to_transfer: shield_amount,
    };
    let (shield_output, shield_proof) = execute_and_prove(
        vec![sender_pre, group_pre_shield],
        Program::serialize_instruction(shield_instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateUnauthorized {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&group_npk, &group_vpk),
                npk: group_npk,
                ssk: shield_secret,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();
    let shield_message =
        Message::try_from_circuit_output(vec![sender_id], vec![sender_nonce], shield_output)
            .unwrap();
    let shield_witness =
        WitnessSet::for_message(&shield_message, shield_proof, &[&Keys::holder_key()]);
    let shield_tx = PrivacyPreservingTransaction::new(shield_message, shield_witness);
    state
        .transition_from_privacy_preserving_transaction(&shield_tx, 0, 0)
        .unwrap();

    let group_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount,
        }),
        nonce: Nonce::private_account_nonce_init(&group_id),
    };
    let group_commitment = Commitment::new(&group_id, &group_account);
    assert!(state.get_proof_for_commitment(&group_commitment).is_some());

    // Bob — who never touched Alice's `GroupKeyHolder` object, only the sealed GMS — burns
    // from the group-owned holding using his independently derived nsk.
    let membership_proof = state
        .get_proof_for_commitment(&group_commitment)
        .expect("group holding's commitment must be in the set");
    let burn_shared_secret =
        SharedSecretKey::encapsulate_deterministic(&group_vpk, &[0u8; 32], 0).0;

    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let group_pre_burn = AccountWithMetadata::new(group_account, true, group_id);

    let burn_instruction = token_core::Instruction::Burn {
        amount_to_burn: burn_amount,
    };
    let (burn_output, burn_proof) = execute_and_prove(
        vec![definition_pre, group_pre_burn],
        Program::serialize_instruction(burn_instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivateAuthorizedUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&group_npk, &group_vpk),
                ssk: burn_shared_secret,
                nsk: bob_nsk,
                membership_proof,
                identifier: 0,
            },
        ],
        &token_program().into(),
    )
    .unwrap();

    let burn_message =
        Message::try_from_circuit_output(vec![Ids::token_definition()], vec![], burn_output)
            .unwrap();
    let burn_witness = WitnessSet::for_message(&burn_message, burn_proof, &[]);
    let burn_tx = PrivacyPreservingTransaction::new(burn_message, burn_witness);
    state
        .transition_from_privacy_preserving_transaction(&burn_tx, 0, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: Some(Ids::token_definition()),
            }),
            nonce: Nonce(0),
        }
    );

    let group_nonce_after =
        Nonce::private_account_nonce_init(&group_id).private_account_nonce_increment(&bob_nsk);
    let new_group_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount - burn_amount,
        }),
        nonce: group_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&group_id, &new_group_account))
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
