use integration_tests::{
    private_authorized_init_identity, private_authorized_update_identity,
    private_unauthorized_identity, GroupOwner,
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
    program::PdaSeed,
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
        Self::token_holding(1_000_000_u128, Nonce(0))
    }

    fn recipient_init() -> Account {
        Self::token_holding(0_u128, Nonce(0))
    }

    fn authority_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::default(),
            nonce: Nonce(0),
        }
    }

    /// A token holding account for the canonical `Ids::token_definition()`, at the given
    /// balance and nonce. Covers every private and public token-holding shape in this file —
    /// the `program_owner`/`definition_id` are fixed for this test module.
    fn token_holding(balance: u128, nonce: Nonce) -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance,
            }),
            nonce,
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
        Accounts::token_holding(1_000_000_u128, Nonce(1))
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
        Accounts::token_holding(0_u128, Nonce(1))
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
        Accounts::token_holding(500_000_u128, Nonce(1))
    );

    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Accounts::token_holding(500_000_u128, Nonce(0))
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
        Accounts::token_holding(500_000_u128, Nonce(1))
    );

    assert_eq!(
        state.get_account_by_id(Ids::recipient()),
        Accounts::token_holding(500_000_u128, Nonce(1))
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
        Accounts::token_holding(800_000_u128, Nonce(1))
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
        Accounts::token_holding(1_500_000_u128, Nonce(0))
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
        Accounts::token_holding(500_000_u128, Nonce(1))
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

/// TODO
/// EXPERIMENTAL — investigating whether `PrivatePdaInit`'s `seed: Some((seed,
/// authority_program_id))` external-derivation-check path lets a private-PDA account be used as an
/// input to an *existing* program's flow (Token) without any chained call, `Claim::Pda`, or
/// awareness from the `authority_program_id` itself. Per `lee_core`'s
/// `circuit_io.rs`/`execution_state.rs`, this path binds the position purely via
/// `AccountId::for_private_pda(authority_program_id, seed, npk, identifier) ==
/// pre_state.account_id`, checked directly against the top-level `account_identities` — no chained
/// call needed. Using `Ids::token_program()` as the `authority_program_id` here, but per the
/// circuit source this is not required to correspond to anything Token itself is aware of; it's
/// purely a hash input.
#[test]
fn token_shield_into_private_pda_via_external_seed() {
    let mut state = state_for_token_tests();
    let amount = 500_000_u128;

    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;
    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);

    let authority_program_id = Ids::token_program();
    let pda_seed = PdaSeed::new([77u8; 32]);
    let recipient_nsk: NullifierSecretKey = [123u8; 32];
    let recipient_npk = NullifierPublicKey::from(&recipient_nsk);
    let recipient_vpk = ViewingPublicKey::from_seed(&[124u8; 32], &[125u8; 32]);
    let recipient_id =
        AccountId::for_private_pda(&authority_program_id, &pda_seed, &recipient_npk, 0);

    let recipient_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&recipient_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivatePdaInit {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&recipient_npk, &recipient_vpk),
                npk: recipient_npk,
                ssk: shared_secret,
                identifier: 0,
                seed: Some((pda_seed, authority_program_id)),
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

    let recipient_account =
        Accounts::token_holding(amount, Nonce::private_account_nonce_init(&recipient_id));
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
        .is_some());
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

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender, recipient],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_unauthorized_identity(recipient_npk, &recipient_vpk, 0),
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

    Accounts::token_holding(amount, Nonce::private_account_nonce_init(&recipient_id))
}

#[test]
fn token_shielded_transfer() {
    let mut state = state_for_token_tests();
    let amount = 500_000_u128;

    let recipient_account = shielded_token_transfer(amount, &mut state);

    assert_eq!(
        state.get_account_by_id(Ids::holder()),
        Accounts::token_holding(1_000_000 - amount, Nonce(1))
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
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);
    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_authorized_init_identity(recipient_nsk, &recipient_vpk, 0),
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
        Accounts::token_holding(1_000_000 - amount, Nonce(1))
    );

    let recipient_account =
        Accounts::token_holding(amount, Nonce::private_account_nonce_init(&recipient_id));
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

    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, sender_id);
    let new_recipient_pre = AccountWithMetadata::new(Account::default(), false, new_recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, new_recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            // Distinct `output_index` per private output keeps the encapsulated secrets
            // reproducible.
            private_authorized_update_identity(sender_nsk, &sender_vpk, membership_proof, 0),
            private_unauthorized_identity(new_recipient_npk, &new_recipient_vpk, 1),
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
    let new_sender_account =
        Accounts::token_holding(shielded_amount - transfer_amount, sender_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&sender_id, &new_sender_account))
        .is_some());

    let new_recipient_account = Accounts::token_holding(
        transfer_amount,
        Nonce::private_account_nonce_init(&new_recipient_id),
    );
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
    let sender_nsk = PrivateKeys::recipient_nsk();
    let sender_vpk = PrivateKeys::recipient_vpk();
    let sender_id = PrivateKeys::recipient_id();

    let public_recipient_id = Ids::recipient();
    let sender_commitment = Commitment::new(&sender_id, &sender_account);
    let membership_proof = state
        .get_proof_for_commitment(&sender_commitment)
        .expect("sender's commitment must be in the set");

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
            private_authorized_update_identity(sender_nsk, &sender_vpk, membership_proof, 0),
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
        Accounts::token_holding(deshield_amount, Nonce(0))
    );

    let sender_nonce_after =
        Nonce::private_account_nonce_init(&sender_id).private_account_nonce_increment(&sender_nsk);
    let new_sender_account =
        Accounts::token_holding(shielded_amount - deshield_amount, sender_nonce_after);
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

    let instruction = token_core::Instruction::Mint { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_unauthorized_identity(recipient_npk, &recipient_vpk, 0),
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

    let recipient_account = Accounts::token_holding(
        amount_to_mint,
        Nonce::private_account_nonce_init(&recipient_id),
    );
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
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);

    let instruction = token_core::Instruction::Mint { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_authorized_init_identity(recipient_nsk, &recipient_vpk, 0),
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

    let recipient_account = Accounts::token_holding(
        amount_to_mint,
        Nonce::private_account_nonce_init(&recipient_id),
    );
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
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let recipient_pre = Accounts::token_holding(
        pre_balance,
        Nonce::private_account_nonce_init(&recipient_id),
    );
    let recipient_commitment = Commitment::new(&recipient_id, &recipient_pre);
    state = state.with_private_accounts([(
        recipient_commitment.clone(),
        Nullifier::for_account_initialization(&recipient_id),
    )]);

    let membership_proof = state
        .get_proof_for_commitment(&recipient_commitment)
        .expect("seeded recipient's commitment must be in the set");

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let existing_recipient_pre =
        AccountWithMetadata::new(recipient_pre.clone(), true, recipient_id);

    let (output, second_proof) = execute_and_prove(
        vec![definition_pre, existing_recipient_pre],
        Program::serialize_instruction(token_core::Instruction::Mint { amount_to_mint }).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_authorized_update_identity(recipient_nsk, &recipient_vpk, membership_proof, 0),
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
    let witness = WitnessSet::for_message(&message, second_proof, &[&Keys::def_key()]);
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
    let recipient_after_second_mint =
        Accounts::token_holding(pre_balance + amount_to_mint, recipient_nonce_after);
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

    let holder_nsk = PrivateKeys::recipient_nsk();
    let holder_vpk = PrivateKeys::recipient_vpk();
    let holder_id = PrivateKeys::recipient_id();

    // Predefined holding account to burn from.
    let holder_account = Accounts::token_holding(
        holding_balance,
        Nonce::private_account_nonce_init(&holder_id),
    );
    let holder_commitment = Commitment::new(&holder_id, &holder_account);
    state = state.with_private_accounts([(
        holder_commitment.clone(),
        Nullifier::for_account_initialization(&holder_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&holder_commitment)
        .expect("holder's commitment must be in the set");

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
            private_authorized_update_identity(holder_nsk, &holder_vpk, membership_proof, 0),
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

    let new_holder_account = Accounts::token_holding(
        holding_balance - burn_amount,
        Nonce::private_account_nonce_init(&holder_id).private_account_nonce_increment(&holder_nsk),
    );
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

    let recipient_nsk = PrivateKeys::recipient_nsk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    let recipient_account = Accounts::token_holding(
        init_balance,
        Nonce::private_account_nonce_init(&recipient_id),
    );
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
            private_authorized_update_identity(recipient_nsk, &recipient_vpk, membership_proof, 0),
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
        // `first_amount` was seeded directly into the recipient, never debited from the
        // sender — only the real transfer (`second_amount`) actually happened.
        Accounts::token_holding(1_000_000 - second_amount, Nonce(1))
    );

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let new_recipient_account =
        Accounts::token_holding(init_balance + second_amount, recipient_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &new_recipient_account))
        .is_some());
}

/// Private Token transfer into a pre-existing Token holding account. This requires
/// the account's `nsk`; `PrivateAuthorizedUpdate`.
#[test]
fn token_private_transfer_into_existing_private_holding() {
    let mut state = state_for_token_tests();
    let sender_initial_balance = 500_000_u128;
    let recipient_initial_balance = 300_000_u128;
    let transfer_amount = 200_000_u128;

    let sender_nsk = PrivateKeys::recipient_nsk();
    let sender_vpk = PrivateKeys::recipient_vpk();
    let sender_id = PrivateKeys::recipient_id();

    let recipient_nsk = PrivateKeys::holder_nsk();
    let recipient_vpk = PrivateKeys::holder_vpk();
    let recipient_id = PrivateKeys::holder_id();

    // Seed both sides directly — neither needs a real prior transaction to exist.
    let sender_account = Accounts::token_holding(
        sender_initial_balance,
        Nonce::private_account_nonce_init(&sender_id),
    );
    let recipient_account = Accounts::token_holding(
        recipient_initial_balance,
        Nonce::private_account_nonce_init(&recipient_id),
    );
    let sender_commitment = Commitment::new(&sender_id, &sender_account);
    let recipient_commitment = Commitment::new(&recipient_id, &recipient_account);
    state = state.with_private_accounts([
        (
            sender_commitment.clone(),
            Nullifier::for_account_initialization(&sender_id),
        ),
        (
            recipient_commitment.clone(),
            Nullifier::for_account_initialization(&recipient_id),
        ),
    ]);

    let sender_membership_proof = state
        .get_proof_for_commitment(&sender_commitment)
        .expect("sender's commitment must be in the set");
    let recipient_membership_proof = state
        .get_proof_for_commitment(&recipient_commitment)
        .expect("recipient's commitment must be in the set");

    let sender_pre = AccountWithMetadata::new(sender_account.clone(), true, sender_id);
    let recipient_pre = AccountWithMetadata::new(recipient_account.clone(), true, recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![sender_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_authorized_update_identity(sender_nsk, &sender_vpk, sender_membership_proof, 0),
            private_authorized_update_identity(
                recipient_nsk,
                &recipient_vpk,
                recipient_membership_proof,
                1,
            ),
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
    let new_sender_account =
        Accounts::token_holding(sender_initial_balance - transfer_amount, sender_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&sender_id, &new_sender_account))
        .is_some());

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let new_recipient_account = Accounts::token_holding(
        recipient_initial_balance + transfer_amount,
        recipient_nonce_after,
    );
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
    let owner_vpk = PrivateKeys::recipient_vpk();
    let owner_id = PrivateKeys::recipient_id();

    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let account_to_init_pre = AccountWithMetadata::new(Account::default(), true, owner_id);

    let instruction = token_core::Instruction::InitializeAccount;
    let (output, proof) = execute_and_prove(
        vec![definition_pre, account_to_init_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_authorized_init_identity(owner_nsk, &owner_vpk, 0),
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

    let expected_account = Accounts::token_holding(0, Nonce::private_account_nonce_init(&owner_id));
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &expected_account))
        .is_some());
}

/// Confirms that `InitializeAccount` cannot be performed without private account's `nsk`.
/// E.g., account must be `PrivateAuthorizedInit` and not `PrivateUnauthorized`.
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

    let result = execute_and_prove(
        vec![definition_pre, account_to_init_pre],
        Program::serialize_instruction(token_core::Instruction::InitializeAccount).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_unauthorized_identity(recipient_npk, &recipient_vpk, 0),
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

/// Two independent parties (Alice and Bob) control a private Token holding (via `GroupKeyHolder`).
/// Alice initializes the private Token account, and Bob burns tokens from the shared account.
#[test]
fn token_group_owned_holding_shared_control_burn() {
    let mut state = state_for_token_tests();
    let shield_amount = 500_000_u128;
    let burn_amount = 200_000_u128;

    // Alice creates the group and derives the shared account's keys; Bob is admitted via the
    // real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_nsk = alice.admit_member();
    let group_npk = alice.npk;
    let group_vpk = alice.vpk;
    let group_id = alice.id;

    // Alice shields tokens into the group-owned holding (mirrors `shielded_token_transfer`,
    // parameterized by the group's npk/vpk instead of a personal one).
    let sender_id = Ids::holder();
    let sender_account = state.get_account_by_id(sender_id);
    let sender_nonce = sender_account.nonce;
    let sender_pre = AccountWithMetadata::new(sender_account, true, sender_id);
    let group_pre_shield = AccountWithMetadata::new(Account::default(), false, group_id);

    let shield_instruction = token_core::Instruction::Transfer {
        amount_to_transfer: shield_amount,
    };
    let (shield_output, shield_proof) = execute_and_prove(
        vec![sender_pre, group_pre_shield],
        Program::serialize_instruction(shield_instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_unauthorized_identity(group_npk, &group_vpk, 0),
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

    let group_account =
        Accounts::token_holding(shield_amount, Nonce::private_account_nonce_init(&group_id));
    let group_commitment = Commitment::new(&group_id, &group_account);
    assert!(state.get_proof_for_commitment(&group_commitment).is_some());

    // Bob — who never touched Alice's `GroupKeyHolder` object, only the sealed GMS — burns
    // from the group-owned holding using his independently derived nsk.
    let membership_proof = state
        .get_proof_for_commitment(&group_commitment)
        .expect("group holding's commitment must be in the set");

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
            private_authorized_update_identity(bob_nsk, &group_vpk, membership_proof, 0),
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
    let new_group_account = Accounts::token_holding(shield_amount - burn_amount, group_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&group_id, &new_group_account))
        .is_some());
}

/// Two independent parties (Alice and Bob) control a private Token holding (via `GroupKeyHolder`).
/// Alice initializes the private Token account, and Bob transfers tokens from the shared account.
#[test]
fn token_group_owned_holding_shared_control_transfer() {
    let mut state = state_for_token_tests();
    let shield_amount = 500_000_u128;
    let transfer_amount = 200_000_u128;

    // Alice creates the group and derives the shared account's keys; Bob is admitted via the
    // real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_nsk = alice.admit_member();
    let group_vpk = alice.vpk;
    let group_id = alice.id;

    let group_account =
        Accounts::token_holding(shield_amount, Nonce::private_account_nonce_init(&group_id));
    let group_commitment = Commitment::new(&group_id, &group_account);
    state = state.with_private_accounts([(
        group_commitment.clone(),
        Nullifier::for_account_initialization(&group_id),
    )]);

    // Bob spends via Transfer — not Burn — sending to a fresh private recipient.
    let membership_proof = state
        .get_proof_for_commitment(&group_commitment)
        .expect("group holding's commitment must be in the set");

    let recipient_npk = PrivateKeys::holder_npk();
    let recipient_vpk = PrivateKeys::holder_vpk();
    let recipient_id = PrivateKeys::holder_id();

    let group_pre = AccountWithMetadata::new(group_account, true, group_id);
    let recipient_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);

    let instruction = token_core::Instruction::Transfer {
        amount_to_transfer: transfer_amount,
    };
    let (output, proof) = execute_and_prove(
        vec![group_pre, recipient_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_authorized_update_identity(bob_nsk, &group_vpk, membership_proof, 0),
            private_unauthorized_identity(recipient_npk, &recipient_vpk, 1),
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

    let group_nonce_after =
        Nonce::private_account_nonce_init(&group_id).private_account_nonce_increment(&bob_nsk);
    let new_group_account =
        Accounts::token_holding(shield_amount - transfer_amount, group_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&group_id, &new_group_account))
        .is_some());

    let new_recipient_account = Accounts::token_holding(
        transfer_amount,
        Nonce::private_account_nonce_init(&recipient_id),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &new_recipient_account))
        .is_some());
}

/// Two independent parties (Alice and Bob) control a private Token holding (via `GroupKeyHolder`).
/// Alice initializes the private Token account (`InitializeAccount` with `PrivateAuthorizedInit`)/
#[test]
fn token_group_owned_holding_shared_control_initialize() {
    let mut state = state_for_token_tests_without_recipient();

    // Alice creates the group and derives the shared account's keys; Bob is admitted via the
    // real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let group_id = alice.id;
    let bob_nsk = alice.admit_member();

    // Bob — who never created the group — self-initializes the shared holding directly.
    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let group_pre = AccountWithMetadata::new(Account::default(), true, group_id);

    let instruction = token_core::Instruction::InitializeAccount;
    let (output, proof) = execute_and_prove(
        vec![definition_pre, group_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_authorized_init_identity(bob_nsk, &alice.vpk, 0),
        ],
        &token_program().into(),
    )
    .unwrap();

    let message =
        Message::try_from_circuit_output(vec![Ids::token_definition()], vec![], output).unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let expected_account = Accounts::token_holding(0, Nonce::private_account_nonce_init(&group_id));
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&group_id, &expected_account))
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
        Accounts::token_holding(1_500_000_u128, Nonce(0))
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

#[test]
fn token_mint_with_authority_to_private_holding() {
    let mut state = V03State::new();
    deploy_token(&mut state);

    let authority_key: [u8; 32] = Ids::authority()
        .as_ref()
        .try_into()
        .expect("AccountId is always 32 bytes");

    // Create the definition with an external mint authority from the start — the rotation
    // dance itself is already covered by `token_rotate_authority_then_new_authority_can_mint`.
    let instruction = token_core::Instruction::NewFungibleDefinition {
        name: String::from("Gold"),
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

    state.force_insert_account(Ids::authority(), Accounts::authority_init());

    let amount_to_mint = 500_000_u128;
    let recipient_npk = PrivateKeys::recipient_npk();
    let recipient_vpk = PrivateKeys::recipient_vpk();
    let recipient_id = PrivateKeys::recipient_id();

    // Definition is `#[account(mut)]` only under external authority — it does not itself
    // authorize the mint, so it goes in as an ordinary (unauthorized) public account.
    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::token_definition()),
        false,
        Ids::token_definition(),
    );
    let recipient_pre = AccountWithMetadata::new(Account::default(), false, recipient_id);
    let authority_account = state.get_account_by_id(Ids::authority());
    let authority_nonce = authority_account.nonce;
    let authority_pre = AccountWithMetadata::new(authority_account, true, Ids::authority());

    let instruction = token_core::Instruction::MintWithAuthority { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, recipient_pre, authority_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            private_unauthorized_identity(recipient_npk, &recipient_vpk, 0),
            InputAccountIdentity::Public,
        ],
        &token_program().into(),
    )
    .unwrap();

    // `public_account_ids` carries every public account for post-state zipping (definition,
    // then authority — their `execute_and_prove` input order); `nonces` carries only the
    // signer(s), positionally matched to the witness keys below (just `authority` here).
    let message = Message::try_from_circuit_output(
        vec![Ids::token_definition(), Ids::authority()],
        vec![authority_nonce],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::authority_key()]);
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
                authority: Some(AccountId::new(authority_key)),
            }),
            nonce: Nonce(1),
        }
    );

    let recipient_account = Accounts::token_holding(
        amount_to_mint,
        Nonce::private_account_nonce_init(&recipient_id),
    );
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_account))
        .is_some());
}

/// TODO
/// EXPERIMENTAL — follow-up to `token_shield_into_private_pda_via_external_seed`: proves the
/// *update* half of the same mechanism (crediting an *existing* private PDA, not just creating
/// one), completing a genuine round trip rather than a one-shot creation. `PrivatePdaUpdate`'s
/// external seed path has a different pre-condition than `Init`: `execution_state.rs` asserts
/// `pre_state.is_authorized ^ external_seed.is_some()` — with an external seed supplied, the
/// pre-state must be *unauthorized*, even though we're touching it with a real `nsk` +
/// `membership_proof`. That's incompatible with `Transfer`'s sender role, which requires a
/// framework-level `#[account(signer)]` (`is_authorized: true`) — confirmed empirically: using
/// the private-PDA holder as `Transfer`'s sender fails at the SPEL macro's own validation
/// ("must be a signer"), before Token's own logic is ever reached. `Mint`'s
/// `user_holding_account` has no such requirement (`mint_inner` never asserts `is_authorized` on
/// it, crediting an existing holding or not), so it's used here instead — mirroring the `EXIST`
/// dimension's existing-account-crediting pattern (`token_mint_into_existing_private_holding`),
/// just with a private-PDA holder instead of a regular private account.
#[test]
fn token_mint_into_existing_private_pda_via_external_seed() {
    let mut state = state_for_token_tests_without_recipient();
    let holding_balance = 500_000_u128;
    let amount_to_mint = 200_000_u128;

    let authority_program_id = Ids::token_program();
    let pda_seed = PdaSeed::new([88u8; 32]);
    let holder_nsk: NullifierSecretKey = [131u8; 32];
    let holder_npk = NullifierPublicKey::from(&holder_nsk);
    let holder_vpk = ViewingPublicKey::from_seed(&[132u8; 32], &[133u8; 32]);
    let holder_id = AccountId::for_private_pda(&authority_program_id, &pda_seed, &holder_npk, 0);

    // Seed the private-PDA holding directly (established technique — no real transaction
    // needed). Its eligibility as a private PDA is re-derived independently by the update-side
    // check below; nothing about how it was seeded matters to that check.
    let holder_account = Accounts::token_holding(
        holding_balance,
        Nonce::private_account_nonce_init(&holder_id),
    );
    let holder_commitment = Commitment::new(&holder_id, &holder_account);
    state = state.with_private_accounts([(
        holder_commitment.clone(),
        Nullifier::for_account_initialization(&holder_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&holder_commitment)
        .expect("seeded holder's commitment must be in the set");

    let definition_account = state.get_account_by_id(Ids::token_definition());
    let definition_nonce = definition_account.nonce;
    let definition_pre =
        AccountWithMetadata::new(definition_account, true, Ids::token_definition());
    let holder_pre = AccountWithMetadata::new(holder_account, false, holder_id);

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&holder_vpk, &[0u8; 32], 0).0;

    let instruction = token_core::Instruction::Mint { amount_to_mint };
    let (output, proof) = execute_and_prove(
        vec![definition_pre, holder_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::PrivatePdaUpdate {
                epk: EphemeralPublicKey(Vec::new()),
                view_tag: EncryptedAccountData::compute_view_tag(&holder_npk, &holder_vpk),
                ssk: shared_secret,
                nsk: holder_nsk,
                membership_proof,
                identifier: 0,
                seed: Some((pda_seed, authority_program_id)),
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

    let holder_nonce_after =
        Nonce::private_account_nonce_init(&holder_id).private_account_nonce_increment(&holder_nsk);
    let new_holder_account =
        Accounts::token_holding(holding_balance + amount_to_mint, holder_nonce_after);
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&holder_id, &new_holder_account))
        .is_some());
}
