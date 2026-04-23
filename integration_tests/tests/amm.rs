use amm_core::{
    PoolDefinition, FEE_BPS_DENOMINATOR, FEE_TIER_BPS_1, FEE_TIER_BPS_100, FEE_TIER_BPS_30,
    FEE_TIER_BPS_5, MINIMUM_LIQUIDITY,
};
use ata_core::{compute_ata_seed, get_associated_token_account_id};
use nssa::{
    error::NssaError,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use token_core::{TokenDefinition, TokenHolding};

struct Keys;
struct Ids;
struct Balances;
struct Accounts;

impl Keys {
    fn owner() -> PrivateKey {
        PrivateKey::try_new([30; 32]).expect("valid private key")
    }

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

    fn ata_program() -> nssa_core::program::ProgramId {
        ata_methods::ATA_ID
    }

    fn malicious_ata_program() -> ProgramId {
        malicious_ata_methods::MALICIOUS_ATA_ID
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

    fn owner() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::owner()))
    }

    fn owner_token_a_ata() -> AccountId {
        get_associated_token_account_id(
            &Self::ata_program(),
            &compute_ata_seed(Self::owner(), Self::token_a_definition()),
        )
    }

    fn owner_token_b_ata() -> AccountId {
        get_associated_token_account_id(
            &Self::ata_program(),
            &compute_ata_seed(Self::owner(), Self::token_b_definition()),
        )
    }

    fn owner_token_lp_ata() -> AccountId {
        get_associated_token_account_id(
            &Self::ata_program(),
            &compute_ata_seed(Self::owner(), Self::token_lp_definition()),
        )
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
    fn owner() -> Account {
        Account {
            program_owner: ProgramId::default(),
            balance: 0_u128,
            data: Data::default(),
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

    fn owner_token_a_ata() -> Account {
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

    fn owner_token_b_ata() -> Account {
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

    fn owner_token_lp_ata() -> Account {
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

    fn user_lp_holding_new_init_precreated() -> Account {
        Account {
            program_owner: Ids::token_program(),
            balance: 0_u128,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: Ids::token_lp_definition(),
                balance: Balances::lp_user_init(),
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

fn deploy_program(state: &mut V03State, elf: &[u8], name: &str) {
    let message = program_deployment_transaction::Message::new(elf.to_vec());
    state
        .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(message))
        .unwrap_or_else(|_| panic!("{name} program deployment must succeed"));
}

fn deploy_programs(state: &mut V03State) {
    deploy_program(state, token_methods::TOKEN_ELF, "token");
    deploy_program(state, amm_methods::AMM_ELF, "amm");
}

fn deploy_programs_with_malicious_ata(state: &mut V03State) {
    deploy_programs(state);
    deploy_program(
        state,
        malicious_ata_methods::MALICIOUS_ATA_ELF,
        "malicious ata",
    );
}

fn state_for_amm_tests() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[], 0);
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
    let mut state = V03State::new_with_genesis_accounts(&[], &[], 0);
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

fn state_for_malicious_ata_attack() -> V03State {
    let mut state = V03State::new_with_genesis_accounts(&[], &[], 0);
    deploy_programs_with_malicious_ata(&mut state);
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
    state.force_insert_account(Ids::owner(), Accounts::owner());
    state.force_insert_account(Ids::owner_token_a_ata(), Accounts::owner_token_a_ata());
    state.force_insert_account(Ids::owner_token_b_ata(), Accounts::owner_token_b_ata());
    state.force_insert_account(Ids::owner_token_lp_ata(), Accounts::owner_token_lp_ata());
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_init());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_init());
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

fn try_execute_amm_as_owner(
    state: &mut V03State,
    instruction: amm_core::Instruction,
    accounts: Vec<AccountId>,
) -> Result<(), NssaError> {
    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        accounts,
        vec![current_nonce(state, Ids::owner())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::owner()]);
    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0)
}

fn state_for_amm_tests_with_precreated_user_lp_for_new_def() -> V03State {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding_init_zero());
    state
}

fn try_execute_new_definition(
    state: &mut V03State,
    fees: u128,
    authorize_user_lp: bool,
) -> Result<(), NssaError> {
    let instruction = amm_core::Instruction::NewDefinition {
        token_a_amount: Balances::vault_a_init(),
        token_b_amount: Balances::vault_b_init(),
        fees,
        amm_program_id: Ids::amm_program(),
    };

    let message = public_transaction::Message::try_new(
        Ids::amm_program(),
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::lp_lock_holding(),
            Ids::user_a(),
            Ids::user_b(),
            Ids::user_lp(),
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

fn execute_new_definition(state: &mut V03State, fees: u128) {
    try_execute_new_definition(state, fees, true).unwrap();
}

fn execute_swap_a_to_b(state: &mut V03State, swap_amount_in: u128, min_amount_out: u128) {
    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        token_definition_id_in: Ids::token_a_definition(),
        ata_program_id: Ids::ata_program(),
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
        vec![current_nonce(state, Ids::user_a())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_a()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

fn execute_swap_b_to_a(state: &mut V03State, swap_amount_in: u128, min_amount_out: u128) {
    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        token_definition_id_in: Ids::token_b_definition(),
        ata_program_id: Ids::ata_program(),
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
        vec![current_nonce(state, Ids::user_b())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_b()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
}

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
        ata_program_id: Ids::ata_program(),
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
        ata_program_id: Ids::ata_program(),
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
        vec![current_nonce(state, Ids::user_lp())],
        instruction,
    )
    .unwrap();

    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::user_lp()]);

    let tx = PublicTransaction::new(message, witness_set);
    state.transition_from_public_transaction(&tx, 0, 0).unwrap();
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

fn fungible_total_supply(account: &Account) -> u128 {
    let definition = TokenDefinition::try_from(&account.data).expect("expected token definition");
    let TokenDefinition::Fungible {
        name: _,
        total_supply,
        metadata_id: _,
    } = definition
    else {
        panic!("expected fungible token definition")
    };

    total_supply
}

fn exact_output_required_amount_in(
    reserve_in: u128,
    reserve_out: u128,
    exact_amount_out: u128,
    fee_bps: u128,
) -> u128 {
    let effective_in_min = reserve_in
        .checked_mul(exact_amount_out)
        .expect("reserve_in * exact_amount_out overflows")
        .div_ceil(
            reserve_out
                .checked_sub(exact_amount_out)
                .expect("exact_amount_out must stay below reserve_out"),
        );
    let fee_multiplier = FEE_BPS_DENOMINATOR
        .checked_sub(fee_bps)
        .expect("fee_bps exceeds denominator");

    effective_in_min
        .checked_mul(FEE_BPS_DENOMINATOR)
        .expect("effective_in_min * denominator overflows")
        .div_ceil(fee_multiplier)
}

fn add_liquidity_malicious_ata_attack_witness() -> Option<&'static str> {
    let mut state = state_for_malicious_ata_attack();
    let instruction = amm_core::Instruction::AddLiquidity {
        min_amount_liquidity: Balances::add_min_lp(),
        max_amount_to_add_token_a: Balances::add_max_a(),
        max_amount_to_add_token_b: Balances::add_max_b(),
        ata_program_id: Ids::malicious_ata_program(),
    };

    if try_execute_amm_as_owner(
        &mut state,
        instruction,
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::owner(),
            Ids::owner_token_a_ata(),
            Ids::owner_token_b_ata(),
            Ids::owner_token_lp_ata(),
        ],
    )
    .is_err()
    {
        return None;
    }

    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(pool.liquidity_pool_supply, Balances::token_lp_supply_add());
    assert_eq!(pool.reserve_a, Balances::vault_a_add());
    assert_eq!(pool.reserve_b, Balances::vault_b_add());
    assert_eq!(
        fungible_total_supply(&state.get_account_by_id(Ids::token_lp_definition())),
        Balances::token_lp_supply_add()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_lp_ata())),
        Balances::user_lp_add()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_a_ata())),
        Balances::user_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_b_ata())),
        Balances::user_b_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_a())),
        Balances::vault_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_b())),
        Balances::vault_b_init()
    );

    Some(
        "add_liquidity: LP supply and owner LP balance increase while both deposit legs leave balances unchanged",
    )
}

fn remove_liquidity_malicious_ata_attack_witness() -> Option<&'static str> {
    let mut state = state_for_malicious_ata_attack();
    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        ata_program_id: Ids::malicious_ata_program(),
    };

    if try_execute_amm_as_owner(
        &mut state,
        instruction,
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::token_lp_definition(),
            Ids::owner(),
            Ids::owner_token_a_ata(),
            Ids::owner_token_b_ata(),
            Ids::owner_token_lp_ata(),
        ],
    )
    .is_err()
    {
        return None;
    }

    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(
        pool.liquidity_pool_supply,
        Balances::token_lp_supply_remove()
    );
    assert_eq!(pool.reserve_a, Balances::vault_a_remove());
    assert_eq!(pool.reserve_b, Balances::vault_b_remove());
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_a_ata())),
        Balances::user_a_remove()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_b_ata())),
        Balances::user_b_remove()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_a())),
        Balances::vault_a_remove()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_b())),
        Balances::vault_b_remove()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_lp_ata())),
        Balances::user_lp_init()
    );
    assert_eq!(
        fungible_total_supply(&state.get_account_by_id(Ids::token_lp_definition())),
        Balances::token_lp_supply()
    );

    Some(
        "remove_liquidity: owner receives vault tokens while LP balance and LP definition supply stay unchanged",
    )
}

fn swap_exact_input_malicious_ata_attack_witness() -> Option<&'static str> {
    let mut state = state_for_malicious_ata_attack();
    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        token_definition_id_in: Ids::token_a_definition(),
        ata_program_id: Ids::malicious_ata_program(),
    };

    if try_execute_amm_as_owner(
        &mut state,
        instruction,
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::owner(),
            Ids::owner_token_a_ata(),
            Ids::owner_token_b_ata(),
        ],
    )
    .is_err()
    {
        return None;
    }

    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(pool.reserve_a, Balances::reserve_a_swap_2());
    assert_eq!(pool.reserve_b, Balances::reserve_b_swap_2());
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_a_ata())),
        Balances::user_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_b_ata())),
        Balances::user_b_swap_2()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_a())),
        Balances::vault_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_b())),
        Balances::vault_b_swap_2()
    );

    Some(
        "swap_exact_input: owner receives output while the input balance and deposit vault stay unchanged",
    )
}

fn swap_exact_output_malicious_ata_attack_witness() -> Option<&'static str> {
    const EXACT_AMOUNT_OUT: u128 = 500;
    const MAX_AMOUNT_IN: u128 = 2_000;

    let mut state = state_for_malicious_ata_attack();
    let instruction = amm_core::Instruction::SwapExactOutput {
        exact_amount_out: EXACT_AMOUNT_OUT,
        max_amount_in: MAX_AMOUNT_IN,
        token_definition_id_in: Ids::token_a_definition(),
        ata_program_id: Ids::malicious_ata_program(),
    };

    if try_execute_amm_as_owner(
        &mut state,
        instruction,
        vec![
            Ids::pool_definition(),
            Ids::vault_a(),
            Ids::vault_b(),
            Ids::owner(),
            Ids::owner_token_a_ata(),
            Ids::owner_token_b_ata(),
        ],
    )
    .is_err()
    {
        return None;
    }

    let required_amount_in = exact_output_required_amount_in(
        Balances::vault_a_init(),
        Balances::vault_b_init(),
        EXACT_AMOUNT_OUT,
        Balances::fee_tier(),
    );
    let pool = pool_definition(&state.get_account_by_id(Ids::pool_definition()));
    assert_eq!(
        pool.reserve_a,
        Balances::vault_a_init() + required_amount_in
    );
    assert_eq!(pool.reserve_b, Balances::vault_b_init() - EXACT_AMOUNT_OUT);
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_a_ata())),
        Balances::user_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::owner_token_b_ata())),
        Balances::user_b_init() + EXACT_AMOUNT_OUT
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_a())),
        Balances::vault_a_init()
    );
    assert_eq!(
        fungible_balance(&state.get_account_by_id(Ids::vault_b())),
        Balances::vault_b_init() - EXACT_AMOUNT_OUT
    );

    Some(
        "swap_exact_output: owner receives exact output while the required input balance and deposit vault stay unchanged",
    )
}

#[test]
fn amm_rejects_malicious_ata_program_for_all_value_paths() {
    let accepted_attacks = [
        add_liquidity_malicious_ata_attack_witness(),
        remove_liquidity_malicious_ata_attack_witness(),
        swap_exact_input_malicious_ata_attack_witness(),
        swap_exact_output_malicious_ata_attack_witness(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    assert!(
        accepted_attacks.is_empty(),
        "AMM accepted a malicious ATA program for value paths: {}",
        accepted_attacks.join("; ")
    );
}

#[test]
fn amm_remove_liquidity() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        ata_program_id: Ids::ata_program(),
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
}

#[test]
fn amm_remove_liquidity_insufficient_user_lp_fails() {
    let mut state = state_for_amm_tests();
    state.force_insert_account(Ids::user_lp(), Accounts::user_lp_holding_with_balance(500));

    let instruction = amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: Balances::remove_lp(),
        min_amount_to_remove_token_a: Balances::remove_min_a(),
        min_amount_to_remove_token_b: Balances::remove_min_b(),
        ata_program_id: Ids::ata_program(),
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
}

#[test]
fn amm_new_definition_without_user_lp_authorization_fails() {
    let mut state = state_for_amm_tests_with_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

    let result = try_execute_new_definition(&mut state, Balances::fee_tier(), false);

    assert!(matches!(result, Err(NssaError::ProgramExecutionFailed(_))));
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
fn amm_new_definition_precreated_zero_balance_user_lp() {
    let mut state = state_for_amm_tests_with_precreated_user_lp_for_new_def();
    state.force_insert_account(Ids::vault_a(), Accounts::vault_a_reinitializable());
    state.force_insert_account(Ids::vault_b(), Accounts::vault_b_reinitializable());

    try_execute_new_definition(&mut state, Balances::fee_tier(), false).unwrap();

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
        Accounts::user_lp_holding_new_init_precreated()
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

    let result = try_execute_new_definition(&mut state, 2, false);

    assert!(matches!(result, Err(NssaError::ProgramExecutionFailed(_))));
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
        ata_program_id: Ids::ata_program(),
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
}

#[test]
fn amm_swap_b_to_a() {
    let mut state = state_for_amm_tests();

    let instruction = amm_core::Instruction::SwapExactInput {
        swap_amount_in: Balances::swap_amount_in(),
        min_amount_out: Balances::swap_min_out(),
        token_definition_id_in: Ids::token_b_definition(),
        ata_program_id: Ids::ata_program(),
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
        token_definition_id_in: Ids::token_a_definition(),
        ata_program_id: Ids::ata_program(),
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
