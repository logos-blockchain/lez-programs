use nssa::{
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use nssa_core::account::{Account, AccountId, Data, Nonce};
use stablecoin_core::{compute_position_pda, compute_position_vault_pda, Position};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Balances;
struct Accounts;

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
    let mut state = V03State::new_with_genesis_accounts(&[], vec![], 0);
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
    let mut state = V03State::new_with_genesis_accounts(&[], vec![], 0);
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
