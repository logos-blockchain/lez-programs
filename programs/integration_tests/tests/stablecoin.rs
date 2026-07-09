use std::collections::HashMap;

use integration_tests::{
    private_authorized_init_identity, private_authorized_update_identity,
    private_unauthorized_identity, GroupOwner,
};
use nssa::{
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::ProgramWithDependencies, Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use nssa_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    encryption::ViewingPublicKey,
    Commitment, InputAccountIdentity, Nullifier, NullifierPublicKey, NullifierSecretKey,
};
use stablecoin_core::{compute_position_pda, compute_position_vault_pda, Position};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Balances;
struct Accounts;
struct PrivateKeys;

impl PrivateKeys {
    fn destination_nsk() -> NullifierSecretKey {
        [111; 32]
    }

    fn destination_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::destination_nsk())
    }

    fn destination_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[141; 32], &[142; 32])
    }

    fn destination_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::destination_npk(), 0)
    }

    fn stablecoin_holding_nsk() -> NullifierSecretKey {
        [121; 32]
    }

    fn stablecoin_holding_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::stablecoin_holding_nsk())
    }

    fn stablecoin_holding_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[151; 32], &[152; 32])
    }

    fn stablecoin_holding_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::stablecoin_holding_npk(), 0)
    }
}

impl Keys {
    fn owner() -> PrivateKey {
        PrivateKey::try_new([41; 32]).expect("valid private key")
    }

    fn user_holding() -> PrivateKey {
        PrivateKey::try_new([42; 32]).expect("valid private key")
    }

    fn user_stablecoin_holding() -> PrivateKey {
        PrivateKey::try_new([43; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn stablecoin_program() -> nssa_core::program::ProgramId {
        stablecoin_methods::STABLECOIN_ID
    }

    fn collateral_definition() -> AccountId {
        AccountId::new([5; 32])
    }

    fn owner() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::owner()))
    }

    fn user_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_holding()))
    }

    fn stablecoin_definition() -> AccountId {
        AccountId::new([6; 32])
    }

    fn user_stablecoin_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(
            &Keys::user_stablecoin_holding(),
        ))
    }

    fn position() -> AccountId {
        compute_position_pda(
            Self::stablecoin_program(),
            Self::owner(),
            Self::collateral_definition(),
        )
    }

    fn vault() -> AccountId {
        compute_position_vault_pda(Self::stablecoin_program(), Self::position())
    }
}

impl Balances {
    fn user_holding_init() -> u128 {
        1_000_000
    }

    fn collateral_deposit() -> u128 {
        500_000
    }

    fn collateral_withdraw() -> u128 {
        200_000
    }

    fn stablecoin_supply_init() -> u128 {
        1_000
    }

    fn user_stablecoin_holding_init() -> u128 {
        1_000
    }

    fn initial_debt() -> u128 {
        300
    }

    fn debt_repay_amount() -> u128 {
        100
    }
}

impl Accounts {
    fn collateral_definition_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("Gold"),
                total_supply: Balances::user_holding_init(),
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_holding_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::collateral_definition(),
                balance: Balances::user_holding_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn stablecoin_definition_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("DAI"),
                total_supply: Balances::stablecoin_supply_init(),
                metadata_id: None,
                authority: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_stablecoin_holding_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::stablecoin_definition(),
                balance: Balances::user_stablecoin_holding_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn position_with_debt_init() -> Account {
        Account {
            program_owner: stablecoin_methods::STABLECOIN_ID,
            balance: 0_u128,
            data: Data::from(&Position {
                collateral_vault_id: Ids::vault(),
                collateral_definition_id: Ids::collateral_definition(),
                collateral_amount: Balances::collateral_deposit(),
                debt_amount: Balances::initial_debt(),
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

    let stablecoin_message =
        program_deployment_transaction::Message::new(stablecoin_methods::STABLECOIN_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            stablecoin_message,
        ))
        .expect("stablecoin program deployment must succeed");
}

fn state_for_stablecoin_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

fn state_for_stablecoin_repay_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );
    state.force_insert_account(Ids::position(), Accounts::position_with_debt_init());
    state.force_insert_account(
        Ids::user_stablecoin_holding(),
        Accounts::user_stablecoin_holding_init(),
    );
    state
}

fn assert_position(state: &V03State, expected_collateral: u128) {
    let position =
        Position::try_from(&state.get_account_by_id(Ids::position()).data).expect("valid Position");
    assert_eq!(position.collateral_amount, expected_collateral);
    assert_eq!(position.debt_amount, 0);
    assert_eq!(position.collateral_vault_id, Ids::vault());
    assert_eq!(
        position.collateral_definition_id,
        Ids::collateral_definition()
    );
}

fn assert_fungible_balance(state: &V03State, account_id: AccountId, expected_balance: u128) {
    let holding = TokenHolding::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid TokenHolding");
    match holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } => {
            assert_eq!(definition_id, Ids::collateral_definition());
            assert_eq!(balance, expected_balance);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

#[test]
fn stablecoin_open_position_then_withdraw_collateral() {
    let mut state = state_for_stablecoin_tests();

    // Open the position: deposit collateral from the user's holding into a fresh vault.
    let open = stablecoin_core::Instruction::OpenPosition {
        collateral_amount: Balances::collateral_deposit(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_holding(),
            Ids::collateral_definition(),
        ],
        vec![
            current_nonce(&state, Ids::owner()),
            current_nonce(&state, Ids::user_holding()),
        ],
        open,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("open_position must succeed");

    assert_position(&state, Balances::collateral_deposit());
    assert_fungible_balance(&state, Ids::vault(), Balances::collateral_deposit());
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - Balances::collateral_deposit(),
    );

    // Withdraw part of the collateral back to the same user holding.
    let withdraw = stablecoin_core::Instruction::WithdrawCollateral {
        amount: Balances::collateral_withdraw(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::vault(),
            Ids::user_holding(),
        ],
        vec![current_nonce(&state, Ids::owner())],
        withdraw,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("withdraw_collateral must succeed");

    assert_position(
        &state,
        Balances::collateral_deposit() - Balances::collateral_withdraw(),
    );
    assert_fungible_balance(
        &state,
        Ids::vault(),
        Balances::collateral_deposit() - Balances::collateral_withdraw(),
    );
    assert_fungible_balance(
        &state,
        Ids::user_holding(),
        Balances::user_holding_init() - Balances::collateral_deposit()
            + Balances::collateral_withdraw(),
    );
}

#[test]
fn stablecoin_repay_debt_burns_stablecoins_and_decreases_debt() {
    let mut state = state_for_stablecoin_repay_tests();

    let repay = stablecoin_core::Instruction::RepayDebt {
        amount: Balances::debt_repay_amount(),
    };
    let message = public_transaction::Message::try_new(
        Ids::stablecoin_program(),
        vec![
            Ids::owner(),
            Ids::position(),
            Ids::stablecoin_definition(),
            Ids::user_stablecoin_holding(),
        ],
        vec![
            current_nonce(&state, Ids::owner()),
            current_nonce(&state, Ids::user_stablecoin_holding()),
        ],
        repay,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::owner(), &Keys::user_stablecoin_holding()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .expect("repay_debt must succeed");

    // Position debt decreased; collateral untouched.
    let position =
        Position::try_from(&state.get_account_by_id(Ids::position()).data).expect("valid Position");
    assert_eq!(
        position.debt_amount,
        Balances::initial_debt() - Balances::debt_repay_amount()
    );
    assert_eq!(position.collateral_amount, Balances::collateral_deposit());

    // Stablecoin total supply decreased by the burn amount.
    let definition =
        TokenDefinition::try_from(&state.get_account_by_id(Ids::stablecoin_definition()).data)
            .expect("valid TokenDefinition");
    match definition {
        TokenDefinition::Fungible { total_supply, .. } => {
            assert_eq!(
                total_supply,
                Balances::stablecoin_supply_init() - Balances::debt_repay_amount()
            );
        }
        TokenDefinition::NonFungible { .. } => panic!("expected Fungible definition"),
    }

    // User stablecoin holding decreased by the burn amount.
    let holding =
        TokenHolding::try_from(&state.get_account_by_id(Ids::user_stablecoin_holding()).data)
            .expect("valid TokenHolding");
    match holding {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(
                balance,
                Balances::user_stablecoin_holding_init() - Balances::debt_repay_amount()
            );
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible holding")
        }
    }
}

fn stablecoin_program() -> Program {
    Program::new(stablecoin_methods::STABLECOIN_ELF.to_vec().into()).expect("valid stablecoin ELF")
}

fn token_program_instance() -> Program {
    Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF")
}

fn stablecoin_with_token_deps() -> ProgramWithDependencies {
    ProgramWithDependencies::new(
        stablecoin_program(),
        HashMap::from([(Ids::token_program(), token_program_instance())]),
    )
}


/// `OpenPosition` is blocked by the `privacy_preserving_circuit` due to the handling of
/// sibling chain calls of (uninitialized) private accounts.
#[test]
fn stablecoin_open_position_via_privacy_transaction_is_not_expressible() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre = AccountWithMetadata::new(Account::default(), false, position_id);
    let vault_pre = AccountWithMetadata::new(Account::default(), false, vault_id);
    let user_holding_pre =
        AccountWithMetadata::new(Accounts::user_holding_init(), true, Ids::user_holding());
    let definition_pre = AccountWithMetadata::new(
        Accounts::collateral_definition_init(),
        false,
        Ids::collateral_definition(),
    );

    let collateral_amount = Balances::collateral_deposit();
    let instruction = stablecoin_core::Instruction::OpenPosition { collateral_amount };

    let result = execute_and_prove(
        vec![
            owner_pre,
            position_pre,
            vault_pre,
            user_holding_pre,
            definition_pre,
        ],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &stablecoin_with_token_deps(),
    );

    let err = result.expect_err(
        "OpenPosition must be rejected by the privacy-preserving circuit: vault's second \
         chained-call occurrence declares is_authorized: false after already being marked \
         authorized by the first chained call's pda_seeds match",
    );
    let message = format!("{err:?}");
    assert!(
        message.contains("Inconsistent authorization for account"),
        "expected the authorization-consistency rejection, got a different error: {message}"
    );
}

/// `WithdrawCollateral` to private account (`PrivateAuthorized`; `nsk` is known).
#[test]
fn stablecoin_withdraw_collateral_private_destination() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: 0,
        }),
        nonce: Nonce(0),
    };
    let vault_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    let destination_nsk = PrivateKeys::destination_nsk();
    let destination_vpk = PrivateKeys::destination_vpk();
    let destination_id = PrivateKeys::destination_id();
    let destination_initial_balance = 100_000_u128;
    let destination_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&destination_id),
    };
    state = state.with_private_accounts([(
        Commitment::new(&destination_id, &destination_account),
        Nullifier::for_account_initialization(&destination_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &destination_account))
        .expect("destination's commitment must be in the set");

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let vault_pre = AccountWithMetadata::new(state.get_account_by_id(vault_id), false, vault_id);
    let destination_pre =
        AccountWithMetadata::new(destination_account.clone(), true, destination_id);

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        vec![owner_pre, position_pre, vault_pre, destination_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_authorized_update_identity(
                destination_nsk,
                &destination_vpk,
                membership_proof,
                0,
            ),
        ],
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![owner_id, position_id, vault_id],
        vec![Nonce(0)],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position =
        Position::try_from(&state.get_account_by_id(position_id).data).expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );
    assert_eq!(position.debt_amount, 0);

    match TokenHolding::try_from(&state.get_account_by_id(vault_id).data).expect("valid holding") {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(balance, position_collateral - withdraw_amount);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible vault holding")
        }
    }

    let destination_nonce_after = Nonce::private_account_nonce_init(&destination_id)
        .private_account_nonce_increment(&destination_nsk);
    let new_destination_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance + withdraw_amount,
        }),
        nonce: destination_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &new_destination_account))
        .is_some());
}

/// `WithdrawCollateral` blocks withdraws to private accounts (via private donations);
/// `PrivateUnauthorized` account initialization (e.g., `nsk` is not known) is not permitted
/// due to the assertion in `withdraw_collateral.rs` asserts `destination.account != Account::default()`
#[test]
fn stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: 0,
        }),
        nonce: Nonce(0),
    };
    let vault_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    let destination_npk = PrivateKeys::destination_npk();
    let destination_vpk = PrivateKeys::destination_vpk();
    let destination_id = PrivateKeys::destination_id();

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let vault_pre = AccountWithMetadata::new(state.get_account_by_id(vault_id), false, vault_id);
    let destination_pre = AccountWithMetadata::new(Account::default(), false, destination_id);

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        amount: withdraw_amount,
    };

    let result = execute_and_prove(
        vec![owner_pre, position_pre, vault_pre, destination_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_unauthorized_identity(destination_npk, &destination_vpk, 0),
        ],
        &stablecoin_with_token_deps(),
    );

    let err = result.expect_err(
        "WithdrawCollateral must be rejected: destination is a brand-new (default) private \
         account, but withdraw_collateral.rs requires the destination to already be initialized",
    );
    let message = format!("{err:?}");
    assert!(
        message.contains("Destination must be initialized"),
        "expected the destination-must-be-initialized rejection, got a different error: {message}"
    );
}

#[test]
fn stablecoin_withdraw_collateral_group_owned_destination() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;

    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: 0,
        }),
        nonce: Nonce(0),
    };
    let vault_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    // Alice creates the group and derives the shared destination's keys; Bob is admitted via
    // the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_nsk = alice.admit_member();
    let destination_vpk = alice.vpk;
    let destination_id = alice.id;

    let destination_initial_balance = 100_000_u128;
    let destination_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&destination_id),
    };
    state = state.with_private_accounts([(
        Commitment::new(&destination_id, &destination_account),
        Nullifier::for_account_initialization(&destination_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &destination_account))
        .expect("destination's commitment must be in the set");

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let vault_pre = AccountWithMetadata::new(state.get_account_by_id(vault_id), false, vault_id);
    let destination_pre =
        AccountWithMetadata::new(destination_account.clone(), true, destination_id);

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        vec![owner_pre, position_pre, vault_pre, destination_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_authorized_update_identity(bob_nsk, &destination_vpk, membership_proof, 0),
        ],
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![owner_id, position_id, vault_id],
        vec![Nonce(0)],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position =
        Position::try_from(&state.get_account_by_id(position_id).data).expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );

    let destination_nonce_after = Nonce::private_account_nonce_init(&destination_id)
        .private_account_nonce_increment(&bob_nsk);
    let new_destination_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: destination_initial_balance + withdraw_amount,
        }),
        nonce: destination_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&destination_id, &new_destination_account))
        .is_some());
}

#[test]
fn stablecoin_repay_debt_private_stablecoin_holding() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = Balances::collateral_deposit();
    let initial_debt = Balances::initial_debt();
    let repay_amount = Balances::debt_repay_amount();

    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: initial_debt,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);

    let stablecoin_holding_nsk = PrivateKeys::stablecoin_holding_nsk();
    let stablecoin_holding_vpk = PrivateKeys::stablecoin_holding_vpk();
    let stablecoin_holding_id = PrivateKeys::stablecoin_holding_id();
    let initial_stablecoin_balance = Balances::user_stablecoin_holding_init();
    let stablecoin_holding_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&stablecoin_holding_id),
    };
    state = state.with_private_accounts([(
        Commitment::new(&stablecoin_holding_id, &stablecoin_holding_account),
        Nullifier::for_account_initialization(&stablecoin_holding_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(
            &stablecoin_holding_id,
            &stablecoin_holding_account,
        ))
        .expect("stablecoin holding's commitment must be in the set");

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let definition_pre = AccountWithMetadata::new(
        Accounts::stablecoin_definition_init(),
        false,
        Ids::stablecoin_definition(),
    );
    let stablecoin_holding_pre = AccountWithMetadata::new(
        stablecoin_holding_account.clone(),
        true,
        stablecoin_holding_id,
    );

    let instruction = stablecoin_core::Instruction::RepayDebt {
        amount: repay_amount,
    };

    let (output, proof) = execute_and_prove(
        vec![
            owner_pre,
            position_pre,
            definition_pre,
            stablecoin_holding_pre,
        ],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_authorized_update_identity(
                stablecoin_holding_nsk,
                &stablecoin_holding_vpk,
                membership_proof,
                0,
            ),
        ],
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![owner_id, position_id, Ids::stablecoin_definition()],
        vec![Nonce(0)],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position =
        Position::try_from(&state.get_account_by_id(position_id).data).expect("valid Position");
    assert_eq!(position.debt_amount, initial_debt - repay_amount);
    assert_eq!(position.collateral_amount, position_collateral);

    match TokenDefinition::try_from(&state.get_account_by_id(Ids::stablecoin_definition()).data)
        .expect("valid TokenDefinition")
    {
        TokenDefinition::Fungible { total_supply, .. } => {
            assert_eq!(
                total_supply,
                Balances::stablecoin_supply_init() - repay_amount
            );
        }
        _ => panic!("expected Fungible definition"),
    }

    let stablecoin_holding_nonce_after = Nonce::private_account_nonce_init(&stablecoin_holding_id)
        .private_account_nonce_increment(&stablecoin_holding_nsk);
    let new_stablecoin_holding_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance - repay_amount,
        }),
        nonce: stablecoin_holding_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(
            &stablecoin_holding_id,
            &new_stablecoin_holding_account
        ))
        .is_some());
}

#[test]
fn stablecoin_repay_debt_group_owned_stablecoin_holding() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(
        Ids::stablecoin_definition(),
        Accounts::stablecoin_definition_init(),
    );

    let owner_id = Ids::owner();
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = Balances::collateral_deposit();
    let initial_debt = Balances::initial_debt();
    let repay_amount = Balances::debt_repay_amount();

    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: initial_debt,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);

    // Alice creates the group and derives the shared stablecoin holding's keys; Bob is
    // admitted via the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_nsk = alice.admit_member();
    let holding_vpk = alice.vpk;
    let holding_id = alice.id;

    let initial_stablecoin_balance = Balances::user_stablecoin_holding_init();
    let holding_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance,
        }),
        nonce: Nonce::private_account_nonce_init(&holding_id),
    };
    state = state.with_private_accounts([(
        Commitment::new(&holding_id, &holding_account),
        Nullifier::for_account_initialization(&holding_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &holding_account))
        .expect("stablecoin holding's commitment must be in the set");

    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let definition_pre = AccountWithMetadata::new(
        Accounts::stablecoin_definition_init(),
        false,
        Ids::stablecoin_definition(),
    );
    let holding_pre = AccountWithMetadata::new(holding_account.clone(), true, holding_id);

    let instruction = stablecoin_core::Instruction::RepayDebt {
        amount: repay_amount,
    };

    let (output, proof) = execute_and_prove(
        vec![owner_pre, position_pre, definition_pre, holding_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_authorized_update_identity(bob_nsk, &holding_vpk, membership_proof, 0),
        ],
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![owner_id, position_id, Ids::stablecoin_definition()],
        vec![Nonce(0)],
        output,
    )
    .unwrap();
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::owner()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    let position =
        Position::try_from(&state.get_account_by_id(position_id).data).expect("valid Position");
    assert_eq!(position.debt_amount, initial_debt - repay_amount);

    let holding_nonce_after =
        Nonce::private_account_nonce_init(&holding_id).private_account_nonce_increment(&bob_nsk);
    let new_holding_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::stablecoin_definition(),
            balance: initial_stablecoin_balance - repay_amount,
        }),
        nonce: holding_nonce_after,
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &new_holding_account))
        .is_some());
}

#[test]
fn stablecoin_group_owned_position_owner() {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::collateral_definition(),
        Accounts::collateral_definition_init(),
    );
    state.force_insert_account(Ids::user_holding(), Accounts::user_holding_init());

    // Alice creates the group and derives the shared owner identity's keys; Bob is admitted
    // via the real seal/unseal handshake and independently re-derives the same keys.
    let alice = GroupOwner::new([7_u8; 32]);
    let bob_nsk = alice.admit_member();
    let owner_id = alice.id;

    // Position/vault addresses are derived from the group-owned owner_id — still ordinary
    // public PDAs (the seed formula doesn't care whether owner_id is public or private), seeded
    // directly since OpenPosition can't be routed through the privacy circuit at all.
    let position_id = compute_position_pda(
        Ids::stablecoin_program(),
        owner_id,
        Ids::collateral_definition(),
    );
    let vault_id = compute_position_vault_pda(Ids::stablecoin_program(), position_id);

    let position_collateral = 500_000_u128;
    let withdraw_amount = 200_000_u128;
    let position_account = Account {
        program_owner: Ids::stablecoin_program(),
        balance: 0,
        data: Data::from(&Position {
            collateral_vault_id: vault_id,
            collateral_definition_id: Ids::collateral_definition(),
            collateral_amount: position_collateral,
            debt_amount: 0,
        }),
        nonce: Nonce(0),
    };
    let vault_account = Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::collateral_definition(),
            balance: position_collateral,
        }),
        nonce: Nonce(0),
    };
    state.force_insert_account(position_id, position_account);
    state.force_insert_account(vault_id, vault_account);

    // Bob self-initializes and signs the owner identity in the same transaction, then
    // withdraws collateral through it. Destination stays public to isolate what's under test:
    // only the owner identity's privacy/sharing, nothing else.
    let owner_pre = AccountWithMetadata::new(Account::default(), true, owner_id);
    let position_pre =
        AccountWithMetadata::new(state.get_account_by_id(position_id), false, position_id);
    let vault_pre = AccountWithMetadata::new(state.get_account_by_id(vault_id), false, vault_id);
    let destination_pre =
        AccountWithMetadata::new(Accounts::user_holding_init(), false, Ids::user_holding());

    let instruction = stablecoin_core::Instruction::WithdrawCollateral {
        amount: withdraw_amount,
    };

    let (output, proof) = execute_and_prove(
        vec![owner_pre, position_pre, vault_pre, destination_pre],
        Program::serialize_instruction(instruction).unwrap(),
        vec![
            private_authorized_init_identity(bob_nsk, &alice.vpk, 0),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &stablecoin_with_token_deps(),
    )
    .unwrap();

    let message = Message::try_from_circuit_output(
        vec![position_id, vault_id, Ids::user_holding()],
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

    let position =
        Position::try_from(&state.get_account_by_id(position_id).data).expect("valid Position");
    assert_eq!(
        position.collateral_amount,
        position_collateral - withdraw_amount
    );

    match TokenHolding::try_from(&state.get_account_by_id(Ids::user_holding()).data)
        .expect("valid holding")
    {
        TokenHolding::Fungible { balance, .. } => {
            assert_eq!(balance, Balances::user_holding_init() + withdraw_amount);
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected Fungible destination holding")
        }
    }

    let owner_expected = Account {
        nonce: Nonce::private_account_nonce_init(&owner_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&owner_id, &owner_expected))
        .is_some());
}
