#![expect(
    clippy::arithmetic_side_effects,
    reason = "integration fixtures use fixed balances to assert AMM state transitions"
)]

use amm_core::{
    PoolDefinition, FEE_TIER_BPS_1, FEE_TIER_BPS_100, FEE_TIER_BPS_30, FEE_TIER_BPS_5,
    MINIMUM_LIQUIDITY,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa::{
    error::LeeError,
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

    fn admin() -> PrivateKey {
        PrivateKey::try_new([34; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> nssa_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn amm_program() -> nssa_core::program::ProgramId {
        amm_methods::AMM_ID
    }

    fn twap_oracle_program() -> nssa_core::program::ProgramId {
        twap_oracle_methods::TWAP_ORACLE_ID
    }

    fn config() -> AccountId {
        amm_core::compute_config_pda(Self::amm_program())
    }

    fn price_observations(window_duration: u64) -> AccountId {
        twap_oracle_core::compute_price_observations_pda(
            Self::twap_oracle_program(),
            Self::pool_definition(),
            window_duration,
        )
    }

    fn current_tick_account() -> AccountId {
        twap_oracle_core::compute_current_tick_account_pda(
            Self::twap_oracle_program(),
            Self::pool_definition(),
        )
    }

    fn oracle_price_account(window_duration: u64) -> AccountId {
        twap_oracle_core::compute_oracle_price_account_pda(
            Self::twap_oracle_program(),
            Self::pool_definition(),
            window_duration,
        )
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

    fn lp_lock_holding() -> AccountId {
        amm_core::compute_lp_lock_holding_pda(Self::amm_program(), Self::pool_definition())
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

    fn admin() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::admin()))
    }
}

impl Balances {
    fn fee_tier() -> u128 {
        FEE_TIER_BPS_30
    }

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

    fn reserve_a_swap_1() -> u128 {
        3_575
    }

    fn reserve_b_swap_1() -> u128 {
        3_500
    }

    fn vault_a_swap_1() -> u128 {
        3_575
    }

    fn vault_b_swap_1() -> u128 {
        3_500
    }

    fn user_a_swap_1() -> u128 {
        11_425
    }

    fn user_b_swap_1() -> u128 {
        9_000
    }

    fn reserve_a_swap_2() -> u128 {
        6_000
    }

    fn reserve_b_swap_2() -> u128 {
        2_085
    }

    fn vault_a_swap_2() -> u128 {
        6_000
    }

    fn vault_b_swap_2() -> u128 {
        2_085
    }

    fn user_a_swap_2() -> u128 {
        9_000
    }

    fn user_b_swap_2() -> u128 {
        10_415
    }

    fn exact_output_a_to_b_amount_in() -> u128 {
        437
    }

    fn reserve_a_swap_exact_output_a_to_b() -> u128 {
        Self::vault_a_init() + Self::exact_output_a_to_b_amount_in()
    }

    fn reserve_b_swap_exact_output_a_to_b() -> u128 {
        Self::vault_b_init() - Self::swap_min_out()
    }

    fn user_a_swap_exact_output_a_to_b() -> u128 {
        Self::user_a_init() - Self::exact_output_a_to_b_amount_in()
    }

    fn user_b_swap_exact_output_a_to_b() -> u128 {
        Self::user_b_init() + Self::swap_min_out()
    }

    fn exact_output_b_to_a_amount_in() -> u128 {
        106
    }

    fn reserve_a_swap_exact_output_b_to_a() -> u128 {
        Self::vault_a_init() - Self::swap_min_out()
    }

    fn reserve_b_swap_exact_output_b_to_a() -> u128 {
        Self::vault_b_init() + Self::exact_output_b_to_a_amount_in()
    }

    fn user_a_swap_exact_output_b_to_a() -> u128 {
        Self::user_a_init() + Self::swap_min_out()
    }

    fn user_b_swap_exact_output_b_to_a() -> u128 {
        Self::user_b_init() - Self::exact_output_b_to_a_amount_in()
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

    fn lp_user_init() -> u128 {
        Self::lp_supply_init() - MINIMUM_LIQUIDITY
    }
}

impl Accounts {
    fn config() -> Account {
        Account {
            program_owner: Ids::amm_program(),
            balance: 0_u128,
            data: Data::from(&amm_core::AmmConfig {
                token_program_id: Ids::token_program(),
                twap_oracle_program_id: Ids::twap_oracle_program(),
                authority: Ids::admin(),
            }),
            nonce: Nonce(0),
        }
    }

    /// The pool's TWAP current-tick account, owned by the oracle program, holding `tick`. Seeded
    /// into state so swap/sync can refresh it via a chained UpdateCurrentTick call.
    fn current_tick_account(tick: i32) -> Account {
        Account {
            program_owner: Ids::twap_oracle_program(),
            balance: 0_u128,
            data: Data::from(&twap_oracle_core::CurrentTickAccount {
                tick,
                last_updated: 0,
            }),
            nonce: Nonce(0),
        }
    }

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
                fees: Balances::fee_tier(),
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
                authority: None,
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
                authority: None,
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
                authority: Some(Ids::token_lp_definition()),
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

    fn user_lp_holding_with_balance(balance: u128) -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance,
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
                reserve_a: Balances::reserve_a_swap_1(),
                reserve_b: Balances::reserve_b_swap_1(),
                fees: Balances::fee_tier(),
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
                reserve_a: Balances::reserve_a_swap_2(),
                reserve_b: Balances::reserve_b_swap_2(),
                fees: Balances::fee_tier(),
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

    fn pool_definition_swap_exact_output_a_to_b() -> Account {
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
                reserve_a: Balances::reserve_a_swap_exact_output_a_to_b(),
                reserve_b: Balances::reserve_b_swap_exact_output_a_to_b(),
                fees: Balances::fee_tier(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_swap_exact_output_a_to_b() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::reserve_a_swap_exact_output_a_to_b(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_swap_exact_output_a_to_b() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::reserve_b_swap_exact_output_a_to_b(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_swap_exact_output_a_to_b() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_swap_exact_output_a_to_b(),
            }),
            nonce: Nonce(1),
        }
    }

    fn user_b_holding_swap_exact_output_a_to_b() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_swap_exact_output_a_to_b(),
            }),
            nonce: Nonce(0),
        }
    }

    fn pool_definition_swap_exact_output_b_to_a() -> Account {
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
                reserve_a: Balances::reserve_a_swap_exact_output_b_to_a(),
                reserve_b: Balances::reserve_b_swap_exact_output_b_to_a(),
                fees: Balances::fee_tier(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_swap_exact_output_b_to_a() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::reserve_a_swap_exact_output_b_to_a(),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_b_swap_exact_output_b_to_a() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::reserve_b_swap_exact_output_b_to_a(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_a_holding_swap_exact_output_b_to_a() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_a_definition(),
                balance: Balances::user_a_swap_exact_output_b_to_a(),
            }),
            nonce: Nonce(0),
        }
    }

    fn user_b_holding_swap_exact_output_b_to_a() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_b_definition(),
                balance: Balances::user_b_swap_exact_output_b_to_a(),
            }),
            nonce: Nonce(1),
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
                fees: Balances::fee_tier(),
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
                authority: Some(Ids::token_lp_definition()),
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
                fees: Balances::fee_tier(),
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
                authority: Some(Ids::token_lp_definition()),
            }),
            nonce: Nonce(0),
        }
    }

    fn token_lp_definition_reinitializable() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenDefinition::Fungible {
                name: String::from("LP Token"),
                total_supply: 0,
                metadata_id: None,
                authority: Some(Ids::token_lp_definition()),
            }),
            nonce: Nonce(0),
        }
    }

    fn vault_a_reinitializable() -> Account {
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

    fn vault_b_reinitializable() -> Account {
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

    fn pool_definition_zero_supply_reinitializable() -> Account {
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
                fees: Balances::fee_tier(),
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
                balance: Balances::lp_user_init(),
            }),
            nonce: Nonce(1),
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
                authority: Some(Ids::token_lp_definition()),
            }),
            nonce: Nonce(0),
        }
    }

    fn lp_lock_holding_new_init() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: MINIMUM_LIQUIDITY,
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
                fees: Balances::fee_tier(),
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

    let twap_message =
        program_deployment_transaction::Message::new(twap_oracle_methods::TWAP_ORACLE_ELF.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
            twap_message,
        ))
        .expect("twap oracle program deployment must succeed");
}

fn state_for_amm_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::config(), Accounts::config());
    state.force_insert_account(Ids::pool_definition(), Accounts::pool_definition_init());
    // Seed the pool's current-tick account so swaps and syncs can refresh it. Its initial value is
    // the tick of the opening reserves; swap/sync tests assert it is updated to the new price.
    state.force_insert_account(
        Ids::current_tick_account(),
        Accounts::current_tick_account(initial_pool_tick()),
    );
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
    // rc6's `V03State::new()` no longer auto-creates the clock account; seed the canonical
    // 1-block clock so AMM ops that read it (current-tick refresh, TWAP observations) work.
    advance_clock(&mut state, 0);
    state
}

fn state_for_amm_tests_with_new_def() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    state.force_insert_account(Ids::config(), Accounts::config());
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
    // rc6's `V03State::new()` no longer auto-creates the clock account; seed the canonical
    // 1-block clock so the chained TWAP calls in `new_definition` can read it.
    advance_clock(&mut state, 0);
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

fn state_for_amm_tests_with_precreated_user_lp_for_new_def() -> V03State {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding_init_zero());
    state
}

#[cfg(test)]
fn try_execute_new_definition(
    state: &mut V03State,
    fees: u128,
    authorize_user_lp: bool,
) -> Result<(), LeeError> {
    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        fees,
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::lp_lock_holding(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        if authorize_user_lp {
            vec![
                current_nonce(state, Ids::user_a()),
                current_nonce(state, Ids::user_b()),
                current_nonce(state, Ids::user_lp()),
            ]
        } else {
            vec![
                current_nonce(state, Ids::user_a()),
                current_nonce(state, Ids::user_b()),
            ]
        },
        instruction,
    )
    .unwrap();

    let witness_set = if authorize_user_lp {
        public_transaction::WitnessSet::for_message(
            &message,
            &[&Keys::user_a(), &Keys::user_b(), &Keys::user_lp()],
        )
    } else {
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()])
    };

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

#[cfg(test)]
fn execute_new_definition(state: &mut V03State, fees: u128) {
    try_execute_new_definition(state, fees, true).unwrap();
}

#[cfg(test)]
fn execute_swap_a_to_b(state: &mut V03State, swap_amount_in: u128, min_amount_out: u128) {
    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::user_a())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

#[cfg(test)]
fn execute_swap_b_to_a(state: &mut V03State, swap_amount_in: u128, min_amount_out: u128) {
    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        deadline: u64::MAX,
    };

    // Token B is the input, so the B holding occupies the signed input slot.
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_b(),
            Ids::user_a(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::user_b())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

#[cfg(test)]
fn execute_add_liquidity(
    state: &mut V03State,
    min_amount_liquidity: u128,
    max_amount_to_add_token_a: u128,
    max_amount_to_add_token_b: u128,
) {
    let instruction = amm_core::Instruction::AddLiquidity {
        min_amount_liquidity,
        max_amount_to_add_token_a,
        max_amount_to_add_token_b,
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![
            current_nonce(state, Ids::user_a()),
            current_nonce(state, Ids::user_b()),
        ],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

#[cfg(test)]
fn execute_remove_liquidity(
    state: &mut V03State,
    remove_liquidity_amount: u128,
    min_amount_to_remove_token_a: u128,
    min_amount_to_remove_token_b: u128,
) {
    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount,
        min_amount_to_remove_token_a,
        min_amount_to_remove_token_b,
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::user_lp())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

#[cfg(test)]
fn execute_initialize(state: &mut V03State) {
    let instruction = amm_core::Instruction::Initialize {
        token_program_id: Ids::token_program(),
        twap_oracle_program_id: Ids::twap_oracle_program(),
        authority: Ids::admin(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![Ids::config()],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

#[cfg(test)]
fn execute_create_price_observations(
    state: &mut V03State,
    window_duration: u64,
) -> Result<(), LeeError> {
    let instruction = amm_core::Instruction::CreatePriceObservations { window_duration };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::current_tick_account(),
            Ids::price_observations(window_duration),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

#[cfg(test)]
fn execute_create_oracle_price_account(
    state: &mut V03State,
    window_duration: u64,
) -> Result<(), LeeError> {
    let instruction = amm_core::Instruction::CreateOraclePriceAccount { window_duration };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::oracle_price_account(window_duration),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

#[cfg(test)]
fn execute_sync_reserves(state: &mut V03State) {
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        amm_core::Instruction::SyncReserves,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

/// Builds a state whose pool was created through `new_definition`, which also creates the pool's
/// TWAP current-tick account (seeded from the opening reserves). Used by the observation tests so
/// they consume the real current-tick account rather than a hand-inserted one.
#[cfg(test)]
fn state_with_pool_created_via_new_definition() -> V03State {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());
    execute_new_definition(&mut state, Balances::fee_tier());
    state
}

fn fungible_balance(account: &Account) -> u128 {
    let holding = TokenHolding::try_from(&account.data).expect("expected token holding");
    let TokenHolding::Fungible {
        definition_id: _,
        balance,
    } = holding
    else {
        panic!("expected fungible token holding")
    };

    balance
}

fn pool_definition(account: &Account) -> PoolDefinition {
    PoolDefinition::try_from(&account.data).expect("expected pool definition")
}

fn initial_pool_tick() -> i32 {
    twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_init(),
        Balances::vault_b_init(),
    ))
}

fn assert_initial_swap_state(state: &V03State) {
    assert_eq!(state.get_account_by_id(Ids::config()), Accounts::config());
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_init()
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
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding()
    );
    assert_eq!(
        state.get_account_by_id(Ids::current_tick_account()),
        Accounts::current_tick_account(initial_pool_tick())
    );
}

fn assert_current_tick_matches_pool(state: &V03State) {
    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        pool.reserve_a,
        pool.reserve_b,
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

fn fungible_total_supply(account: &Account) -> u128 {
    let definition = TokenDefinition::try_from(&account.data).expect("expected token definition");
    let TokenDefinition::Fungible {
        name: _,
        total_supply,
        metadata_id: _,
        authority: _,
    } = definition
    else {
        panic!("expected fungible token definition")
    };

    total_supply
}

#[test]
fn amm_initialize_creates_config_account() {
    let mut state = V03State::new();
    deploy_programs(&mut state);

    // Before initialization the config PDA does not exist.
    assert_eq!(state.get_account_by_id(Ids::config()), Account::default());

    execute_initialize(&mut state);

    // Initialization creates the config PDA, owned by the AMM program.
    let config_account = state.get_account_by_id(Ids::config());
    assert_eq!(config_account, Accounts::config());

    // Explicitly assert the stored Token Program ID and admin authority round-trip from the
    // instruction arguments.
    let config = amm_core::AmmConfig::try_from(&config_account.data)
        .expect("config account must hold a valid AmmConfig");
    assert_eq!(config.token_program_id, Ids::token_program());
    assert_eq!(config.authority, Ids::admin());
}

#[cfg(test)]
fn execute_update_config(
    state: &mut V03State,
    signer: &PrivateKey,
    new_authority: AccountId,
) -> Result<(), LeeError> {
    let signer_id = AccountId::from(&PublicKey::new_from_private_key(signer));
    let instruction = amm_core::Instruction::UpdateConfig { new_authority };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![Ids::config(), signer_id],
        vec![current_nonce(state, signer_id)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[signer]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

fn config_data(state: &V03State) -> amm_core::AmmConfig {
    amm_core::AmmConfig::try_from(&state.get_account_by_id(Ids::config()).data)
        .expect("config account must hold a valid AmmConfig")
}

fn initialized_amm_state() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    execute_initialize(&mut state);
    state
}

#[test]
fn amm_update_config_transfers_authority_and_keeps_program_ids() {
    let mut state = initialized_amm_state();
    let new_admin = Ids::user_a();

    let before = config_data(&state);
    execute_update_config(&mut state, &Keys::admin(), new_admin).unwrap();
    let after = config_data(&state);

    assert_eq!(after.authority, new_admin);
    // The Token and TWAP oracle program IDs are immutable deployment parameters — the instruction
    // has no field to change them, so they are unaffected by an authority transfer.
    assert_eq!(after.token_program_id, before.token_program_id);
    assert_eq!(after.twap_oracle_program_id, before.twap_oracle_program_id);
}

#[test]
fn amm_update_config_rejects_non_admin() {
    let mut state = initialized_amm_state();

    // user_a is not the admin; even though they sign, the transfer is rejected and the config is
    // left unchanged.
    let result = execute_update_config(&mut state, &Keys::user_a(), Ids::user_a());
    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));

    assert_eq!(config_data(&state).authority, Ids::admin());
}

#[test]
fn amm_update_config_authority_handoff_revokes_old_admin() {
    let mut state = initialized_amm_state();
    let new_admin = Ids::user_a();

    // Admin hands off control to user_a.
    execute_update_config(&mut state, &Keys::admin(), new_admin).unwrap();
    assert_eq!(config_data(&state).authority, new_admin);

    // The original admin can no longer transfer authority.
    let result = execute_update_config(&mut state, &Keys::admin(), Ids::admin());
    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(config_data(&state).authority, new_admin);

    // The new admin can.
    execute_update_config(&mut state, &Keys::user_a(), Ids::admin()).unwrap();
    assert_eq!(config_data(&state).authority, Ids::admin());
}

#[test]
fn amm_creates_price_observations_on_twap_oracle() {
    let mut state = state_with_pool_created_via_new_definition();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // The current-tick account created during pool creation supplies the authoritative seed tick,
    // derived from the opening reserves (reserve_b / reserve_a as a Q64.64 spot price).
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_init(),
        Balances::vault_b_init(),
    ));

    // The observations PDA does not exist before the AMM creates it.
    assert_eq!(
        state.get_account_by_id(Ids::price_observations(window_duration)),
        Account::default()
    );

    execute_create_price_observations(&mut state, window_duration).unwrap();

    // The observations account now exists, is owned by the TWAP oracle program, and is seeded
    // with the pool as its price source and the tick read from the current-tick account.
    let account = state.get_account_by_id(Ids::price_observations(window_duration));
    assert_ne!(account, Account::default());
    assert_eq!(account.program_owner, Ids::twap_oracle_program());

    let feed = twap_oracle_core::PriceObservations::try_from(&account.data)
        .expect("observations account must hold a valid PriceObservations");
    assert_eq!(feed.price_source_id, Ids::pool_definition());
    assert_eq!(feed.last_recorded_tick, expected_tick);
    assert_eq!(feed.write_index, 1);
    assert_eq!(feed.total_entries, 1);
    assert_eq!(
        feed.entries.len(),
        usize::try_from(twap_oracle_core::OBSERVATIONS_CAPACITY)
            .expect("OBSERVATIONS_CAPACITY fits in usize")
    );

    // The AMM config and pool are left unchanged by the operation.
    assert_eq!(state.get_account_by_id(Ids::config()), Accounts::config());
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_new_init()
    );
}

#[test]
fn amm_creates_oracle_price_account_on_twap_oracle() {
    let mut state = state_with_pool_created_via_new_definition();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // CreateOraclePriceAccount rejects a zero clock timestamp, so advance the clock first.
    let now = 1_700_000_000_000u64;
    advance_clock(&mut state, now);

    // The price-account PDA does not exist before the AMM creates it.
    assert_eq!(
        state.get_account_by_id(Ids::oracle_price_account(window_duration)),
        Account::default()
    );

    execute_create_oracle_price_account(&mut state, window_duration).unwrap();

    // The price account now exists, is owned by the TWAP oracle program, and is seeded with the
    // pool as its source, the pool's asset pair, and the pool's current spot price (reserve_b /
    // reserve_a as a Q64.64) — all derived on-chain, none caller-supplied — stamped with the clock.
    let account = state.get_account_by_id(Ids::oracle_price_account(window_duration));
    assert_ne!(account, Account::default());
    assert_eq!(account.program_owner, Ids::twap_oracle_program());

    let price = twap_oracle_core::OraclePriceAccount::try_from(&account.data)
        .expect("price account must hold a valid OraclePriceAccount");
    assert_eq!(price.source_id, Ids::pool_definition());
    assert_eq!(price.base_asset, Ids::token_a_definition());
    assert_eq!(price.quote_asset, Ids::token_b_definition());
    assert_eq!(
        price.price,
        amm_core::spot_price_q64_64(Balances::vault_a_init(), Balances::vault_b_init())
    );
    assert_eq!(price.timestamp, now);
    assert_eq!(price.confidence_interval, 0);

    // The AMM config and pool are left unchanged by the operation.
    assert_eq!(state.get_account_by_id(Ids::config()), Accounts::config());
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_new_init()
    );
}

#[test]
fn amm_create_oracle_price_account_rejects_existing_account() {
    let mut state = state_with_pool_created_via_new_definition();
    let window_duration = 24 * 60 * 60 * 1_000u64;
    advance_clock(&mut state, 1_700_000_000_000u64);

    // First creation succeeds.
    execute_create_oracle_price_account(&mut state, window_duration).unwrap();
    let after_first = state.get_account_by_id(Ids::oracle_price_account(window_duration));

    // A second creation for the same (pool, window) is rejected and leaves the account intact.
    let result = execute_create_oracle_price_account(&mut state, window_duration);
    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(
        state.get_account_by_id(Ids::oracle_price_account(window_duration)),
        after_first
    );
}

#[test]
fn amm_create_price_observations_rejects_existing_account() {
    let mut state = state_with_pool_created_via_new_definition();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // First creation succeeds.
    execute_create_price_observations(&mut state, window_duration).unwrap();
    let feed_after_first = twap_oracle_core::PriceObservations::try_from(
        &state
            .get_account_by_id(Ids::price_observations(window_duration))
            .data,
    )
    .expect("observations account must hold a valid PriceObservations");

    // A second creation for the same (pool, window) is rejected because the observations account
    // already exists, and leaves the existing account intact.
    let result = execute_create_price_observations(&mut state, window_duration);
    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    let feed_after_second = twap_oracle_core::PriceObservations::try_from(
        &state
            .get_account_by_id(Ids::price_observations(window_duration))
            .data,
    )
    .expect("observations account must hold a valid PriceObservations");
    assert_eq!(feed_after_first, feed_after_second);
}

#[test]
fn amm_create_price_observations_without_current_tick_account_fails() {
    let mut state = state_for_amm_tests();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // Remove the seeded current-tick account: with no authoritative tick to seed from, creation
    // must be rejected.
    state.force_insert_account(Ids::current_tick_account(), Account::default());

    let result = execute_create_price_observations(&mut state, window_duration);
    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(
        state.get_account_by_id(Ids::price_observations(window_duration)),
        Account::default()
    );
}

/// Advances the canonical 1-block clock to `timestamp` by writing the clock account directly into
/// state. `RecordTick` reads this account (`CLOCK_01_PROGRAM_ACCOUNT_ID`), so the TWAP tests use it
/// to simulate the passage of time between observations.
///
/// rc6 moved the clock program out of `nssa` into the separate system-programs crate (gated behind
/// the guest-building `artifacts` feature), so the clock can no longer be ticked by submitting a
/// real clock transaction here. Instead we set the account state directly via
/// `force_insert_account`, matching how the upstream rc6 state-machine tests seed accounts.
#[cfg(test)]
fn advance_clock(state: &mut V03State, timestamp: u64) {
    let data = ClockAccountData {
        block_id: 0,
        timestamp,
    }
    .to_bytes();
    let clock_account = Account {
        // The real CLOCK_01 system account is owned by the clock program, not the
        // default program (see lee `system_accounts::clock_account`). A default owner
        // makes the spel-framework output filter drop the (unchanged, unclaimed)
        // clock post-state, which v0.2.1's DeclaredAccountMissingFromOutput invariant
        // then rejects. Use a non-default placeholder owner, as the oracle fixtures do.
        program_owner: [8u32; 8],
        data: Data::try_from(data).expect("clock account data fits"),
        ..Account::default()
    };
    state.force_insert_account(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_account);
}

/// Calls the TWAP oracle's permissionless `RecordTick` directly (it is not wrapped by the AMM),
/// folding the pool's current tick into its observations ring buffer for the given window.
#[cfg(test)]
fn execute_record_tick(state: &mut V03State, window_duration: u64) -> Result<(), LeeError> {
    let instruction = twap_oracle_core::Instruction::RecordTick {
        price_source_id: Ids::pool_definition(),
        window_duration,
    };

    let message = public_transaction::Message::try_new(
        Ids::twap_oracle_program(),
        vec![
            Ids::price_observations(window_duration),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

#[cfg(test)]
fn read_observations(
    state: &V03State,
    window_duration: u64,
) -> twap_oracle_core::PriceObservations {
    twap_oracle_core::PriceObservations::try_from(
        &state
            .get_account_by_id(Ids::price_observations(window_duration))
            .data,
    )
    .expect("observations account must hold a valid PriceObservations")
}

#[cfg(test)]
fn read_current_tick(state: &V03State) -> i32 {
    read_current_tick_account(state).tick
}

#[cfg(test)]
fn read_current_tick_account(state: &V03State) -> twap_oracle_core::CurrentTickAccount {
    twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount")
}

#[cfg(test)]
fn read_oracle_price(
    state: &V03State,
    window_duration: u64,
) -> twap_oracle_core::OraclePriceAccount {
    twap_oracle_core::OraclePriceAccount::try_from(
        &state
            .get_account_by_id(Ids::oracle_price_account(window_duration))
            .data,
    )
    .expect("oracle price account must hold a valid OraclePriceAccount")
}

/// Calls the oracle's permissionless `PublishPrice` directly (the AMM does not wrap it): computes
/// the TWAP from the pool's observations — extrapolating the tail from the current tick — and
/// writes it to the oracle price account.
#[cfg(test)]
fn execute_publish_price(state: &mut V03State, window_duration: u64) -> Result<(), LeeError> {
    let instruction = twap_oracle_core::Instruction::PublishPrice {
        price_source_id: Ids::pool_definition(),
        window_duration,
    };

    let message = public_transaction::Message::try_new(
        Ids::twap_oracle_program(),
        vec![
            Ids::price_observations(window_duration),
            Ids::oracle_price_account(window_duration),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

/// Builds a state whose pool feed already holds several observations: creates the observations
/// account, then (advance clock, swap, record) three times at 60s spacing — above the sampling
/// guard's ~42s minimum, so every record is accepted. The clock ends at 180_000 and the buffer
/// holds 4 entries (1 from creation + 3 recorded). Shared scaffolding for the `PublishPrice` tests.
#[cfg(test)]
fn state_with_recorded_window(window_duration: u64) -> V03State {
    let mut state = state_for_amm_tests();
    execute_create_price_observations(&mut state, window_duration).unwrap();
    for (step, swap) in [1u64, 2, 3]
        .into_iter()
        .zip([Swap::AtoB, Swap::AtoB, Swap::BtoA])
    {
        advance_clock(&mut state, step * 60_000);
        match swap {
            Swap::AtoB => execute_swap_a_to_b(&mut state, 1_000, 1),
            Swap::BtoA => execute_swap_b_to_a(&mut state, 1_000, 1),
        }
        execute_record_tick(&mut state, window_duration).unwrap();
    }
    state
}

/// End-to-end TWAP accumulation: a pool's price moves over time through real swaps, the oracle
/// folds each new tick into its observations buffer via `RecordTick`, and the time-weighted average
/// recovered from two snapshots matches the expected arithmetic mean of the intervening ticks.
///
/// This is the headline path the rest of the suite never exercises: every other test stops at the
/// freshly-created buffer (`write_index == 1`), so the accumulator math and the consult subtraction
/// are only proven here, through the zkVM-facing instruction interface.
#[test]
fn amm_twap_observations_accumulate_across_swaps_and_yield_time_weighted_average() {
    let mut state = state_for_amm_tests();
    let window_duration = 24 * 60 * 60 * 1_000u64;
    execute_create_price_observations(&mut state, window_duration).unwrap();

    // The creation observation sits at index 0 with a zero cumulative and the genesis timestamp.
    let created = read_observations(&state, window_duration);
    assert_eq!(created.write_index, 1);
    assert_eq!(created.total_entries, 1);
    assert_eq!(created.entries[0].timestamp, 0);
    assert_eq!(created.entries[0].tick_cumulative, 0);

    // Each step advances the clock by a fixed interval, moves the price with a swap, then records
    // the resulting tick. The interval (60s) is above the sampling guard's minimum
    // (window / OBSERVATIONS_CAPACITY ≈ 42s), so every record is accepted.
    let step_ms = 60_000u64;
    let mut prev_timestamp = 0u64;
    let mut prev_cumulative = 0i64;
    let mut prev_recorded_tick = created.last_recorded_tick;
    let mut snapshots: Vec<(u64, i32, i64)> = Vec::new();

    for (step, do_swap) in [1u64, 2, 3].into_iter().zip([
        // a->b, a->b (price keeps falling), then b->a (price rebounds), so the recorded ticks vary
        // in both directions across the window.
        Swap::AtoB,
        Swap::AtoB,
        Swap::BtoA,
    ]) {
        let now = step * step_ms;
        advance_clock(&mut state, now);
        match do_swap {
            Swap::AtoB => execute_swap_a_to_b(&mut state, 1_000, 1),
            Swap::BtoA => execute_swap_b_to_a(&mut state, 1_000, 1),
        }

        let current_tick = read_current_tick(&state);
        // Keep moves within the per-observation clamp so the integrated tick equals the raw tick;
        // the clamping path itself is covered by the oracle's unit tests.
        assert!(
            (current_tick - prev_recorded_tick).abs() <= twap_oracle_core::MAX_TICK_DELTA,
            "swap move must stay within MAX_TICK_DELTA for this test's exact-equality assertions"
        );

        execute_record_tick(&mut state, window_duration).unwrap();

        let elapsed = i64::try_from(now - prev_timestamp).unwrap();
        let expected_cumulative = prev_cumulative + i64::from(current_tick) * elapsed;

        let obs = read_observations(&state, window_duration);
        let written_index = usize::try_from(step).unwrap();
        assert_eq!(
            obs.total_entries,
            step + 1,
            "each accepted record appends exactly one entry"
        );
        assert_eq!(obs.write_index, u32::try_from(step + 1).unwrap());
        assert_eq!(obs.entries[written_index].timestamp, now);
        assert_eq!(
            obs.entries[written_index].tick_cumulative, expected_cumulative,
            "cumulative must advance by tick × elapsed_ms"
        );
        // last_recorded_tick tracks the raw tick for the next delta computation.
        assert_eq!(obs.last_recorded_tick, current_tick);

        snapshots.push((now, current_tick, expected_cumulative));
        prev_timestamp = now;
        prev_cumulative = expected_cumulative;
        prev_recorded_tick = current_tick;
    }

    // Consult the oracle the way a consumer would: the arithmetic-mean tick over [t1, t3] is the
    // difference of the two cumulative snapshots divided by the elapsed time.
    let (t1, _tick1, cum1) = snapshots[0];
    let (t3, _tick3, cum3) = snapshots[2];
    let time_weighted_tick = (cum3 - cum1) / i64::try_from(t3 - t1).unwrap();

    // With equal 60s intervals the time-weighted mean reduces to the plain average of the ticks
    // recorded at t2 and t3 (the two increments inside the (t1, t3] window).
    let tick2 = snapshots[1].1;
    let tick3 = snapshots[2].1;
    assert_eq!(
        time_weighted_tick,
        (i64::from(tick2) + i64::from(tick3)) / 2
    );

    // Sanity: the average lies between the extremes it was built from.
    let lo = i64::from(tick2.min(tick3));
    let hi = i64::from(tick2.max(tick3));
    assert!((lo..=hi).contains(&time_weighted_tick));
}

/// `RecordTick` is permissionless and may be called on every block; its sampling guard silently
/// skips writes until `window / OBSERVATIONS_CAPACITY` ms have elapsed. This drives that guard
/// through the real instruction path: a too-soon call is a no-op that also leaves the delta
/// baseline untouched, and a later call past the interval resumes recording.
#[test]
fn amm_twap_record_tick_sampling_guard_skips_calls_below_min_interval() {
    let mut state = state_for_amm_tests();
    let window_duration = 24 * 60 * 60 * 1_000u64;
    execute_create_price_observations(&mut state, window_duration).unwrap();

    let baseline = read_observations(&state, window_duration);
    let min_interval = window_duration / u64::from(twap_oracle_core::OBSERVATIONS_CAPACITY);

    // Move the price, then record well within the minimum interval: the guard must skip the write.
    advance_clock(&mut state, min_interval - 1);
    execute_swap_a_to_b(&mut state, 1_000, 1);
    execute_record_tick(&mut state, window_duration).unwrap();

    let after_skip = read_observations(&state, window_duration);
    assert_eq!(
        after_skip.write_index, baseline.write_index,
        "a too-soon record must not advance the ring buffer"
    );
    assert_eq!(after_skip.total_entries, baseline.total_entries);
    assert_eq!(
        after_skip.last_recorded_tick, baseline.last_recorded_tick,
        "the skipped record must not move the delta baseline either"
    );

    // Past the minimum interval the same call resumes recording.
    advance_clock(&mut state, min_interval + 1);
    let current_tick = read_current_tick(&state);
    execute_record_tick(&mut state, window_duration).unwrap();

    let after_write = read_observations(&state, window_duration);
    assert_eq!(after_write.write_index, baseline.write_index + 1);
    assert_eq!(after_write.total_entries, baseline.total_entries + 1);
    assert_eq!(after_write.last_recorded_tick, current_tick);
    let written = usize::try_from(baseline.write_index).unwrap();
    assert_eq!(after_write.entries[written].timestamp, min_interval + 1);
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum Swap {
    AtoB,
    BtoA,
}

/// `CreateOraclePriceAccount` end-to-end through the zkVM: a signing account acts as the authorized
/// price source (the AMM does not route this for a pool-owned source), and the oracle claims and
/// initializes the consumer-facing [`twap_oracle_core::OraclePriceAccount`] PDA.
#[test]
fn amm_twap_create_oracle_price_account() {
    let mut state = state_for_amm_tests();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // The price source must be authorized; a signing user provides that (a pool PDA cannot sign).
    let source = Ids::user_a();
    let price_account_id = twap_oracle_core::compute_oracle_price_account_pda(
        Ids::twap_oracle_program(),
        source,
        window_duration,
    );

    // CreateOraclePriceAccount rejects a zero clock timestamp, so move the clock forward first.
    advance_clock(&mut state, 5_000);

    let initial_price = 1u128 << 64; // Q64.64 1.0
    let instruction = twap_oracle_core::Instruction::CreateOraclePriceAccount {
        base_asset: Ids::token_a_definition(),
        quote_asset: Ids::token_b_definition(),
        initial_price,
        window_duration,
    };
    let message = public_transaction::Message::try_new(
        Ids::twap_oracle_program(),
        vec![price_account_id, source, CLOCK_01_PROGRAM_ACCOUNT_ID],
        vec![current_nonce(&state, source)],
        instruction,
    )
    .unwrap();
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    let account = state.get_account_by_id(price_account_id);
    assert_eq!(account.program_owner, Ids::twap_oracle_program());
    let price = twap_oracle_core::OraclePriceAccount::try_from(&account.data)
        .expect("oracle price account must hold a valid OraclePriceAccount");
    assert_eq!(price.price, initial_price);
    assert_eq!(price.timestamp, 5_000);
    assert_eq!(price.source_id, source);
    assert_eq!(price.base_asset, Ids::token_a_definition());
    assert_eq!(price.quote_asset, Ids::token_b_definition());
    assert_eq!(price.confidence_interval, 0);
}

/// End-to-end publish: with the clock at the newest observation the tail is empty, so the published
/// price is exactly the stored-window arithmetic-mean tick, converted to a `Q64.64` price. Proves
/// the full pipeline — observations built by real swaps + `RecordTick`, then consumed by
/// `PublishPrice` — composes through the zkVM-facing interface.
#[test]
fn amm_twap_publish_price_publishes_window_average() {
    let window_duration = 24 * 60 * 60 * 1_000u64;
    let mut state = state_with_recorded_window(window_duration);
    // Register the consumer-facing price account through the AMM (seeded with the pool's spot
    // price); PublishPrice overwrites its price/timestamp below.
    execute_create_oracle_price_account(&mut state, window_duration).unwrap();

    // The clock sits at the last record (180_000) == the newest observation's timestamp, so
    // [t2, now] is empty and the TWAP is the average over [t1, t2].
    let obs = read_observations(&state, window_duration);
    assert_eq!(obs.total_entries, 4);
    let t1 = &obs.entries[0];
    let t2 = &obs.entries[usize::try_from(obs.write_index).unwrap() - 1];
    let elapsed = i64::try_from(t2.timestamp - t1.timestamp).unwrap();
    let expected_tick = (t2.tick_cumulative - t1.tick_cumulative).div_euclid(elapsed);
    let expected_price =
        twap_oracle_core::tick_to_oracle_price(i32::try_from(expected_tick).unwrap());

    execute_publish_price(&mut state, window_duration).unwrap();

    let published = read_oracle_price(&state, window_duration);
    assert_eq!(
        published.timestamp, t2.timestamp,
        "publish stamps the price with now"
    );
    assert_eq!(published.price, expected_price);
    // Identity fields are untouched by publish.
    assert_eq!(published.source_id, Ids::pool_definition());
    assert_eq!(published.base_asset, Ids::token_a_definition());
}

/// End-to-end publish with an elapsed tail: the clock advances past the newest observation without
/// a new record, so `PublishPrice` must project the accumulator to `now` from the current tick. The
/// published timestamp is `now` (a fresh price, not a stale window), and the value reflects the
/// extrapolated tail.
#[test]
fn amm_twap_publish_price_extrapolates_tail_to_now() {
    let window_duration = 24 * 60 * 60 * 1_000u64;
    let mut state = state_with_recorded_window(window_duration);
    // Register the consumer-facing price account through the AMM (seeded with the pool's spot
    // price); PublishPrice overwrites its price/timestamp below.
    execute_create_oracle_price_account(&mut state, window_duration).unwrap();

    // Advance well past the newest observation (180_000) with no intervening record.
    let now = 240_000u64;
    advance_clock(&mut state, now);

    // Reproduce the tail split (see twap_oracle::publish_price): [t2, boundary] carries the tick
    // stored at t2, [boundary, now] carries the clamped live tick.
    let obs = read_observations(&state, window_duration);
    let ct = read_current_tick_account(&state);
    let t1 = &obs.entries[0];
    let t2 = &obs.entries[usize::try_from(obs.write_index).unwrap() - 1];
    let boundary = ct.last_updated.clamp(t2.timestamp, now);
    let pre_ms = i64::try_from(boundary - t2.timestamp).unwrap();
    let post_ms = i64::try_from(now - boundary).unwrap();
    let delta = (ct.tick - obs.last_recorded_tick).clamp(
        -twap_oracle_core::MAX_TICK_DELTA,
        twap_oracle_core::MAX_TICK_DELTA,
    );
    let clamped_tick = obs.last_recorded_tick + delta;
    let cum_now = t2.tick_cumulative
        + i64::from(obs.last_recorded_tick) * pre_ms
        + i64::from(clamped_tick) * post_ms;
    let elapsed = i64::try_from(now - t1.timestamp).unwrap();
    let expected_tick = (cum_now - t1.tick_cumulative).div_euclid(elapsed);
    let expected_price =
        twap_oracle_core::tick_to_oracle_price(i32::try_from(expected_tick).unwrap());

    execute_publish_price(&mut state, window_duration).unwrap();

    let published = read_oracle_price(&state, window_duration);
    assert_eq!(
        published.timestamp, now,
        "publish must project the timestamp forward to now"
    );
    assert_eq!(published.price, expected_price);
}

/// `PublishPrice` is a no-op when fewer than two observations exist: there is nothing to average,
/// so the price account is left untouched (consumers keep seeing its prior value).
#[test]
fn amm_twap_publish_price_noop_with_fewer_than_two_observations() {
    let mut state = state_for_amm_tests();
    let window_duration = 24 * 60 * 60 * 1_000u64;

    // Register the feed and its price account through the AMM. The clock must be non-zero for
    // CreateOraclePriceAccount, so advance it first; the observations feed keeps only its single
    // creation entry (no RecordTick), so PublishPrice has nothing to average.
    advance_clock(&mut state, 100_000);
    execute_create_price_observations(&mut state, window_duration).unwrap(); // total_entries == 1
    execute_create_oracle_price_account(&mut state, window_duration).unwrap();
    let seeded = read_oracle_price(&state, window_duration);

    // Time passes, but the feed still has only the creation observation.
    advance_clock(&mut state, 500_000);
    execute_publish_price(&mut state, window_duration).unwrap();

    assert_eq!(
        read_oracle_price(&state, window_duration),
        seeded,
        "publish with < 2 observations must leave the price account unchanged"
    );
}

#[test]
fn amm_remove_liquidity() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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

    // Removing liquidity also refreshes the pool's TWAP current tick to the post-removal spot
    // price.
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_remove(),
        Balances::vault_b_remove(),
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

#[test]
fn amm_remove_liquidity_insufficient_user_lp_fails() {
    let mut state = state_for_amm_tests();
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding_with_balance(500));

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);

    let tx = PublicTransaction::new(message, witness_set);
    assert!(state.transition_from_public_transaction(&tx, 0, 0).is_err());
}

#[test]
fn amm_new_definition_uninitialized_pool() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

    execute_new_definition(&mut state, Balances::fee_tier());

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
        state.get_account_by_id(Ids::lp_lock_holding()),
        Accounts::lp_lock_holding_new_init()
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

    // Pool creation also created the pool's TWAP current-tick account (a chained call to the
    // oracle), owned by the oracle program and seeded with the tick derived from the opening
    // reserves (reserve_b / reserve_a as a Q64.64 spot price).
    let current_tick = state.get_account_by_id(Ids::current_tick_account());
    assert_eq!(current_tick.program_owner, Ids::twap_oracle_program());
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(&current_tick.data)
        .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_init(),
        Balances::vault_b_init(),
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

#[test]
fn amm_new_definition_without_user_lp_authorization_fails() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

    let result = try_execute_new_definition(&mut state, Balances::fee_tier(), false);

    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::lp_lock_holding()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding()
    );
    assert_eq!(state.get_account_by_id(Ids::user_lp()), Account::default());
}

#[test]
fn amm_new_definition_precreated_user_lp_unsigned_fails() {
    // `user_holding_lp` is now a required signer: even a pre-existing (non-default)
    // LP holding must be signed. An unsigned transaction is rejected and reverts.
    let mut state = state_for_amm_tests_with_precreated_user_lp_for_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

    let result = try_execute_new_definition(&mut state, Balances::fee_tier(), false);

    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::lp_lock_holding()),
        Account::default()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_init_zero()
    );
}

#[test]
fn amm_new_definition_supports_all_fee_tiers() {
    for fees in [
        FEE_TIER_BPS_1,
        FEE_TIER_BPS_5,
        FEE_TIER_BPS_30,
        FEE_TIER_BPS_100,
    ] {
        let mut state = state_for_amm_tests_with_new_def();
        state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
        state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

        execute_new_definition(&mut state, fees);

        let pool_definition =
            PoolDefinition::try_from(&state.get_account_by_id(Ids::pool_definition()).data)
                .expect("new definition should create a valid pool");
        assert_eq!(pool_definition.fees, fees);
    }
}

#[test]
fn amm_new_definition_rejects_unsupported_fee_tier_transaction() {
    let mut state = state_for_amm_tests_with_precreated_user_lp_for_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());
    state.force_insert_account(
        Ids::pool_definition(),
        Accounts::pool_definition_zero_supply_reinitializable(),
    );
    state.force_insert_account(
        Ids::token_lp_definition(),
        Accounts::token_lp_definition_reinitializable(),
    );

    // `user_holding_lp` is signed so the rejection isolates the unsupported fee tier.
    let result = try_execute_new_definition(&mut state, 2, true);

    assert!(matches!(result, Err(LeeError::ProgramExecutionFailed(_))));
    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_zero_supply_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::token_lp_definition()),
        Accounts::token_lp_definition_reinitializable()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_lp()),
        Accounts::user_lp_holding_init_zero()
    );
}

#[test]
fn amm_add_liquidity() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::AddLiquidity {
        min_amount_liquidity: Balances::add_min_lp(),
        max_amount_to_add_token_a: Balances::add_max_a(),
        max_amount_to_add_token_b: Balances::add_max_b(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0), Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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

    // Adding liquidity also refreshes the pool's TWAP current tick to the post-add spot price.
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_add(),
        Balances::vault_b_add(),
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

#[test]
fn amm_swap_b_to_a() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        deadline: u64::MAX,
    };

    // Token B is the input, so the B holding occupies the signed input slot.
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_b(),
            Ids::user_a(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

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

    // The swap refreshed the pool's TWAP current tick to the post-swap spot price.
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::reserve_a_swap_2(),
        Balances::reserve_b_swap_2(),
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

#[test]
fn amm_swap_exact_output_refreshes_current_tick() {
    let mut state = state_for_amm_tests();

    let initial_tick = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount")
    .tick;

    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: Balances::swap_min_out(),
        max_amount_in: Balances::swap_amount_in(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_swap_exact_output_a_to_b()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_swap_exact_output_a_to_b()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_swap_exact_output_a_to_b()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_swap_exact_output_a_to_b()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_swap_exact_output_a_to_b()
    );

    // The swap refreshed the pool's TWAP current tick to the post-swap spot price, computed from
    // the reserves the swap actually settled on.
    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        pool.reserve_a,
        pool.reserve_b,
    ));
    assert_eq!(tick_account.tick, expected_tick);
    assert_ne!(
        tick_account.tick, initial_tick,
        "swap should have moved the current tick"
    );
}

#[test]
fn amm_swap_exact_output_b_to_a_signs_only_input() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: Balances::swap_min_out(),
        max_amount_in: Balances::swap_amount_in(),
        deadline: u64::MAX,
    };

    // Token B is the input, so the B holding occupies the signed input slot; only it is signed.
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_b(),
            Ids::user_a(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();

    assert_eq!(
        state.get_account_by_id(Ids::pool_definition()),
        Accounts::pool_definition_swap_exact_output_b_to_a()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_a()),
        Accounts::vault_a_swap_exact_output_b_to_a()
    );
    assert_eq!(
        state.get_account_by_id(Ids::vault_b()),
        Accounts::vault_b_swap_exact_output_b_to_a()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_a()),
        Accounts::user_a_holding_swap_exact_output_b_to_a()
    );
    assert_eq!(
        state.get_account_by_id(Ids::user_b()),
        Accounts::user_b_holding_swap_exact_output_b_to_a()
    );

    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    let required_input = pool
        .reserve_b
        .checked_sub(Balances::vault_b_init())
        .expect("swap should increase input-token reserve");
    let user_a_balance = fungible_balance(&state.get_account_by_id(Ids::user_a()));
    let user_b_balance = fungible_balance(&state.get_account_by_id(Ids::user_b()));

    assert_eq!(
        pool.reserve_a,
        Balances::vault_a_init() - Balances::swap_min_out()
    );
    assert_ne!(required_input, 0);
    assert!(required_input <= Balances::swap_amount_in());
    assert_eq!(
        Balances::user_b_init() - user_b_balance,
        required_input,
        "user debit must equal pool input reserve increase"
    );
    assert_eq!(
        user_a_balance,
        Balances::user_a_init() + Balances::swap_min_out()
    );
    assert_current_tick_matches_pool(&state);
}

#[test]
fn amm_swap_exact_input_requires_input_signature() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, 0),
        Err(LeeError::ProgramExecutionFailed(_))
    ));

    assert_initial_swap_state(&state);
}

#[test]
fn amm_swap_exact_input_rejects_when_only_output_holding_signed() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        deadline: u64::MAX,
    };

    // Input slot holds token A but only the output (B) holding is signed: the input is unsigned, so
    // the swap must be rejected.
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, 0),
        Err(LeeError::ProgramExecutionFailed(_))
    ));

    assert_initial_swap_state(&state);
}

#[test]
fn amm_swap_exact_output_requires_input_signature() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: Balances::swap_min_out(),
        max_amount_in: Balances::swap_amount_in(),
        deadline: u64::MAX,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, 0),
        Err(LeeError::ProgramExecutionFailed(_))
    ));

    assert_initial_swap_state(&state);
}

#[test]
fn amm_swap_exact_output_rejects_when_only_output_holding_signed() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: Balances::swap_min_out(),
        max_amount_in: Balances::swap_amount_in(),
        deadline: u64::MAX,
    };

    // Input slot holds token A but only the output (B) holding is signed: the input is unsigned, so
    // the swap must be rejected.
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, 0),
        Err(LeeError::ProgramExecutionFailed(_))
    ));

    assert_initial_swap_state(&state);
}

#[test]
fn amm_sync_reserves_updates_pool_and_current_tick() {
    let mut state = state_for_amm_tests();

    // Donate token A straight into vault A, so the vault balance exceeds the recorded reserve.
    let donation = 1_000u128;
    let mut donated_vault_a = Accounts::vault_a_init();
    donated_vault_a.data = Data::from(&TokenHolding::Fungible {
        definition_id: Ids::token_a_definition(),
        balance: Balances::vault_a_init() + donation,
    });
    state.force_insert_account(Ids::vault_a(), donated_vault_a);

    execute_sync_reserves(&mut state);

    // Sync reconciles the pool reserves with the actual vault balances.
    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(pool.reserve_a, Balances::vault_a_init() + donation);
    assert_eq!(pool.reserve_b, Balances::vault_b_init());

    // And refreshes the current tick to the synced spot price.
    let tick_account = twap_oracle_core::CurrentTickAccount::try_from(
        &state.get_account_by_id(Ids::current_tick_account()).data,
    )
    .expect("current tick account must hold a valid CurrentTickAccount");
    let expected_tick = twap_oracle_core::price_to_tick(amm_core::spot_price_q64_64(
        Balances::vault_a_init() + donation,
        Balances::vault_b_init(),
    ));
    assert_eq!(tick_account.tick, expected_tick);
}

#[test]
fn amm_fee_accumulates_across_multiple_swaps_and_pays_out_on_remove() {
    let mut state = state_for_amm_tests();

    execute_swap_a_to_b(&mut state, 1_000, 200);
    execute_swap_b_to_a(&mut state, 1_000, 200);

    let pool_before_remove = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(pool_before_remove.reserve_a, 4_060);
    assert_eq!(pool_before_remove.reserve_b, 3_085);
    assert_eq!(pool_before_remove.fees, Balances::fee_tier());

    let vault_a_before_remove = fungible_balance(&state.get_account_by_id(Ids::vault_a()));
    let vault_b_before_remove = fungible_balance(&state.get_account_by_id(Ids::vault_b()));
    assert_eq!(vault_a_before_remove, 4_060);
    assert_eq!(vault_b_before_remove, 3_085);
    assert_eq!(vault_a_before_remove, pool_before_remove.reserve_a);
    assert_eq!(vault_b_before_remove, pool_before_remove.reserve_b);

    execute_remove_liquidity(&mut state, 1_000, 812, 617);

    let pool_after_remove = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(pool_after_remove.reserve_a, 3_248);
    assert_eq!(pool_after_remove.reserve_b, 2_468);
    assert_eq!(pool_after_remove.liquidity_pool_supply, 4_000);

    let vault_a_after_remove = fungible_balance(&state.get_account_by_id(Ids::vault_a()));
    let vault_b_after_remove = fungible_balance(&state.get_account_by_id(Ids::vault_b()));
    assert_eq!(vault_a_after_remove, 3_248);
    assert_eq!(vault_b_after_remove, 2_468);
    assert_eq!(vault_a_after_remove, pool_after_remove.reserve_a);
    assert_eq!(vault_b_after_remove, pool_after_remove.reserve_b);

    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_a())),
        11_752
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_b())),
        10_032
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_lp())),
        1_000
    );
    assert_eq!(
        fungible_total_supply(&state.get_account_by_id(Ids::token_lp_definition())),
        4_000
    );
}

#[test]
fn amm_swap_rejects_expired_deadline() {
    let mut state = state_for_amm_tests();

    let deadline_ms = 1_000u64;
    let block_timestamp_ms = 2_000u64;

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        deadline: deadline_ms,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![Nonce(0)],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, block_timestamp_ms),
        Err(LeeError::OutOfValidityWindow)
    ));
}

#[test]
fn amm_swap_exact_output_rejects_expired_deadline() {
    let mut state = state_for_amm_tests();

    let deadline_ms = 1_000u64;
    let block_timestamp_ms = 2_000u64;

    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: Balances::swap_min_out(),
        max_amount_in: Balances::swap_amount_in(),
        deadline: deadline_ms,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(&state, Ids::user_a())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, block_timestamp_ms),
        Err(LeeError::OutOfValidityWindow)
    ));
}

#[test]
fn amm_add_liquidity_rejects_expired_deadline() {
    let mut state = state_for_amm_tests();

    let deadline_ms = 1_000u64;
    let block_timestamp_ms = 2_000u64;

    let instruction = amm_core::Instruction::AddLiquidity {
        min_amount_liquidity: Balances::add_min_lp(),
        max_amount_to_add_token_a: Balances::add_max_a(),
        max_amount_to_add_token_b: Balances::add_max_b(),
        deadline: deadline_ms,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![
            current_nonce(&state, Ids::user_a()),
            current_nonce(&state, Ids::user_b()),
        ],
        instruction,
    )
    .unwrap();

    let witness_set =
        public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a(), &Keys::user_b()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, block_timestamp_ms),
        Err(LeeError::OutOfValidityWindow)
    ));
}

#[test]
fn amm_remove_liquidity_rejects_expired_deadline() {
    let mut state = state_for_amm_tests();

    let deadline_ms = 1_000u64;
    let block_timestamp_ms = 2_000u64;

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        deadline: deadline_ms,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(&state, Ids::user_lp())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, block_timestamp_ms),
        Err(LeeError::OutOfValidityWindow)
    ));
}

#[test]
fn amm_new_definition_rejects_expired_deadline() {
    let mut state = state_for_amm_tests_with_precreated_user_lp_for_new_def();

    let deadline_ms = 1_000u64;
    let block_timestamp_ms = 2_000u64;

    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        fees: amm_core::FEE_TIER_BPS_30,
        deadline: deadline_ms,
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::config(),
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::lp_lock_holding(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
            Ids::current_tick_account(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![
            current_nonce(&state, Ids::user_a()),
            current_nonce(&state, Ids::user_b()),
            current_nonce(&state, Ids::user_lp()),
        ],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(
        &message,
        &[&Keys::user_a(), &Keys::user_b(), &Keys::user_lp()],
    );
    let tx = PublicTransaction::new(message, witness_set);
    assert!(matches!(
        state.transition_from_public_transaction(&tx, 0, block_timestamp_ms),
        Err(LeeError::OutOfValidityWindow)
    ));
}

#[test]
fn amm_add_liquidity_after_fee_accrual() {
    let mut state = state_for_amm_tests();

    execute_swap_a_to_b(&mut state, 1_000, 200);
    execute_swap_b_to_a(&mut state, 1_000, 200);
    execute_swap_a_to_b(&mut state, 1_000, 200);
    execute_swap_b_to_a(&mut state, 1_000, 200);

    let pool_before_add = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    let vault_a_before_add = fungible_balance(&state.get_account_by_id(Ids::vault_a()));
    let vault_b_before_add = fungible_balance(&state.get_account_by_id(Ids::vault_b()));

    assert_eq!(pool_before_add.reserve_a, 3_608);
    assert_eq!(pool_before_add.reserve_b, 3_477);
    assert_eq!(vault_a_before_add, pool_before_add.reserve_a);
    assert_eq!(vault_b_before_add, pool_before_add.reserve_b);

    execute_add_liquidity(&mut state, 1_436, 2_000, 1_000);

    let pool_after_add = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    let vault_a_after_add = fungible_balance(&state.get_account_by_id(Ids::vault_a()));
    let vault_b_after_add = fungible_balance(&state.get_account_by_id(Ids::vault_b()));

    assert_eq!(pool_after_add.reserve_a, 4_645);
    assert_eq!(pool_after_add.reserve_b, 4_477);
    assert_eq!(pool_after_add.liquidity_pool_supply, 6_437);
    assert_eq!(vault_a_after_add, pool_after_add.reserve_a);
    assert_eq!(vault_b_after_add, pool_after_add.reserve_b);

    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_a())),
        10_355
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_b())),
        8_023
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::user_lp())),
        3_437
    );
    assert_eq!(
        fungible_total_supply(&state.get_account_by_id(Ids::token_lp_definition())),
        6_437
    );
}
