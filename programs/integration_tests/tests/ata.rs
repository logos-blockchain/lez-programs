use std::collections::HashMap;

use ata_core::{compute_ata_seed, get_associated_token_account_id, AtaContents};
use integration_tests::{
    private_authorized_init_identity, private_authorized_update_identity,
    private_foreign_init_identity, GroupOwner,
};
use lee::{
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::{ProgramWithDependencies, ProvingInput},
        Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, Nonce, ProgramShardSelector, ShardData},
    encryption::ViewingPublicKey,
    AuthorizationSecretKey, Commitment, Identifier, NullifierPublicKey, NullifierSecretKey,
    NullifierWitness, PrivateWitness, WitnessKind,
};
use token_core::{TokenDefinition, TokenDescriptor, TokenHolding, TokenKind};

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
    /// The program's account id: since v0.2.5 a program is addressed by its deployed
    /// `ProgramHeader` account, and the test harness seeds that header at the ImageID
    /// bijection address, so `AccountId::from(<ELF>_ID)` is where it lives here.
    fn token_program() -> AccountId {
        AccountId::from_builtin_program(token_methods::TOKEN_ID)
    }

    /// The program's account id: since v0.2.5 a program is addressed by its deployed
    /// `ProgramHeader` account, and the test harness seeds that header at the ImageID
    /// bijection address, so `AccountId::from(<ELF>_ID)` is where it lives here.
    fn ata_program() -> AccountId {
        AccountId::from_builtin_program(ata_methods::ATA_ID)
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: None,
            }),
        )
    }

    fn owner_ata_init() -> Account {
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
        )
    }

    fn recipient_ata_init() -> Account {
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
        )
    }

    fn foreign_owned_token_definition() -> Account {
        Account::default().with_shard(
            AccountId::new([99u8; 32]),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Foreign Gold"),
                total_supply: 1_000_000_u128,
                metadata_id: None,
                authority: None,
            }),
        )
    }
}

// v0.2.5 deleted `ProgramDeploymentTransaction`; deployment is now the `program_loader`
// pseudo-program's WriteSegment/CreateHeader flow. `with_programs` seeds each program in the
// shape that flow produces — a loader-owned header plus its segment — without making every
// test drive a deployment. The header lands at `AccountId::from(program.id())`, so a program's
// account id stays derivable from its ELF here.
fn deploy_programs(state: &mut V03State) {
    *state = std::mem::take(state).with_programs([
        Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF"),
        Program::new(ata_methods::ATA_ELF.to_vec().into()).expect("valid ata ELF"),
    ]);
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
        kind: TokenKind::Fungible,
        contents: AtaContents::Empty,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
        )
    );
}

#[test]
fn ata_create_is_idempotent() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
        kind: TokenKind::Fungible,
        contents: AtaContents::Intended,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
        ],
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
        )
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
        kind: TokenKind::Fungible,
        contents: AtaContents::Empty,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
        ],
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

// `ata_create_rejects_existing_ata_owned_by_unexpected_token_program` has no
// v0.3.0 successor. It seeded a holding "owned by" an unrelated program and
// asserted Create refused. Ownership is a written shard now: those bytes live
// under the other program's key, so the Token Program's shard at that address is
// empty and Create legitimately succeeds as a fresh initialization. There is no
// program-level check left to exercise — which program's shard a slot refers to
// is named by the transaction's `ProgramShardSelector` and asserted by SPEL.
// `programs/ata/src/tests.rs` retired six unit tests of exactly this kind.

#[test]
fn ata_create_replaces_a_squatted_ata_when_the_owner_authorizes() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    // Someone else's holding for a different definition sits at the ATA address.
    let mut squatted_ata = Accounts::owner_ata_init();
    squatted_ata.data.set_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::recipient(),
            balance: 1_000_000_u128,
        }),
    );
    state.force_insert_account(Ids::owner_ata(), squatted_ata);

    // v0.2.5 rejected this outright. v0.3.0 treats it as the one value-destroying
    // branch and allows it on the owner's authorization instead (`create.rs`:
    // "Replacing someone else's bytes at this address is the one branch that
    // destroys value, so it needs the owner to say so"). The refusal without the
    // owner, and the guard against claiming `Intended` over a squat, are unit
    // tested in `programs/ata/src/tests.rs`; what is end-to-end here is that a
    // real transaction actually performs the replacement.
    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
        kind: TokenKind::Fungible,
        contents: AtaContents::Squatted,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
        ),
        "the squatted holding must be replaced by the intended one"
    );
}

#[test]
fn ata_transfer() {
    let mut state = state_for_ata_tests_with_precreated_recipient_ata();

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: 400_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
            ProgramShardSelector::new(Ids::recipient_ata(), Ids::token_program()),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 600_000_u128,
            }),
        )
    );

    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 400_000_u128,
            }),
        )
    );
}

#[test]
fn ata_transfer_rejects_default_recipient() {
    let mut state = state_for_ata_tests();

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: 1_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
            ProgramShardSelector::new(Ids::recipient_ata(), Ids::token_program()),
        ],
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
    let mismatched_recipient = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: foreign_definition_id,
            balance: 0_u128,
        }),
    );
    state.force_insert_account(Ids::recipient_ata(), mismatched_recipient.clone());

    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: 1_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
            ProgramShardSelector::new(Ids::recipient_ata(), Ids::token_program()),
        ],
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
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: 300_000_u128,
    };

    let message = public_transaction::Message::try_new(
        Ids::ata_program(),
        vec![
            ProgramShardSelector::new(Ids::owner(), Ids::token_program()),
            ProgramShardSelector::new(Ids::owner_ata(), Ids::token_program()),
            ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner_key()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::owner_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 700_000_u128,
            }),
        )
    );

    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 700_000_u128,
                metadata_id: None,
                authority: None,
            }),
        )
    );
}

#[test]
fn ata_create_from_private_owner() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    // Private owner key material
    let owner_ask = AuthorizationSecretKey([13u8; 32]);
    let owner_nsk = NullifierSecretKey::from(&owner_ask);
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    // `ViewingPublicKey::from_seed` needs two 32-byte halves `(d, z)`.
    let owner_vpk = ViewingPublicKey::from_seed(&[31u8; 32], &[32u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, &owner_vpk, Identifier::ZERO);

    // ATA derived from the private owner
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let owner_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    // Pre-states: private uninitialized owner, public token definition, public uninitialized ATA.

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
        kind: TokenKind::Fungible,
        contents: AtaContents::Empty,
    };
    let instruction_data = Program::serialize_instruction(instruction).unwrap();

    // The private owner is a fresh account the caller does not own: PrivateForeignInit.
    // The circuit derives the EPK from `random_seed`; the init references the current root.
    let commitment_root = state.commitment_root();

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
                ProgramShardSelector::new(owner_ata_id, Ids::token_program()),
            ],
            signers: [].into(),
            private_witnesses: vec![
                // owner: a new private account the caller neither owns nor spends
                // (no credential, no proof).
                private_foreign_init_identity(owner_npk, &owner_vpk, commitment_root),
            ],
            instruction_data,
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);

    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);
    state
        .transition_from_privacy_preserving_transaction(&tx, 0, 0)
        .unwrap();

    assert_eq!(
        state.get_account_by_id(owner_ata_id),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
        )
    );
}

/// ATA cannot be created as a private account.
#[test]
fn ata_create_private_ata_holding_is_not_expressible() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::token_definition(), Accounts::token_definition_init());

    let owner_id = Ids::owner();

    // Fresh personal npk/vpk for the ATA holding's privacy identity — distinct from the
    // owner's plain public keypair, which only supplies the seed input.
    let ata_ask = AuthorizationSecretKey([21u8; 32]);
    let ata_nsk = NullifierSecretKey::from(&ata_ask);
    let ata_npk = NullifierPublicKey::from(&ata_nsk);
    let ata_vpk = ViewingPublicKey::from_seed(&[41u8; 32], &[42u8; 32]);

    // Address stays the *standard* public-PDA formula ATA always uses — only its state
    // privacy is under test, not its address derivation.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
        kind: TokenKind::Fungible,
        contents: AtaContents::Empty,
    };
    let instruction_data = Program::serialize_instruction(instruction).unwrap();

    let commitment_root = state.commitment_root();

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let result = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
                ProgramShardSelector::new(ata_id, Ids::token_program()),
            ],
            signers: [owner_id].into(),
            private_witnesses: vec![
                // A private-PDA init: `WitnessKind::Pda`, whose optional `binding`
                // replaced the old `seed` field.
                PrivateWitness {
                    vpk: ata_vpk.clone(),
                    random_seed: [0; 32],
                    identifier: Identifier::ZERO,
                    // v0.3.0 made the binding mandatory, so the witness has to
                    // claim the ATA program and the ATA's own seed outright.
                    kind: WitnessKind::Pda {
                        binding: (Ids::ata_program(), seed),
                    },
                    nullifier: NullifierWitness::Init {
                        npk: ata_npk,
                        commitment_root,
                    },
                },
            ],
            instruction_data,
            ..Default::default()
        },
        &program_with_deps,
    );

    let err = result.expect_err(
        "a private-PDA ATA holding must be rejected: its account id can never satisfy both \
         ATA's own for_public_pda address check and the private-PDA witness's for_private_pda \
         binding requirement simultaneously",
    );
    let message = format!("{err:?}");
    // Same impossibility, reported at an earlier checkpoint in v0.3.0. A
    // `WitnessKind::Pda` witness derives its account id with `for_private_pda`
    // from the binding, while the slot names ATA's `for_public_pda` address, so
    // the witness's account is simply not one of the root call's inputs —
    // `execution_state` rejects it before any binding check runs. The two
    // derivations can never agree, which is exactly what this test asserts.
    assert!(
        message.contains("WitnessNotInRoot"),
        "expected the private-PDA witness to be rejected as not an input of the \
         root call, got a different error: {message}"
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128,
            }),
        ),
    );

    let recipient_ask = AuthorizationSecretKey([51u8; 32]);
    let recipient_nsk = NullifierSecretKey::from(&recipient_ask);
    let recipient_npk = NullifierPublicKey::from(&recipient_nsk);
    let recipient_vpk = ViewingPublicKey::from_seed(&[61u8; 32], &[62u8; 32]);
    let recipient_id =
        AccountId::for_regular_private_account(&recipient_npk, &recipient_vpk, Identifier::ZERO);

    let shield_amount = 500_000_u128;
    let shield_commitment_root = state.commitment_root();

    let token_program_for_shield = ProgramWithDependencies::new(
        Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF"),
        Ids::token_program(),
        HashMap::new(),
    );
    let (shield_output, shield_proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(shield_source_id, Ids::token_program()),
                ProgramShardSelector::new(recipient_id, Ids::token_program()),
            ],
            signers: [shield_source_id].into(),
            private_witnesses: vec![private_foreign_init_identity(
                recipient_npk,
                &recipient_vpk,
                shield_commitment_root,
            )],
            instruction_data: Program::serialize_instruction(token_core::Instruction::Transfer {
                amount_to_transfer: shield_amount,
                descriptor: TokenDescriptor {
                    definition_id: Ids::token_definition(),
                    kind: TokenKind::Fungible,
                },
            })
            .unwrap(),
            ..Default::default()
        },
        &token_program_for_shield,
    )
    .unwrap();
    let shield_message = Message::from_circuit_output(vec![Nonce(0)], shield_output);
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
        nonce: Nonce::private_account_nonce_init(&recipient_id),
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount,
        }),
    );
    assert!(
        state
            .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_after_shield))
            .is_some(),
        "shield setup must land before the ATA transfer under test"
    );

    // Now the actual test: owner's ATA sends more into that now-existing private recipient.
    let owner_id = Ids::owner();
    let sender_ata_id = Ids::owner_ata();

    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_after_shield))
        .expect("recipient's commitment must be in the set");

    let ata_transfer_amount = 200_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: ata_transfer_amount,
    };
    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(sender_ata_id, Ids::token_program()),
                ProgramShardSelector::new(recipient_id, Ids::token_program()),
            ],
            signers: [owner_id, recipient_id].into(),
            private_witnesses: vec![private_authorized_update_identity(
                recipient_ask,
                &recipient_vpk,
                recipient_after_shield.clone(),
                membership_proof,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![Nonce(0)], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - ata_transfer_amount,
            }),
        )
    );

    let recipient_nonce_after = Nonce::private_account_nonce_init(&recipient_id)
        .private_account_nonce_increment(&recipient_nsk);
    let recipient_after_ata_transfer = Account {
        nonce: recipient_nonce_after,
        ..Account::default()
    }
    .with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: shield_amount + ata_transfer_amount,
        }),
    );
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

    let owner_ask = AuthorizationSecretKey([91u8; 32]);
    let owner_nsk = NullifierSecretKey::from(&owner_ask);
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    let owner_vpk = ViewingPublicKey::from_seed(&[93u8; 32], &[94u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, &owner_vpk, Identifier::ZERO);

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let ata_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
    );
    state.force_insert_account(ata_id, ata_account.clone());

    let burn_amount = 300_000_u128;
    let instruction = ata_core::Instruction::Burn {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: burn_amount,
    };

    let commitment_root = state.commitment_root();

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(ata_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ],
            signers: [owner_id].into(),
            private_witnesses: vec![private_authorized_init_identity(
                owner_ask,
                &owner_vpk,
                commitment_root,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - burn_amount,
            }),
        )
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: None,
            }),
        )
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
    let bob_ask = alice.admit_member();

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let ata_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
    );
    state.force_insert_account(ata_id, ata_account.clone());

    let burn_amount = 300_000_u128;
    let instruction = ata_core::Instruction::Burn {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: burn_amount,
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(ata_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
            ],
            signers: [owner_id].into(),
            private_witnesses: vec![private_authorized_init_identity(
                bob_ask,
                &alice.vpk,
                state.commitment_root(),
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - burn_amount,
            }),
        )
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_definition()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: 1_000_000_u128 - burn_amount,
                metadata_id: None,
                authority: None,
            }),
        )
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

    let owner_ask = AuthorizationSecretKey([95u8; 32]);
    let owner_nsk = NullifierSecretKey::from(&owner_ask);
    let owner_npk = NullifierPublicKey::from(&owner_nsk);
    let owner_vpk = ViewingPublicKey::from_seed(&[96u8; 32], &[97u8; 32]);
    let owner_id = AccountId::for_regular_private_account(&owner_npk, &owner_vpk, Identifier::ZERO);

    // The ATA holding must stay public (per the confirmed PDA finding), so it's seeded
    // directly rather than via a real `Create` transaction.
    let seed = compute_ata_seed(Ids::token_program(), owner_id, Ids::token_definition());
    let sender_ata_id = get_associated_token_account_id(&Ids::ata_program(), &seed);
    let sender_ata_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
    );
    state.force_insert_account(sender_ata_id, sender_ata_account.clone());

    let transfer_amount = 400_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: transfer_amount,
    };

    let commitment_root = state.commitment_root();

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(sender_ata_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::recipient_ata(), Ids::token_program()),
            ],
            signers: [owner_id].into(),
            private_witnesses: vec![private_authorized_init_identity(
                owner_ask,
                &owner_vpk,
                commitment_root,
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - transfer_amount,
            }),
        )
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: transfer_amount,
            }),
        )
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
    let sender_ata_account = Account::default().with_shard(
        Ids::token_program(),
        ShardData::from(&TokenHolding::Fungible {
            definition_id: Ids::token_definition(),
            balance: 1_000_000_u128,
        }),
    );
    state.force_insert_account(sender_ata_id, sender_ata_account.clone());

    let bob_ask = alice.admit_member();

    let transfer_amount = 400_000_u128;
    let instruction = ata_core::Instruction::Transfer {
        token_program_id: Ids::token_program(),
        descriptor: TokenDescriptor {
            definition_id: Ids::token_definition(),
            kind: TokenKind::Fungible,
        },
        amount: transfer_amount,
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(sender_ata_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::recipient_ata(), Ids::token_program()),
            ],
            signers: [owner_id].into(),
            private_witnesses: vec![private_authorized_init_identity(
                bob_ask,
                &alice.vpk,
                state.commitment_root(),
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 1_000_000_u128 - transfer_amount,
            }),
        )
    );
    assert_eq!(
        state.get_account_by_id(Ids::recipient_ata()),
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: transfer_amount,
            }),
        )
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

    // Group-derived owner is a fresh private account initialized via PrivateForeignInit;
    // since logos-execution-zone PR #621 its pre-state must be is_authorized == true.

    let instruction = ata_core::Instruction::Create {
        token_program_id: Ids::token_program(),
        kind: TokenKind::Fungible,
        contents: AtaContents::Empty,
    };

    let ata_program = Program::new(ata_methods::ATA_ELF.to_vec().into()).unwrap();
    let token_program = Program::new(token_methods::TOKEN_ELF.to_vec().into()).unwrap();
    let program_with_deps = ProgramWithDependencies::new(
        ata_program,
        Ids::ata_program(),
        HashMap::from([(Ids::token_program(), token_program)]),
    );

    let (output, proof) = execute_and_prove(
        ProvingInput {
            shard_selectors: vec![
                ProgramShardSelector::new(owner_id, Ids::token_program()),
                ProgramShardSelector::new(Ids::token_definition(), Ids::token_program()),
                ProgramShardSelector::new(owner_ata_id, Ids::token_program()),
            ],
            signers: [].into(),
            private_witnesses: vec![private_foreign_init_identity(
                alice.npk,
                &alice.vpk,
                state.commitment_root(),
            )],
            instruction_data: Program::serialize_instruction(instruction).unwrap(),
            ..Default::default()
        },
        &program_with_deps,
    )
    .unwrap();

    let message = Message::from_circuit_output(vec![], output);
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
        Account::default().with_shard(
            Ids::token_program(),
            ShardData::from(&TokenHolding::Fungible {
                definition_id: Ids::token_definition(),
                balance: 0_u128,
            }),
        )
    );
}
