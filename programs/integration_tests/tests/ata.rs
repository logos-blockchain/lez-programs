use std::collections::HashMap;

use ata_core::{compute_ata_seed, get_associated_token_account_id};
use key_protocol::key_management::{
    group_key_holder::{GroupKeyHolder, SealingPublicKey},
    secret_holders::SecretSpendingKey,
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

// Marvin-todo
/// Documents a confirmed protocol gap (`PDA` Q2 dimension): the ATA holding can never be made
/// a private account as ATA is currently coded. `Create`'s `ChainedCall.pda_seeds` authorizes
/// Token to mutate `for_public_pda(ata_program_id, seed)` — a *public*-form PDA match. Per
/// `resolve_authorization_and_record_bindings` in `lee_core`'s `execution_state.rs`, a
/// caller-seed match only gets recorded in `private_pda_bound_positions` when it matches under
/// `for_private_pda` (`is_private_form == true`); a public-form match authorizes the account
/// but never binds it as a private PDA. Since `PrivatePdaInit`/`PrivatePdaUpdate` require their
/// position to appear in that binding map (`execution_state.rs:211`), and ATA's own
/// `verify_ata_and_get_seed` independently requires the account id to equal
/// `for_public_pda(ata_program_id, seed)` (never `for_private_pda`'s output, by construction),
/// these two requirements can never both hold for the same account_id. This is not
/// program-specific friction — it's structural: fixing it would require `ata_core` (and
/// equally amm_core / stablecoin_core) to derive their PDAs via `for_private_pda` instead,
/// which is a source change to the program, not a test workaround.
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

// Marvin-todo
/// Credits an *already-existing* private holding through ATA's chained call to Token, and
/// documents a structural finding along the way:
/// `ata_program::transfer::transfer_from_associated_token_account` hard-asserts `recipient.account
/// != Account::default()` ("Recipient token holding must be initialized"), so a *fresh* private
/// recipient (shield-style, `PrivateUnauthorized`) can never be created through `ATA::Transfer` —
/// only an existing account can be credited. That collapses what would otherwise be separate `BASE`
/// and `EXIST` tests into one: this test necessarily exercises both "private account through a
/// chained call" (`CHAIN`) and "sending to an existing private account" (`EXIST`, requiring the
/// recipient's cooperation via `PrivateAuthorizedUpdate`, per the finding already confirmed in
/// `token.rs`).
///
/// The private holding is funded beforehand via a direct (non-ATA) `Token::Transfer` shield
/// from a throwaway public holder, since neither `ATA::Transfer` (blocked by the assert above)
/// nor `Token::Mint` (this test fixture's definition has `authority: None`, fixed supply) can
/// create it.
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

// Marvin-todo
/// Tests a previously-untried combination: `Burn`'s guest requires `owner` to be a *signer*
/// (`#[account(signer)]`) — every existing private-owner test so far
/// (`ata_create_from_private_owner`) only used owner as a passive `PrivateUnauthorized` recipient
/// in `Create`, which doesn't need signer authorization at all. Here, owner self-initializes *and*
/// signs in the same transaction via `PrivateAuthorizedInit` (proving control by supplying their
/// own nsk directly) — the ATA holding itself stays public, per the confirmed `PDA` finding above;
/// only the signing identity is private.
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

// Marvin-todo
/// Composes the `GROUP` dimension with the signer-authorization finding just proven above: a
/// group-owned owner (GMS distributed through the real seal/unseal handshake, exactly as in
/// `token_group_owned_holding_shared_control`) signs an `ATA::Burn` via `PrivateAuthorizedInit`.
/// "Bob" — who only ever receives the sealed GMS, never Alice's `GroupKeyHolder` object —
/// independently re-derives the identical nsk/npk and successfully signs for the shared ATA
/// owner identity.
#[test]
fn ata_group_owned_owner_signing() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    // Alice creates the group and derives the shared owner identity's keys.
    let alice_holder = GroupKeyHolder::new();
    let derivation_seed = [13_u8; 32];
    let alice_keys = alice_holder.derive_keys_for_shared_account(&derivation_seed);
    let owner_npk = alice_keys.generate_nullifier_public_key();
    let owner_id = AccountId::for_regular_private_account(&owner_npk, 0);

    // Alice distributes the GMS to Bob via the real seal/unseal handshake.
    let bob_sealing_keys = SecretSpendingKey([17_u8; 32]).produce_private_key_holder(None);
    let bob_sealing_vpk = bob_sealing_keys.generate_viewing_public_key();
    let bob_sealing_vsk = bob_sealing_keys.viewing_secret_key;
    let sealed_gms = alice_holder.seal_for(&SealingPublicKey::from_bytes(
        bob_sealing_vpk.to_bytes().to_vec(),
    ));
    let bob_holder =
        GroupKeyHolder::unseal(&sealed_gms, &bob_sealing_vsk).expect("Bob must unseal the GMS");

    // Bob independently re-derives the same shared owner keys and is the one who signs below.
    let bob_keys = bob_holder.derive_keys_for_shared_account(&derivation_seed);
    let bob_nsk = bob_keys.nullifier_secret_key;
    let bob_vpk = bob_keys.generate_viewing_public_key();
    assert_eq!(
        bob_keys.generate_nullifier_public_key(),
        owner_npk,
        "Bob must derive the identical npk as Alice from the shared GMS"
    );

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

    let shared_secret = SharedSecretKey::encapsulate_deterministic(&bob_vpk, &[0u8; 32], 0).0;

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
                view_tag: EncryptedAccountData::compute_view_tag(&owner_npk, &bob_vpk),
                ssk: shared_secret,
                nsk: bob_nsk,
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

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}
