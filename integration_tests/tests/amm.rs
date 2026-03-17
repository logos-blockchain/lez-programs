use amm_core::PoolDefinition;
use nssa::{
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use nssa_core::account::{Account, AccountId, Data, Nonce};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Balances;
struct Accounts;

impl Keys {
    fn user_a() -> PrivateKey {
        PrivateKey::try_new([31; 32]).expect("valid private key")
    }

    fn user_b() -> PrivateKey {
        PrivateKey::try_new([32; 32]).expect("valid private key")
    }

    fn user_lp() -> PrivateKey {
        PrivateKey::try_new([33; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn amm_program() -> nssa_core::program::ProgramId {
        amm_methods::AMM_ID
    }

    fn token_a_definition() -> AccountId {
        AccountId::new([3; 32])
    }

    fn token_b_definition() -> AccountId {
        AccountId::new([4; 32])
    }

    fn pool_definition() -> AccountId {
        amm_core::compute_pool_pda(
            Self::amm_program(),
            Self::token_a_definition(),
            Self::token_b_definition(),
        )
    }

    fn token_lp_definition() -> AccountId {
        amm_core::compute_liquidity_token_pda(Self::amm_program(), Self::pool_definition())
    }

    fn vault_a() -> AccountId {
        amm_core::compute_vault_pda(
            Self::amm_program(),
            Self::pool_definition(),
            Self::token_a_definition(),
        )
    }

    fn vault_b() -> AccountId {
        amm_core::compute_vault_pda(
            Self::amm_program(),
            Self::pool_definition(),
            Self::token_b_definition(),
        )
    }

    fn user_a() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_a()))
    }

    fn user_b() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_b()))
    }

    fn user_lp() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_lp()))
    }
}

impl Balances {
    fn user_a_init() -> u128 {
        10_000
    }

    fn user_b_init() -> u128 {
        10_000
    }

    fn user_lp_init() -> u128 {
        2_000
    }

    fn vault_a_init() -> u128 {
        5_000
    }

    fn vault_b_init() -> u128 {
        2_500
    }

    fn pool_lp_supply_init() -> u128 {
        5_000
    }

    fn token_a_supply() -> u128 {
        100_000
    }

    fn token_b_supply() -> u128 {
        100_000
    }

    fn token_lp_supply() -> u128 {
        5_000
    }

    fn remove_lp() -> u128 {
        1_000
    }

    fn remove_min_a() -> u128 {
        500
    }

    fn remove_min_b() -> u128 {
        500
    }

    fn add_min_lp() -> u128 {
        1_000
    }

    fn add_max_a() -> u128 {
        2_000
    }

    fn add_max_b() -> u128 {
        1_000
    }

    fn swap_amount_in() -> u128 {
        1_000
    }

    fn swap_min_out() -> u128 {
        200
    }

    fn vault_a_swap_1() -> u128 {
        3_572
    }

    fn vault_b_swap_1() -> u128 {
        3_500
    }

    fn user_a_swap_1() -> u128 {
        11_428
    }

    fn user_b_swap_1() -> u128 {
        9_000
    }

    fn vault_a_swap_2() -> u128 {
        6_000
    }

    fn vault_b_swap_2() -> u128 {
        2_084
    }

    fn user_a_swap_2() -> u128 {
        9_000
    }

    fn user_b_swap_2() -> u128 {
        10_416
    }

    fn vault_a_add() -> u128 {
        7_000
    }

    fn vault_b_add() -> u128 {
        3_500
    }

    fn user_a_add() -> u128 {
        8_000
    }

    fn user_b_add() -> u128 {
        9_000
    }

    fn user_lp_add() -> u128 {
        4_000
    }

    fn token_lp_supply_add() -> u128 {
        7_000
    }

    fn vault_a_remove() -> u128 {
        4_000
    }

    fn vault_b_remove() -> u128 {
        2_000
    }

    fn user_a_remove() -> u128 {
        11_000
    }

    fn user_b_remove() -> u128 {
        10_500
    }

    fn user_lp_remove() -> u128 {
        1_000
    }

    fn token_lp_supply_remove() -> u128 {
        4_000
    }

    fn user_a_new_definition() -> u128 {
        5_000
    }

    fn user_b_new_definition() -> u128 {
        7_500
    }

    fn lp_supply_init() -> u128 {
        (Self::vault_a_init() * Self::vault_b_init()).isqrt()
    }
}

impl Accounts {
    fn user_a_holding() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_b_holding() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_init() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::pool_lp_supply_init(),
                reserve_a: Balances::vault_a_init(),
                reserve_b: Balances::vault_b_init(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn token_a_definition_account() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("test"),
                total_supply: Balances::token_a_supply(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn token_b_definition_account() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("test"),
                total_supply: Balances::token_b_supply(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn token_lp_definition_account() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: Balances::token_lp_supply(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::vault_a_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::vault_b_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_lp_holding() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: Balances::user_lp_init(),
            }),
            nonce: Nonce(0),
        }
    }

    // --- Expected post-state accounts ---

    fn pool_definition_swap_1() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::pool_lp_supply_init(),
                reserve_a: Balances::vault_a_swap_1(),
                reserve_b: Balances::vault_b_swap_1(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_swap_1() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::vault_a_swap_1(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_swap_1() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::vault_b_swap_1(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_swap_1() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_swap_1(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_b_holding_swap_1() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_swap_1(),
            }),
            nonce: Nonce(1),
        }
    }

    fn pool_definition_swap_2() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::pool_lp_supply_init(),
                reserve_a: Balances::vault_a_swap_2(),
                reserve_b: Balances::vault_b_swap_2(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_swap_2() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::vault_a_swap_2(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_swap_2() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::vault_b_swap_2(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_swap_2() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_swap_2(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_b_holding_swap_2() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_swap_2(),
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_add() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::token_lp_supply_add(),
                reserve_a: Balances::vault_a_add(),
                reserve_b: Balances::vault_b_add(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::vault_a_add(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::vault_b_add(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_add(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_b_holding_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_add(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_lp_holding_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: Balances::user_lp_add(),
            }),
            nonce: Nonce(0),
        }
    }

    fn token_lp_definition_add() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: Balances::token_lp_supply_add(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_remove() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::token_lp_supply_remove(),
                reserve_a: Balances::vault_a_remove(),
                reserve_b: Balances::vault_b_remove(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::vault_a_remove(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::vault_b_remove(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_remove(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_b_holding_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_remove(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_lp_holding_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: Balances::user_lp_remove(),
            }),
            nonce: Nonce(1),
        }
    }

    fn token_lp_definition_remove() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: Balances::token_lp_supply_remove(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn token_lp_definition_init_inactive() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: 0,
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_init_inactive() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: 0,
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_init_inactive() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: 0,
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_inactive() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: 0,
                reserve_a: 0,
                reserve_b: 0,
                fees: 0_u128,
                active: false,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_new_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_new_definition(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_b_holding_new_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_new_definition(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_lp_holding_new_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: Balances::lp_supply_init(),
            }),
            nonce: Nonce(0),
        }
    }

    fn token_lp_definition_new_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: Balances::lp_supply_init(),
                metadata_id: None,
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_new_init() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: Ids::token_a_definition(),
                definition_token_b_id: Ids::token_b_definition(),
                vault_a_id: Ids::vault_a(),
                vault_b_id: Ids::vault_b(),
                liquidity_pool_id: Ids::token_lp_definition(),
                liquidity_pool_supply: Balances::lp_supply_init(),
                reserve_a: Balances::vault_a_init(),
                reserve_b: Balances::vault_b_init(),
                fees: 0_u128,
                active: true,
            }),
            nonce: Nonce(0),
        }
    }

    fn user_lp_holding_init_zero() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: 0,
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

    let amm_message = program_deployment_transaction::Message::new(amm_methods::AMM_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            amm_message,
        ))
        .expect("amm program deployment must succeed");
}

fn state_for_amm_tests() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_programs(&mut state);
    state.force_insert_account(Ids::pool_definition(), Accounts::pool_definition_init());
    state.force_insert_account(
        Ids::token_a_definition(),
        Accounts::token_a_definition_account(),
    );
    state.force_insert_account(
        Ids::token_b_definition(),
        Accounts::token_b_definition_account(),
    );
    state.force_insert_account(
        Ids::token_lp_definition(),
        Accounts::token_lp_definition_account(),
    );
    state.force_insert_account(Ids::user_a(), Accounts::user_a_holding());
    state.force_insert_account(Ids::user_b(), Accounts::user_b_holding());
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding());
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_init());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_init());
    state
}

fn state_for_amm_tests_with_new_def() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[]);
    deploy_programs(&mut state);
    state.force_insert_account(
        Ids::token_a_definition(),
        Accounts::token_a_definition_account(),
    );
    state.force_insert_account(
        Ids::token_b_definition(),
        Accounts::token_b_definition_account(),
    );
    state.force_insert_account(Ids::user_a(), Accounts::user_a_holding());
    state.force_insert_account(Ids::user_b(), Accounts::user_b_holding());
    state
}

#[test]
fn amm_remove_liquidity() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_remove()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_remove()
    );
}

#[test]
fn amm_new_definition_inactive_initialized_pool_and_uninit_user_lp() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_init_inactive());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_init_inactive());
    state.force_insert_account(Ids::pool_definition(), Accounts::pool_definition_inactive());
    state.force_insert_account(
        Ids::token_lp_definition(),
        Accounts::token_lp_definition_init_inactive(),
    );

    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        amm_program_id: Ids::amm_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
        ],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_new_init()
    );
}

#[test]
fn amm_new_definition_inactive_initialized_pool_init_user_lp() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_init_inactive());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_init_inactive());
    state.force_insert_account(Ids::pool_definition(), Accounts::pool_definition_inactive());
    state.force_insert_account(
        Ids::token_lp_definition(),
        Accounts::token_lp_definition_init_inactive(),
    );
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding_init_zero());

    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        amm_program_id: Ids::amm_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
        ],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_new_init()
    );
}

#[test]
fn amm_new_definition_uninitialized_pool() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_init_inactive());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_init_inactive());

    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        amm_program_id: Ids::amm_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
        ],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_new_init()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_new_init()
    );
}

#[test]
fn amm_add_liquidity() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::AddLiquidity {
        min_amount_liquidity: Balances::add_min_lp(),
        max_amount_to_add_token_a: Balances::add_max_a(),
        max_amount_to_add_token_b: Balances::add_max_b(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
        ],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_add()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_add()
    );
}

#[test]
fn amm_swap_b_to_a() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::Swap {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        token_definition_id_in: Ids::token_b_definition(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_swap_1()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_swap_1()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_swap_1()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_swap_1()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_swap_1()
    );
}

#[test]
fn amm_swap_a_to_b() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::Swap {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        token_definition_id_in: Ids::token_a_definition(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_swap_2()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_swap_2()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_swap_2()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_swap_2()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_swap_2()
    );
}
