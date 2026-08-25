#![cfg(test)]
#![expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test fixtures use fixed values to lock AMM math boundaries"
)]

use std::num::NonZero;

use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_liquidity_token_pda_seed,
    compute_lp_lock_holding_pda, compute_lp_lock_holding_pda_seed, compute_pool_pda,
    compute_pool_pda_seed, compute_vault_pda, compute_vault_pda_seed, isqrt_product, mul_div_floor,
    AmmConfig, PoolDefinition, FEE_BPS_DENOMINATOR, FEE_TIER_BPS_1, FEE_TIER_BPS_100,
    FEE_TIER_BPS_30, FEE_TIER_BPS_5, MINIMUM_LIQUIDITY,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::{ChainedCall, Claim, ProgramId},
};
use token_core::{TokenDefinition, TokenHolding};

use crate::{
    add::add_liquidity,
    new_definition::new_definition,
    remove::remove_liquidity,
    swap::{swap_exact_input, swap_exact_output},
    sync::sync_reserves,
};

const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
const MALICIOUS_TOKEN_PROGRAM_ID: ProgramId = [99; 8];

struct BalanceForTests;
struct ChainedCallForTests;
struct IdForTests;
struct AccountWithMetadataForTests;
type AccountForTests = AccountWithMetadataForTests;

impl BalanceForTests {
    fn fee_tier() -> u128 {
        FEE_TIER_BPS_30
    }

    fn vault_a_reserve_init() -> u128 {
        5_000
    }

    fn vault_b_reserve_init() -> u128 {
        2_500
    }

    fn vault_a_reserve_low() -> u128 {
        10
    }

    fn vault_b_reserve_low() -> u128 {
        10
    }

    fn vault_a_reserve_high() -> u128 {
        500_000
    }

    fn vault_b_reserve_high() -> u128 {
        500_000
    }

    fn user_token_a_balance() -> u128 {
        10_000
    }

    fn user_token_b_balance() -> u128 {
        10_000
    }

    fn user_token_lp_balance() -> u128 {
        100
    }

    fn remove_min_amount_a() -> u128 {
        50
    }

    fn remove_min_amount_b() -> u128 {
        100
    }

    fn remove_actual_a_successful() -> u128 {
        141
    }

    fn remove_min_amount_b_low() -> u128 {
        50
    }

    fn remove_amount_lp() -> u128 {
        100
    }

    fn remove_amount_lp_1() -> u128 {
        30
    }

    fn add_max_amount_a() -> u128 {
        500
    }

    fn add_max_amount_b() -> u128 {
        200
    }

    fn effective_swap_in_a() -> u128 {
        BalanceForTests::add_max_amount_a() * (FEE_BPS_DENOMINATOR - BalanceForTests::fee_tier())
            / FEE_BPS_DENOMINATOR
    }

    fn effective_swap_in_b() -> u128 {
        BalanceForTests::add_max_amount_b() * (FEE_BPS_DENOMINATOR - BalanceForTests::fee_tier())
            / FEE_BPS_DENOMINATOR
    }

    fn add_max_amount_a_low() -> u128 {
        10
    }

    fn add_max_amount_b_low() -> u128 {
        10
    }

    fn add_min_amount_lp() -> u128 {
        20
    }

    fn lp_supply_init() -> u128 {
        // sqrt(vault_a_reserve_init * vault_b_reserve_init) = sqrt(5000 * 2500) = 3535
        (BalanceForTests::vault_a_reserve_init() * BalanceForTests::vault_b_reserve_init()).isqrt()
    }

    fn lp_user_init() -> u128 {
        BalanceForTests::lp_supply_init() - MINIMUM_LIQUIDITY
    }

    fn vault_a_swap_test_1() -> u128 {
        BalanceForTests::vault_a_reserve_init() + BalanceForTests::add_max_amount_a()
    }

    fn vault_a_swap_test_2() -> u128 {
        BalanceForTests::vault_a_reserve_init() - BalanceForTests::swap_amount_out_a()
    }

    fn vault_b_swap_test_1() -> u128 {
        BalanceForTests::vault_b_reserve_init() - BalanceForTests::swap_amount_out_b()
    }

    fn vault_b_swap_test_2() -> u128 {
        BalanceForTests::vault_b_reserve_init() + BalanceForTests::add_max_amount_b()
    }

    fn min_amount_out() -> u128 {
        200
    }

    fn min_amount_out_too_high() -> u128 {
        BalanceForTests::swap_amount_out_b() + 1
    }

    fn vault_a_add_successful() -> u128 {
        BalanceForTests::vault_a_reserve_init() + BalanceForTests::add_successful_amount_a()
    }

    fn vault_b_add_successful() -> u128 {
        BalanceForTests::vault_b_reserve_init() + BalanceForTests::add_successful_amount_b()
    }

    fn add_successful_amount_a() -> u128 {
        (BalanceForTests::vault_a_reserve_init() * BalanceForTests::add_max_amount_b())
            / BalanceForTests::vault_b_reserve_init()
    }

    fn add_successful_amount_b() -> u128 {
        BalanceForTests::add_max_amount_b()
    }

    fn max_amount_in() -> u128 {
        166
    }

    fn vault_a_remove_successful() -> u128 {
        BalanceForTests::vault_a_reserve_init() - BalanceForTests::remove_actual_a_successful()
    }

    fn vault_b_remove_successful() -> u128 {
        BalanceForTests::vault_b_reserve_init() - BalanceForTests::remove_actual_b_successful()
    }

    fn swap_amount_out_b() -> u128 {
        (BalanceForTests::vault_b_reserve_init() * BalanceForTests::effective_swap_in_a())
            / (BalanceForTests::vault_a_reserve_init() + BalanceForTests::effective_swap_in_a())
    }

    fn swap_amount_out_a() -> u128 {
        (BalanceForTests::vault_a_reserve_init() * BalanceForTests::effective_swap_in_b())
            / (BalanceForTests::vault_b_reserve_init() + BalanceForTests::effective_swap_in_b())
    }

    fn add_delta_lp_successful() -> u128 {
        std::cmp::min(
            BalanceForTests::lp_supply_init() * BalanceForTests::add_successful_amount_a()
                / BalanceForTests::vault_a_reserve_init(),
            BalanceForTests::lp_supply_init() * BalanceForTests::add_successful_amount_b()
                / BalanceForTests::vault_b_reserve_init(),
        )
    }

    fn remove_actual_b_successful() -> u128 {
        (BalanceForTests::vault_b_reserve_init() * BalanceForTests::remove_amount_lp())
            / BalanceForTests::lp_supply_init()
    }

    fn add_lp_supply_successful() -> u128 {
        BalanceForTests::lp_supply_init() + BalanceForTests::add_delta_lp_successful()
    }

    fn remove_lp_supply_successful() -> u128 {
        BalanceForTests::lp_supply_init() - BalanceForTests::remove_amount_lp()
    }
}

impl ChainedCallForTests {
    fn cc_swap_token_a_test_1() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_a(),
                AccountWithMetadataForTests::vault_a_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::add_max_amount_a(),
            },
        )
    }

    fn cc_swap_token_b_test_1() -> ChainedCall {
        let swap_amount = BalanceForTests::swap_amount_out_b();

        let mut vault_b_auth = AccountWithMetadataForTests::vault_b_init();
        vault_b_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_b_auth, AccountWithMetadataForTests::user_holding_b()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }

    fn cc_swap_token_a_test_2() -> ChainedCall {
        let swap_amount = BalanceForTests::swap_amount_out_a();

        let mut vault_a_auth = AccountWithMetadataForTests::vault_a_init();
        vault_a_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_a_auth, AccountWithMetadataForTests::user_holding_a()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_a_definition_id(),
        )])
    }

    fn cc_swap_token_b_test_2() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_b(),
                AccountWithMetadataForTests::vault_b_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::add_max_amount_b(),
            },
        )
    }

    fn cc_swap_exact_output_token_a_test_1() -> ChainedCall {
        // reserve_in=1000, amount_out=166, fee=30bps
        // required_effective_in = ceil(1000 * 166 / 334) = 498
        // deposit = ceil(498 * 10000 / 9970) = 500
        let swap_amount: u128 = 500;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_a(),
                AccountWithMetadataForTests::vault_a_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
    }

    fn cc_swap_exact_output_token_b_test_1() -> ChainedCall {
        let swap_amount: u128 = 166;

        let mut vault_b_auth = AccountWithMetadataForTests::vault_b_init();
        vault_b_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_b_auth, AccountWithMetadataForTests::user_holding_b()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }

    fn cc_swap_exact_output_token_a_test_2() -> ChainedCall {
        let swap_amount: u128 = 285;

        let mut vault_a_auth = AccountWithMetadataForTests::vault_a_init();
        vault_a_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_a_auth, AccountWithMetadataForTests::user_holding_a()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_a_definition_id(),
        )])
    }

    fn cc_swap_exact_output_token_b_test_2() -> ChainedCall {
        // reserve_in=500, amount_out=285, fee=30bps
        // required_effective_in = ceil(500 * 285 / 715) = 200
        // deposit = ceil(200 * 10000 / 9970) = 201
        let swap_amount: u128 = 201;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_b(),
                AccountWithMetadataForTests::vault_b_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: swap_amount,
            },
        )
    }

    fn cc_swap_rounding_boundary_token_a_in() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_a(),
                AccountWithMetadataForTests::vault_a_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: 3,
            },
        )
    }

    fn cc_swap_rounding_boundary_token_b_out() -> ChainedCall {
        let mut vault_b_auth = AccountWithMetadataForTests::vault_b_init();
        vault_b_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_b_auth, AccountWithMetadataForTests::user_holding_b()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: 1,
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }

    fn cc_add_token_a() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_a(),
                AccountWithMetadataForTests::vault_a_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::add_successful_amount_a(),
            },
        )
    }

    fn cc_add_token_b() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountWithMetadataForTests::user_holding_b(),
                AccountWithMetadataForTests::vault_b_init(),
            ],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::add_successful_amount_b(),
            },
        )
    }

    fn cc_add_pool_lp() -> ChainedCall {
        let mut pool_lp_auth = AccountWithMetadataForTests::pool_lp_init();
        pool_lp_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                pool_lp_auth,
                AccountWithMetadataForTests::user_holding_lp_init(),
            ],
            &token_core::Instruction::Mint {
                amount_to_mint: BalanceForTests::add_delta_lp_successful(),
            },
        )
        .with_pda_seeds(vec![compute_liquidity_token_pda_seed(
            IdForTests::pool_definition_id(),
        )])
    }

    fn cc_remove_token_a() -> ChainedCall {
        let mut vault_a_auth = AccountWithMetadataForTests::vault_a_init();
        vault_a_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_a_auth, AccountWithMetadataForTests::user_holding_a()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::remove_actual_a_successful(),
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_a_definition_id(),
        )])
    }

    fn cc_remove_token_b() -> ChainedCall {
        let mut vault_b_auth = AccountWithMetadataForTests::vault_b_init();
        vault_b_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![vault_b_auth, AccountWithMetadataForTests::user_holding_b()],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::remove_actual_b_successful(),
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }

    fn cc_remove_pool_lp() -> ChainedCall {
        let mut pool_lp_auth = AccountWithMetadataForTests::pool_lp_init();
        pool_lp_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                pool_lp_auth,
                AccountWithMetadataForTests::user_holding_lp_init(),
            ],
            &token_core::Instruction::Burn {
                amount_to_burn: BalanceForTests::remove_amount_lp(),
            },
        )
        .with_pda_seeds(vec![compute_liquidity_token_pda_seed(
            IdForTests::pool_definition_id(),
        )])
    }

    fn cc_new_definition_token_a() -> ChainedCall {
        let mut vault_a_auth = AccountWithMetadataForTests::vault_a_init();
        vault_a_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![AccountWithMetadataForTests::user_holding_a(), vault_a_auth],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::vault_a_reserve_init(),
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_a_definition_id(),
        )])
    }

    fn cc_new_definition_token_b() -> ChainedCall {
        let mut vault_b_auth = AccountWithMetadataForTests::vault_b_init();
        vault_b_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![AccountWithMetadataForTests::user_holding_b(), vault_b_auth],
            &token_core::Instruction::Transfer {
                amount_to_transfer: BalanceForTests::vault_b_reserve_init(),
            },
        )
        .with_pda_seeds(vec![compute_vault_pda_seed(
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }

    fn cc_new_definition_token_lp_lock() -> ChainedCall {
        let mut pool_lp_auth = AccountForTests::pool_lp_uninit();
        pool_lp_auth.is_authorized = true;
        let mut lp_lock_holding_auth = AccountForTests::lp_lock_holding_uninit();
        lp_lock_holding_auth.is_authorized = true;

        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![pool_lp_auth.clone(), lp_lock_holding_auth],
            &token_core::Instruction::NewFungibleDefinition {
                name: String::from("LP Token"),
                total_supply: MINIMUM_LIQUIDITY,
                mint_authority: Some(pool_lp_auth.account_id),
            },
        )
        .with_pda_seeds(vec![
            compute_liquidity_token_pda_seed(IdForTests::pool_definition_id()),
            compute_lp_lock_holding_pda_seed(IdForTests::pool_definition_id()),
        ])
    }

    fn cc_new_definition_token_lp_user() -> ChainedCall {
        ChainedCall::new(
            TOKEN_PROGRAM_ID,
            vec![
                AccountForTests::pool_lp_created_after_lock(),
                AccountForTests::user_holding_lp_uninit(),
            ],
            &token_core::Instruction::Mint {
                amount_to_mint: BalanceForTests::lp_user_init(),
            },
        )
        .with_pda_seeds(vec![compute_liquidity_token_pda_seed(
            IdForTests::pool_definition_id(),
        )])
    }

    fn cc_new_definition_create_current_tick() -> ChainedCall {
        // The pool is passed to the oracle in its post-claim state: owned by the AMM program and
        // carrying the freshly written PoolDefinition, authorized as the price source.
        let mut pool_price_source = AccountForTests::pool_definition_init();
        pool_price_source.account.program_owner = AMM_PROGRAM_ID;
        pool_price_source.is_authorized = true;

        let initial_price = amm_core::spot_price_q64_64(
            BalanceForTests::vault_a_reserve_init(),
            BalanceForTests::vault_b_reserve_init(),
        );

        ChainedCall::new(
            TWAP_ORACLE_PROGRAM_ID,
            vec![
                AccountForTests::current_tick_account_uninit(),
                pool_price_source,
                AccountForTests::clock(),
            ],
            &twap_oracle_core::Instruction::CreateCurrentTickAccount { initial_price },
        )
        .with_pda_seeds(vec![compute_pool_pda_seed(
            IdForTests::token_a_definition_id(),
            IdForTests::token_b_definition_id(),
        )])
    }
}

impl IdForTests {
    fn token_a_definition_id() -> AccountId {
        AccountId::new([42; 32])
    }

    fn token_b_definition_id() -> AccountId {
        AccountId::new([43; 32])
    }

    fn token_lp_definition_id() -> AccountId {
        compute_liquidity_token_pda(AMM_PROGRAM_ID, IdForTests::pool_definition_id())
    }

    fn lp_lock_holding_id() -> AccountId {
        compute_lp_lock_holding_pda(AMM_PROGRAM_ID, IdForTests::pool_definition_id())
    }

    fn user_token_a_id() -> AccountId {
        AccountId::new([45; 32])
    }

    fn user_token_b_id() -> AccountId {
        AccountId::new([46; 32])
    }

    fn user_token_lp_id() -> AccountId {
        AccountId::new([47; 32])
    }

    fn pool_definition_id() -> AccountId {
        compute_pool_pda(
            AMM_PROGRAM_ID,
            IdForTests::token_a_definition_id(),
            IdForTests::token_b_definition_id(),
        )
    }

    fn vault_a_id() -> AccountId {
        compute_vault_pda(
            AMM_PROGRAM_ID,
            IdForTests::pool_definition_id(),
            IdForTests::token_a_definition_id(),
        )
    }

    fn vault_b_id() -> AccountId {
        compute_vault_pda(
            AMM_PROGRAM_ID,
            IdForTests::pool_definition_id(),
            IdForTests::token_b_definition_id(),
        )
    }
}

impl AccountWithMetadataForTests {
    fn config_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: AMM_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&AmmConfig {
                    token_program_id: TOKEN_PROGRAM_ID,
                    twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
                    authority: AccountId::new([9; 32]),
                }),
                nonce: Nonce(0),
            },
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
        }
    }

    /// Config PDA that has never been initialized (default, empty data).
    fn config_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: compute_config_pda(AMM_PROGRAM_ID),
        }
    }

    /// An initialized config carrying valid data but stored at the wrong account ID.
    fn config_with_wrong_id() -> AccountWithMetadata {
        let mut config = AccountWithMetadataForTests::config_init();
        config.account_id = AccountId::new([7; 32]);
        config
    }

    /// The pool's TWAP current-tick PDA, uninitialized (created by `new_definition`).
    fn current_tick_account_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: twap_oracle_core::compute_current_tick_account_pda(
                TWAP_ORACLE_PROGRAM_ID,
                IdForTests::pool_definition_id(),
            ),
        }
    }

    /// The canonical 1-block LEZ clock account.
    fn clock() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID,
        }
    }

    fn user_holding_a() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::user_token_a_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_a_id(),
        }
    }

    fn user_holding_b() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::user_token_b_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_b_id(),
        }
    }

    fn vault_a_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::vault_a_reserve_init(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn vault_b_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::vault_b_reserve_init(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_b_id(),
        }
    }

    fn vault_a_init_high() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::vault_a_reserve_high(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn vault_b_init_high() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::vault_b_reserve_high(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_b_id(),
        }
    }

    fn vault_a_init_low() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::vault_a_reserve_low(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn vault_b_init_low() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::vault_b_reserve_low(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_b_id(),
        }
    }

    fn vault_a_init_zero() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn vault_b_init_zero() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_b_id(),
        }
    }

    fn pool_lp_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::lp_supply_init(),
                    metadata_id: None,
                    authority: Some(IdForTests::token_lp_definition_id()),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::token_lp_definition_id(),
        }
    }

    fn pool_lp_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: IdForTests::token_lp_definition_id(),
        }
    }

    fn pool_lp_created_after_lock() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("LP Token"),
                    total_supply: MINIMUM_LIQUIDITY,
                    metadata_id: None,
                    authority: Some(IdForTests::token_lp_definition_id()),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::token_lp_definition_id(),
        }
    }

    fn pool_lp_with_wrong_id() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenDefinition::Fungible {
                    name: String::from("test"),
                    total_supply: BalanceForTests::lp_supply_init(),
                    metadata_id: None,
                    authority: Some(IdForTests::token_lp_definition_id()),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn user_holding_lp_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_lp_definition_id(),
                    balance: 0,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_lp_id(),
        }
    }

    fn user_holding_lp_init() -> AccountWithMetadata {
        AccountForTests::user_holding_lp_with_balance(BalanceForTests::user_token_lp_balance())
    }

    fn user_holding_lp_with_balance(balance: u128) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_lp_definition_id(),
                    balance,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_lp_id(),
        }
    }

    fn lp_lock_holding_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: IdForTests::lp_lock_holding_id(),
        }
    }

    fn lp_lock_holding_with_wrong_id() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: IdForTests::vault_a_id(),
        }
    }

    fn pool_definition_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_uninit() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    /// A smaller pool (reserve_a=1_000, reserve_b=500) used exclusively by
    /// swap-exact-output tests, whose hardcoded expected values were computed
    /// against these reserves.  vault_a_init/vault_b_init still satisfy the
    /// balance ≥ reserve check (5_000 ≥ 1_000, 2_500 ≥ 500).
    fn pool_definition_swap_exact_output_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: 1_000,
                    reserve_b: 500,
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_rounding_boundary_init() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: MINIMUM_LIQUIDITY,
                    reserve_a: 1_000,
                    reserve_b: 1_000,
                    fees: FEE_TIER_BPS_30,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_init_reserve_a_zero() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: 0,
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_init_reserve_b_zero() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: 0,
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_init_reserve_a_low() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::vault_a_reserve_low(),
                    reserve_a: BalanceForTests::vault_a_reserve_low(),
                    reserve_b: BalanceForTests::vault_b_reserve_high(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_init_reserve_b_low() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::vault_a_reserve_high(),
                    reserve_a: BalanceForTests::vault_a_reserve_high(),
                    reserve_b: BalanceForTests::vault_b_reserve_low(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_test_1() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: BalanceForTests::vault_a_swap_test_1(),
                    reserve_b: BalanceForTests::vault_b_swap_test_1(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_test_2() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: BalanceForTests::vault_a_swap_test_2(),
                    reserve_b: BalanceForTests::vault_b_swap_test_2(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_exact_output_test_1() -> AccountWithMetadata {
        // swap token_a in for 166 token_b out, fee=30bps
        // reserve_a: 1000 + 500 = 1500 (gross deposit, see
        // cc_swap_exact_output_token_a_test_1) reserve_b: 500  - 166 = 334
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0_u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: 1500_u128,
                    reserve_b: 334_u128,
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_exact_output_test_2() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0_u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: 715_u128,
                    reserve_b: 701_u128,
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_swap_rounding_boundary_post() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0_u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: MINIMUM_LIQUIDITY,
                    reserve_a: 1003_u128,
                    reserve_b: 999_u128,
                    fees: FEE_TIER_BPS_30,
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_add_zero_lp() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::vault_a_reserve_low(),
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_add_successful() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::add_lp_supply_successful(),
                    reserve_a: BalanceForTests::vault_a_add_successful(),
                    reserve_b: BalanceForTests::vault_b_add_successful(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_init_low_balances() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: MINIMUM_LIQUIDITY,
                    reserve_a: BalanceForTests::vault_a_reserve_low(),
                    reserve_b: BalanceForTests::vault_b_reserve_low(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_remove_successful() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::remove_lp_supply_successful(),
                    reserve_a: BalanceForTests::vault_a_remove_successful(),
                    reserve_b: BalanceForTests::vault_b_remove_successful(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_below_minimum_liquidity() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: MINIMUM_LIQUIDITY - 1,
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }

    fn pool_definition_with_wrong_id() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: BalanceForTests::lp_supply_init(),
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: AccountId::new([4; 32]),
        }
    }

    fn vault_a_with_wrong_id() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::vault_a_reserve_init(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: AccountId::new([4; 32]),
        }
    }

    fn vault_b_with_wrong_id() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::vault_b_reserve_init(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: AccountId::new([4; 32]),
        }
    }

    fn user_holding_a_wrong_program() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: MALICIOUS_TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_a_definition_id(),
                    balance: BalanceForTests::user_token_a_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_a_id(),
        }
    }

    fn user_holding_b_wrong_program() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: MALICIOUS_TOKEN_PROGRAM_ID,
                balance: 0u128,
                data: Data::from(&TokenHolding::Fungible {
                    definition_id: IdForTests::token_b_definition_id(),
                    balance: BalanceForTests::user_token_b_balance(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::user_token_b_id(),
        }
    }

    /// Legacy/corrupted pool state whose reported supply has already been drained down to the
    /// permanent lock (liquidity_pool_supply == MINIMUM_LIQUIDITY).
    fn pool_definition_at_minimum_liquidity() -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                program_owner: ProgramId::default(),
                balance: 0u128,
                data: Data::from(&PoolDefinition {
                    definition_token_a_id: IdForTests::token_a_definition_id(),
                    definition_token_b_id: IdForTests::token_b_definition_id(),
                    vault_a_id: IdForTests::vault_a_id(),
                    vault_b_id: IdForTests::vault_b_id(),
                    liquidity_pool_id: IdForTests::token_lp_definition_id(),
                    liquidity_pool_supply: MINIMUM_LIQUIDITY,
                    reserve_a: BalanceForTests::vault_a_reserve_init(),
                    reserve_b: BalanceForTests::vault_b_reserve_init(),
                    fees: BalanceForTests::fee_tier(),
                }),
                nonce: Nonce(0),
            },
            is_authorized: true,
            account_id: IdForTests::pool_definition_id(),
        }
    }
}

#[test]
fn test_pool_pda_produces_unique_id_for_token_pair() {
    assert!(
        amm_core::compute_pool_pda(
            AMM_PROGRAM_ID,
            IdForTests::token_a_definition_id(),
            IdForTests::token_b_definition_id()
        ) == compute_pool_pda(
            AMM_PROGRAM_ID,
            IdForTests::token_b_definition_id(),
            IdForTests::token_a_definition_id()
        )
    );
}

#[should_panic(expected = "Vault A was not provided")]
#[test]
fn test_call_add_liquidity_vault_a_omitted() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_with_wrong_id(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault B was not provided")]
#[test]
fn test_call_add_liquidity_vault_b_omitted() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_with_wrong_id(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "LP definition mismatch")]
#[test]
fn test_call_add_liquidity_lp_definition_mismatch() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_with_wrong_id(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Both max-balances must be nonzero")]
#[test]
fn test_call_add_liquidity_zero_balance_1() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        0,
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Both max-balances must be nonzero")]
#[test]
fn test_call_add_liquidity_zero_balance_2() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        0,
        BalanceForTests::add_max_amount_a(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vaults' balances must be at least the reserve amounts")]
#[test]
fn test_call_add_liquidity_vault_a_balance_below_reserve() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vaults' balances must be at least the reserve amounts")]
#[test]
fn test_call_add_liquidity_vault_b_balance_below_reserve() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vaults' balances must be at least the reserve amounts")]
#[test]
fn test_call_add_liquidity_vault_insufficient_balance_1() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init_zero(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vaults' balances must be at least the reserve amounts")]
#[test]
fn test_call_add_liquidity_vault_insufficient_balance_2() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init_zero(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "A trade amount is 0")]
#[test]
fn test_call_add_liquidity_actual_amount_zero_1() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init_reserve_a_low(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init_high(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "A trade amount is 0")]
#[test]
fn test_call_add_liquidity_actual_amount_zero_2() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init_reserve_b_low(),
        AccountWithMetadataForTests::vault_a_init_high(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a_low(),
        BalanceForTests::add_max_amount_b_low(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserves must be nonzero")]
#[test]
fn test_call_add_liquidity_reserves_zero_1() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init_reserve_a_zero(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserves must be nonzero")]
#[test]
fn test_call_add_liquidity_reserves_zero_2() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init_reserve_b_zero(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Payable LP must be nonzero")]
#[test]
fn test_call_add_liquidity_payable_lp_zero() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_add_zero_lp(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a_low(),
        BalanceForTests::add_max_amount_b_low(),
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_call_add_liquidity_chained_call_successsful() {
    let (post_states, chained_calls) = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_add_successful().account
            == *pool_post.account()
    );

    let chained_call_lp = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();
    let chained_call_a = chained_calls[2].clone();

    assert!(chained_call_a == ChainedCallForTests::cc_add_token_a());
    assert!(chained_call_b == ChainedCallForTests::cc_add_token_b());
    assert!(chained_call_lp == ChainedCallForTests::cc_add_pool_lp());

    // The fourth chained call refreshes the pool's TWAP current tick from the post-add price.
    assert_eq!(chained_calls.len(), 4);
    assert_update_tick_call(&chained_calls, post_states[1].account());

    // The config account is echoed back unchanged as the first post-state.
    assert_eq!(
        *post_states[0].account(),
        AccountWithMetadataForTests::config_init().account
    );
}

#[should_panic(expected = "AMM Program must be initialized before use")]
#[test]
fn test_call_add_liquidity_uninitialized_config_panics() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_uninit(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "AMM config Account ID does not match PDA")]
#[test]
fn test_call_add_liquidity_wrong_config_pda_panics() {
    let _post_states = add_liquidity(
        AccountWithMetadataForTests::config_with_wrong_id(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).unwrap(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault A was not provided")]
#[test]
fn test_call_remove_liquidity_vault_a_omitted() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_with_wrong_id(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault B was not provided")]
#[test]
fn test_call_remove_liquidity_vault_b_omitted() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_with_wrong_id(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "LP definition mismatch")]
#[test]
fn test_call_remove_liquidity_lp_def_mismatch() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_with_wrong_id(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Invalid liquidity account provided")]
#[test]
fn test_call_remove_liquidity_insufficient_liquidity_amount() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(), /* different token account than lp to
                                               * create desired
                                               * error */
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(
    expected = "Insufficient minimal withdraw amount (Token A) provided for liquidity amount"
)]
#[test]
fn test_call_remove_liquidity_insufficient_balance_1() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp_1()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool only contains locked liquidity")]
#[test]
fn test_call_remove_liquidity_pool_at_minimum_liquidity() {
    // Removing from a legacy/corrupted pool that is already at the locked floor must be rejected.
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_at_minimum_liquidity(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_with_balance(MINIMUM_LIQUIDITY),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(MINIMUM_LIQUIDITY).unwrap(),
        1,
        1,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Cannot remove locked minimum liquidity")]
#[test]
fn test_call_remove_liquidity_exceeds_unlocked_supply() {
    // Model corrupted ownership by giving the caller the full LP supply even though the lock
    // account should permanently hold MINIMUM_LIQUIDITY. The guard must still refuse to burn
    // through the permanent floor.
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_with_balance(BalanceForTests::lp_supply_init()),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::lp_supply_init()).unwrap(),
        1,
        1,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(
    expected = "Insufficient minimal withdraw amount (Token B) provided for liquidity amount"
)]
#[test]
fn test_call_remove_liquidity_insufficient_balance_2() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Minimum withdraw amount must be nonzero")]
#[test]
fn test_call_remove_liquidity_min_bal_zero_1() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        0,
        BalanceForTests::remove_min_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Minimum withdraw amount must be nonzero")]
#[test]
fn test_call_remove_liquidity_min_bal_zero_2() {
    let _post_states = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        0,
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_call_remove_liquidity_chained_call_successful() {
    let (post_states, chained_calls) = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).unwrap(),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b_low(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_remove_successful().account
            == *pool_post.account()
    );

    let chained_call_lp = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();
    let chained_call_a = chained_calls[2].clone();

    assert!(chained_call_a == ChainedCallForTests::cc_remove_token_a());
    assert!(chained_call_b == ChainedCallForTests::cc_remove_token_b());
    assert!(chained_call_lp == ChainedCallForTests::cc_remove_pool_lp());

    // The fourth chained call refreshes the pool's TWAP current tick from the post-removal price.
    assert_eq!(chained_calls.len(), 4);
    assert_update_tick_call(&chained_calls, post_states[1].account());
}

#[should_panic(expected = "Balances must be nonzero")]
#[test]
fn test_call_new_definition_with_zero_balance_1() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(0).expect("Balances must be nonzero"),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Balances must be nonzero")]
#[test]
fn test_call_new_definition_with_zero_balance_2() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(0).expect("Balances must be nonzero"),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Cannot set up a swap for a token with itself")]
#[test]
fn test_call_new_definition_same_token_definition() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Liquidity pool Token Definition Account ID does not match PDA")]
#[test]
fn test_call_new_definition_wrong_liquidity_id() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_with_wrong_id(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "LP lock holding Account ID does not match PDA")]
#[test]
fn test_call_new_definition_wrong_lp_lock_holding_id() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_with_wrong_id(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool Definition Account ID does not match PDA")]
#[test]
fn test_call_new_definition_wrong_pool_id() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_with_wrong_id(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault ID does not match PDA")]
#[test]
fn test_call_new_definition_wrong_vault_id_1() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_with_wrong_id(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault ID does not match PDA")]
#[test]
fn test_call_new_definition_wrong_vault_id_2() {
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_with_wrong_id(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool account must be uninitialized")]
#[test]
fn test_call_new_definition_rejects_initialized_pool() {
    // Verify that new_definition fails if passed an already-initialized pool
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Initial liquidity must exceed minimum liquidity lock")]
#[test]
fn test_call_new_definition_initial_lp_too_small() {
    // isqrt(1000 * 1000) = 1000 == MINIMUM_LIQUIDITY, so the assertion fires.
    let _post_states = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(MINIMUM_LIQUIDITY).unwrap(),
        NonZero::new(MINIMUM_LIQUIDITY).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_call_new_definition_chained_call_successful() {
    let (post_states, chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(AccountWithMetadataForTests::pool_definition_init().account == *pool_post.account());
    assert_eq!(
        pool_post.required_claim(),
        Some(Claim::Pda(compute_pool_pda_seed(
            IdForTests::token_a_definition_id(),
            IdForTests::token_b_definition_id(),
        )))
    );

    let chained_call_lp_lock = chained_calls[0].clone();
    let chained_call_lp_user = chained_calls[1].clone();
    let chained_call_b = chained_calls[2].clone();
    let chained_call_a = chained_calls[3].clone();

    assert!(chained_call_a == ChainedCallForTests::cc_new_definition_token_a());
    assert!(chained_call_b == ChainedCallForTests::cc_new_definition_token_b());
    assert!(chained_call_lp_lock == ChainedCallForTests::cc_new_definition_token_lp_lock());
    assert!(chained_call_lp_user == ChainedCallForTests::cc_new_definition_token_lp_user());

    // The fifth chained call creates the pool's TWAP current-tick account, seeding the tick from
    // the opening reserves.
    assert_eq!(chained_calls.len(), 5);
    assert!(chained_calls[4] == ChainedCallForTests::cc_new_definition_create_current_tick());

    // Two extra post-states (current-tick + clock) are echoed back unchanged.
    assert_eq!(post_states.len(), 11);
}

#[should_panic(expected = "Swap exact input: input holding token is not part of the pool")]
#[test]
fn test_call_swap_incorrect_token_type() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault A was not provided")]
#[test]
fn test_call_swap_vault_a_omitted() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_with_wrong_id(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault B was not provided")]
#[test]
fn test_call_swap_vault_b_omitted() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_with_wrong_id(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserve for Token A exceeds vault balance")]
#[test]
fn test_call_swap_reserves_vault_mismatch_1() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserve for Token B exceeds vault balance")]
#[test]
fn test_call_swap_reserves_vault_mismatch_2() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool liquidity supply is below minimum liquidity")]
#[test]
fn test_call_swap_below_minimum_liquidity() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_below_minimum_liquidity(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Fee tier must be one of 1, 5, 30, or 100 basis points")]
#[test]
fn test_call_swap_rejects_unsupported_fee_tier() {
    let mut pool = AccountWithMetadataForTests::pool_definition_init();
    let mut pool_def = PoolDefinition::try_from(&pool.account.data).unwrap();
    pool_def.fees = 2;
    pool.account.data = Data::from(&pool_def);

    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        pool,
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_a_low(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Withdraw amount is less than minimal amount out")]
#[test]
fn test_call_swap_below_min_out() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out_too_high(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Effective swap amount should be nonzero")]
#[test]
fn test_call_swap_effective_amount_zero() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        1,
        0,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Withdraw amount should be nonzero")]
#[test]
fn test_call_swap_output_rounds_to_zero() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init_low_balances(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        2,
        0,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Withdraw amount is less than minimal amount out")]
#[test]
fn test_call_swap_exact_input_rejects_amount_that_rounds_down_below_target_output() {
    let _post_states = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        2,
        1,
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_call_swap_exact_input_accepts_smallest_amount_for_rounded_boundary() {
    let (post_states, chained_calls) = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        3,
        1,
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert_eq!(
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_post().account,
        *pool_post.account()
    );

    let chained_call_a = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_rounding_boundary_token_a_in()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_rounding_boundary_token_b_out()
    );
}

/// Asserts the last chained call is the oracle UpdateCurrentTick, carrying the post-operation
/// spot price and the post-operation pool authorized as the price source.
fn assert_update_tick_call(chained_calls: &[ChainedCall], pool_post_account: &Account) {
    let pool_def = PoolDefinition::try_from(&pool_post_account.data)
        .expect("pool post-state must hold a valid PoolDefinition");
    let expected_price_source = AccountWithMetadata {
        account: pool_post_account.clone(),
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };
    let expected = ChainedCall::new(
        TWAP_ORACLE_PROGRAM_ID,
        vec![
            AccountWithMetadataForTests::current_tick_account_uninit(),
            expected_price_source,
            AccountWithMetadataForTests::clock(),
        ],
        &twap_oracle_core::Instruction::UpdateCurrentTick {
            price: amm_core::spot_price_q64_64(pool_def.reserve_a, pool_def.reserve_b),
        },
    )
    .with_pda_seeds(vec![compute_pool_pda_seed(
        IdForTests::token_a_definition_id(),
        IdForTests::token_b_definition_id(),
    )]);
    assert_eq!(
        *chained_calls
            .last()
            .expect("expected an UpdateCurrentTick chained call"),
        expected
    );
}

#[test]
fn test_call_swap_chained_call_successful_1() {
    let (post_states, chained_calls) = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_a_low(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_swap_test_1().account == *pool_post.account()
    );

    let chained_call_a = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_token_a_test_1()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_token_b_test_1()
    );

    assert_eq!(chained_calls.len(), 3);
    assert_update_tick_call(&chained_calls, pool_post.account());
}

#[test]
fn test_call_swap_chained_call_successful_2() {
    let (post_states, chained_calls) = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_b(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_swap_test_2().account == *pool_post.account()
    );

    let chained_call_a = chained_calls[1].clone();
    let chained_call_b = chained_calls[0].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_token_a_test_2()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_token_b_test_2()
    );

    assert_eq!(chained_calls.len(), 3);
    assert_update_tick_call(&chained_calls, pool_post.account());
}

#[should_panic(expected = "Swap exact output: input holding token is not part of the pool")]
#[test]
fn call_swap_exact_output_incorrect_token_type() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault A was not provided")]
#[test]
fn call_swap_exact_output_vault_a_omitted() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_with_wrong_id(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Vault B was not provided")]
#[test]
fn call_swap_exact_output_vault_b_omitted() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_with_wrong_id(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserve for Token A exceeds vault balance")]
#[test]
fn call_swap_exact_output_reserves_vault_mismatch_1() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Reserve for Token B exceeds vault balance")]
#[test]
fn call_swap_exact_output_reserves_vault_mismatch_2() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool liquidity supply is below minimum liquidity")]
#[test]
fn call_swap_exact_output_below_minimum_liquidity() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_below_minimum_liquidity(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Required input exceeds maximum amount in")]
#[test]
fn call_swap_exact_output_exceeds_max_in() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        166_u128,
        100_u128,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Exact amount out must be nonzero")]
#[test]
fn call_swap_exact_output_zero() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        0_u128,
        500_u128,
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Exact amount out exceeds reserve")]
#[test]
fn call_swap_exact_output_exceeds_reserve() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::vault_b_reserve_init(),
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[test]
fn call_swap_exact_output_chained_call_successful() {
    let (post_states, chained_calls) = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_exact_output_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::max_amount_in(),
        BalanceForTests::vault_b_reserve_init(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_swap_exact_output_test_1().account
            == *pool_post.account()
    );

    let chained_call_a = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_exact_output_token_a_test_1()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_exact_output_token_b_test_1()
    );
}

#[test]
fn call_swap_exact_output_chained_call_successful_2() {
    let (post_states, chained_calls) = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_exact_output_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        285,
        300,
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert!(
        AccountWithMetadataForTests::pool_definition_swap_exact_output_test_2().account
            == *pool_post.account()
    );

    let chained_call_a = chained_calls[1].clone();
    let chained_call_b = chained_calls[0].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_exact_output_token_a_test_2()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_exact_output_token_b_test_2()
    );
}

// The minimum effective input for exact_amount_out=166 on the 1000/500 pool is 498.
// After fee rounding, the true minimum gross input is 500, so 499 must be rejected.
#[should_panic(expected = "Required input exceeds maximum amount in")]
#[test]
fn call_swap_exact_output_fee_enforced() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_exact_output_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        166_u128, // exact_amount_out: token_b
        499_u128, // max_amount_in: still one short after fee rounding
        AMM_PROGRAM_ID,
    );
}

// On a 1000/1000 pool at 0.3%, exact_amount_out = 1 requires gross input 3.
// max_amount_in = 2 must be rejected because the exact-input path would round
// 2 down to effective_in = 1 and still produce 0 output.
#[should_panic(expected = "Required input exceeds maximum amount in")]
#[test]
fn call_swap_exact_output_rejects_max_in_that_rounds_down_below_target_output() {
    let _post_states = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        1,
        2,
        AMM_PROGRAM_ID,
    );
}

#[test]
fn call_swap_exact_output_accepts_smallest_max_in_for_rounded_boundary() {
    let (post_states, chained_calls) = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        1,
        3,
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();

    assert_eq!(
        AccountWithMetadataForTests::pool_definition_swap_rounding_boundary_post().account,
        *pool_post.account()
    );

    let chained_call_a = chained_calls[0].clone();
    let chained_call_b = chained_calls[1].clone();

    assert_eq!(
        chained_call_a,
        ChainedCallForTests::cc_swap_rounding_boundary_token_a_in()
    );
    assert_eq!(
        chained_call_b,
        ChainedCallForTests::cc_swap_rounding_boundary_token_b_out()
    );
}

// Without widening, `reserve_a * exact_amount_out` silently wraps to 0 in release mode, making
// `deposit_amount = 0`, so an attacker would receive `exact_amount_out` tokens while paying
// nothing. Under Option A the product is computed in U256, so the true (enormous) required input
// is computed exactly and the slippage check `deposit_amount <= max_amount_in` correctly rejects
// the attacker's tiny `max_amount_in`.
#[should_panic(expected = "Required input exceeds maximum amount in")]
#[test]
fn swap_exact_output_overflow_protection() {
    // reserve_a chosen so that reserve_a * 2 overflows u128:
    //   (u128::MAX / 2 + 1) * 2 = u128::MAX + 1 → wraps to 0
    let large_reserve: u128 = u128::MAX / 2 + 1;
    let reserve_b: u128 = 1_000;

    let pool = AccountWithMetadata {
        account: Account {
            program_owner: ProgramId::default(),
            balance: 0,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: IdForTests::token_a_definition_id(),
                definition_token_b_id: IdForTests::token_b_definition_id(),
                vault_a_id: IdForTests::vault_a_id(),
                vault_b_id: IdForTests::vault_b_id(),
                liquidity_pool_id: IdForTests::token_lp_definition_id(),
                liquidity_pool_supply: MINIMUM_LIQUIDITY,
                reserve_a: large_reserve,
                reserve_b,
                fees: BalanceForTests::fee_tier(),
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };

    let vault_a = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_a_definition_id(),
                balance: large_reserve,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::vault_a_id(),
    };

    let vault_b = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_b_definition_id(),
                balance: reserve_b,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::vault_b_id(),
    };

    let _result = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        pool,
        vault_a,
        vault_b,
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        2, // exact_amount_out: small, valid (< reserve_b)
        1, // max_amount_in: tiny — real deposit would be enormous, but
        // overflow wraps it to 0, making 0 <= 1 pass silently
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_new_definition_lp_asymmetric_amounts() {
    let (post_states, chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    // check the minted LP amount
    let pool_post = post_states[1].clone();
    let pool_def = PoolDefinition::try_from(&pool_post.account().data).unwrap();
    assert_eq!(
        pool_def.liquidity_pool_supply,
        BalanceForTests::lp_supply_init()
    );

    let chained_call_lp_lock = chained_calls[0].clone();
    let chained_call_lp_user = chained_calls[1].clone();
    assert!(chained_call_lp_lock == ChainedCallForTests::cc_new_definition_token_lp_lock());
    assert!(chained_call_lp_user == ChainedCallForTests::cc_new_definition_token_lp_user());
}

#[test]
fn test_new_definition_lp_symmetric_amounts() {
    // token_a=2000, token_b=2000 → LP=sqrt(4_000_000)=2000
    let token_a_amount = 2_000u128;
    let token_b_amount = 2_000u128;
    let expected_lp = (token_a_amount * token_b_amount).isqrt();
    assert_eq!(expected_lp, 2_000);

    let (post_states, chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(token_a_amount).unwrap(),
        NonZero::new(token_b_amount).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();
    let pool_def = PoolDefinition::try_from(&pool_post.account().data).unwrap();
    assert_eq!(pool_def.liquidity_pool_supply, expected_lp);

    let chained_call_lp_lock = chained_calls[0].clone();
    let chained_call_lp_user = chained_calls[1].clone();

    let mut pool_lp_auth = AccountForTests::pool_lp_uninit();
    pool_lp_auth.is_authorized = true;
    let mut lp_lock_holding_auth = AccountForTests::lp_lock_holding_uninit();
    lp_lock_holding_auth.is_authorized = true;
    let expected_lp_lock_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![pool_lp_auth.clone(), lp_lock_holding_auth],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
            mint_authority: Some(pool_lp_auth.account_id),
        },
    )
    .with_pda_seeds(vec![
        compute_liquidity_token_pda_seed(IdForTests::pool_definition_id()),
        compute_lp_lock_holding_pda_seed(IdForTests::pool_definition_id()),
    ]);

    let expected_lp_user_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![
            AccountForTests::pool_lp_created_after_lock(),
            AccountForTests::user_holding_lp_uninit(),
        ],
        &token_core::Instruction::Mint {
            amount_to_mint: expected_lp - MINIMUM_LIQUIDITY,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(
        IdForTests::pool_definition_id(),
    )]);

    assert_eq!(chained_call_lp_lock, expected_lp_lock_call);
    assert_eq!(chained_call_lp_user, expected_lp_user_call);
}

#[test]
fn test_new_definition_large_18_decimal_amounts_no_overflow() {
    // 100e18 and 200e18 (1e20 / 2e20). The naive `token_a * token_b` product is 2e40, which
    // overflows u128 (max ~3.4e38) and previously panicked. `isqrt_product` widens the product
    // to U256, so this must now succeed. Expected LP = floor(sqrt(2e40)).
    let token_a_amount = 100_000_000_000_000_000_000u128; // 1e20
    let token_b_amount = 200_000_000_000_000_000_000u128; // 2e20
    let expected_lp = isqrt_product(token_a_amount, token_b_amount);
    assert_eq!(expected_lp, 141_421_356_237_309_504_880);

    let (post_states, _chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(token_a_amount).unwrap(),
        NonZero::new(token_b_amount).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    let pool_post = post_states[1].clone();
    let pool_def = PoolDefinition::try_from(&pool_post.account().data).unwrap();
    assert_eq!(pool_def.reserve_a, token_a_amount);
    assert_eq!(pool_def.reserve_b, token_b_amount);
    assert_eq!(pool_def.liquidity_pool_supply, expected_lp);
    assert!(pool_def.liquidity_pool_supply > MINIMUM_LIQUIDITY);
}

#[test]
fn test_minimum_liquidity_lock_and_remove_all_user_lp() {
    let pool_uninitialized = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };
    let token_a_amount = BalanceForTests::vault_a_reserve_init();
    let token_b_amount = BalanceForTests::vault_b_reserve_init();
    let initial_lp = (token_a_amount * token_b_amount).isqrt();
    let user_lp = initial_lp - MINIMUM_LIQUIDITY;

    let (post_states, chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        pool_uninitialized,
        AccountForTests::vault_a_init(),
        AccountForTests::vault_b_init(),
        AccountForTests::pool_lp_uninit(),
        AccountForTests::lp_lock_holding_uninit(),
        AccountForTests::user_holding_a(),
        AccountForTests::user_holding_b(),
        AccountForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(token_a_amount).unwrap(),
        NonZero::new(token_b_amount).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    let mut pool_lp_auth = AccountForTests::pool_lp_uninit();
    pool_lp_auth.is_authorized = true;
    let mut lp_lock_holding_auth = AccountForTests::lp_lock_holding_uninit();
    lp_lock_holding_auth.is_authorized = true;

    let expected_lock_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![pool_lp_auth.clone(), lp_lock_holding_auth],
        &token_core::Instruction::NewFungibleDefinition {
            name: String::from("LP Token"),
            total_supply: MINIMUM_LIQUIDITY,
            mint_authority: Some(pool_lp_auth.account_id),
        },
    )
    .with_pda_seeds(vec![
        compute_liquidity_token_pda_seed(IdForTests::pool_definition_id()),
        compute_lp_lock_holding_pda_seed(IdForTests::pool_definition_id()),
    ]);
    let expected_user_call = ChainedCall::new(
        TOKEN_PROGRAM_ID,
        vec![
            AccountForTests::pool_lp_created_after_lock(),
            AccountForTests::user_holding_lp_uninit(),
        ],
        &token_core::Instruction::Mint {
            amount_to_mint: user_lp,
        },
    )
    .with_pda_seeds(vec![compute_liquidity_token_pda_seed(
        IdForTests::pool_definition_id(),
    )]);
    assert_eq!(chained_calls[0], expected_lock_call);
    assert_eq!(chained_calls[1], expected_user_call);

    let pool_post = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    assert_eq!(pool_post.liquidity_pool_supply, initial_lp);

    let pool_for_remove = AccountWithMetadata {
        account: post_states[1].account().clone(),
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };
    let (remove_post_states, _) = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        pool_for_remove,
        AccountForTests::vault_a_init(),
        AccountForTests::vault_b_init(),
        AccountForTests::pool_lp_init(),
        AccountForTests::user_holding_a(),
        AccountForTests::user_holding_b(),
        AccountForTests::user_holding_lp_with_balance(user_lp),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(user_lp).unwrap(),
        1,
        1,
        AMM_PROGRAM_ID,
    );

    let pool_after_remove =
        PoolDefinition::try_from(&remove_post_states[1].account().data).unwrap();
    assert_eq!(pool_after_remove.liquidity_pool_supply, MINIMUM_LIQUIDITY);
    assert!(pool_after_remove.reserve_a > 0);
    assert!(pool_after_remove.reserve_b > 0);
}

#[test]
fn test_sync_reserves_with_donation() {
    let pool = AccountWithMetadataForTests::pool_definition_init();
    let donation_a = 111u128;

    let mut donated_vault_a = AccountWithMetadataForTests::vault_a_init();
    donated_vault_a.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: IdForTests::token_a_definition_id(),
        balance: BalanceForTests::vault_a_reserve_init() + donation_a,
    });

    let pool_pre = PoolDefinition::try_from(&pool.account.data).unwrap();
    assert_eq!(pool_pre.reserve_a, BalanceForTests::vault_a_reserve_init());

    let (post_states, chained_calls) = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        pool,
        donated_vault_a,
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
    let pool_post = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    assert_eq!(
        pool_post.reserve_a,
        BalanceForTests::vault_a_reserve_init() + donation_a
    );
    assert_eq!(pool_post.reserve_b, BalanceForTests::vault_b_reserve_init());

    // Sync refreshes the pool's TWAP current tick via a chained call carrying the synced spot
    // price, with the synced pool authorized as the price source.
    assert_eq!(chained_calls.len(), 1);
    assert_update_tick_call(&chained_calls, post_states[1].account());
}

#[should_panic(expected = "Sync reserves: vault A balance is less than its reserve")]
#[test]
fn test_sync_reserves_panics_when_vault_a_under_collateralized() {
    let _ = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init_low(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Sync reserves: vault B balance is less than its reserve")]
#[test]
fn test_sync_reserves_panics_when_vault_b_under_collateralized() {
    let _ = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init_low(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Pool liquidity supply is below minimum liquidity")]
#[test]
fn test_sync_reserves_rejects_pool_below_minimum_liquidity() {
    let _ = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_below_minimum_liquidity(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Fee tier must be one of 1, 5, 30, or 100 basis points")]
#[test]
fn test_sync_reserves_rejects_unsupported_fee_tier() {
    let mut pool = AccountWithMetadataForTests::pool_definition_init();
    let mut pool_def = PoolDefinition::try_from(&pool.account.data).unwrap();
    pool_def.fees = 2;
    pool.account.data = Data::from(&pool_def);

    let _ = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        pool,
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
}

#[test]
fn test_donation_then_add_liquidity_sync_mitigates_mispricing() {
    let donation_a = 100u128;

    let mut donated_vault_a = AccountWithMetadataForTests::vault_a_init();
    donated_vault_a.account.data = Data::from(&TokenHolding::Fungible {
        definition_id: IdForTests::token_a_definition_id(),
        balance: BalanceForTests::vault_a_reserve_init() + donation_a,
    });
    let donated_vault_b = AccountWithMetadataForTests::vault_b_init();

    let (post_unsynced, _) = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        donated_vault_a.clone(),
        donated_vault_b.clone(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(1).unwrap(),
        100,
        50,
        AMM_PROGRAM_ID,
    );
    let unsynced_pool_post = PoolDefinition::try_from(&post_unsynced[1].account().data).unwrap();
    let unsynced_delta_lp =
        unsynced_pool_post.liquidity_pool_supply - BalanceForTests::lp_supply_init();

    let donated_vault_a_for_synced_add = donated_vault_a.clone();
    let donated_vault_b_for_synced_add = donated_vault_b.clone();

    let (sync_post, _) = sync_reserves(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        donated_vault_a,
        donated_vault_b,
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        AMM_PROGRAM_ID,
    );
    let synced_pool = AccountWithMetadata {
        account: sync_post[1].account().clone(),
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };

    let (post_synced, _) = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        synced_pool,
        donated_vault_a_for_synced_add,
        donated_vault_b_for_synced_add,
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(1).unwrap(),
        100,
        50,
        AMM_PROGRAM_ID,
    );
    let synced_pool_post = PoolDefinition::try_from(&post_synced[1].account().data).unwrap();
    let synced_delta_lp = synced_pool_post.liquidity_pool_supply
        - PoolDefinition::try_from(&sync_post[1].account().data)
            .unwrap()
            .liquidity_pool_supply;

    assert!(synced_delta_lp < unsynced_delta_lp);
}

// Under Option A the `token_a * token_b` product is computed in U256, so a product that exceeds
// u128 no longer panics: it is square-rooted exactly. Here `large_amount * 2 = 2^128`, whose
// integer sqrt is `2^64`. Previously this multiplication overflowed u128 and panicked.
#[test]
fn new_definition_overflow_protection() {
    let large_amount = u128::MAX / 2 + 1; // 2^127

    let (post_states, _chained_calls) = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(large_amount).unwrap(),
        NonZero::new(2).unwrap(),
        BalanceForTests::fee_tier(),
        AMM_PROGRAM_ID,
    );

    let pool_def = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    // floor(sqrt(2^127 * 2)) = floor(sqrt(2^128)) = 2^64.
    assert_eq!(pool_def.liquidity_pool_supply, 1u128 << 64);
    assert_eq!(pool_def.reserve_a, large_amount);
    assert_eq!(pool_def.reserve_b, 2);
}

// Under Option A the `reserve * max_amount` and `supply * actual` products are computed in U256, so
// realistic large reserves no longer overflow u128. Here every product (reserve_a * max_b,
// supply * actual, etc.) is `1e30 * 1e30 = 1e60`, far beyond u128 (max ~3.4e38), yet the add
// succeeds and computes the correct widened results. Previously these multiplications panicked.
#[test]
fn add_liquidity_overflow_protection() {
    let large: u128 = 1_000_000_000_000_000_000_000_000_000_000; // 1e30

    let pool = AccountWithMetadata {
        account: Account {
            program_owner: ProgramId::default(),
            balance: 0,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: IdForTests::token_a_definition_id(),
                definition_token_b_id: IdForTests::token_b_definition_id(),
                vault_a_id: IdForTests::vault_a_id(),
                vault_b_id: IdForTests::vault_b_id(),
                liquidity_pool_id: IdForTests::token_lp_definition_id(),
                liquidity_pool_supply: large,
                reserve_a: large,
                reserve_b: large,
                fees: BalanceForTests::fee_tier(),
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };

    let vault_a = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_a_definition_id(),
                balance: large,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: IdForTests::vault_a_id(),
    };

    let vault_b = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_b_definition_id(),
                balance: large,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: IdForTests::vault_b_id(),
    };

    let (post_states, _chained_calls) = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        pool,
        vault_a,
        vault_b,
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(1).unwrap(),
        large, // max_amount_a
        large, // max_amount_b
        AMM_PROGRAM_ID,
    );

    let pool_def = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    // Balanced add of `1e30` to each `1e30` reserve mints `delta_lp = 1e30`.
    assert_eq!(pool_def.reserve_a, large + large);
    assert_eq!(pool_def.reserve_b, large + large);
    assert_eq!(pool_def.liquidity_pool_supply, large + large);
}

// Under Option A the `reserve * remove_amount` product is computed in U256, so a product that
// exceeds u128 no longer panics: `large_reserve * 2 = 2^128` is divided down to a valid u128
// withdraw. Previously this multiplication overflowed u128 and panicked.
#[test]
fn remove_liquidity_overflow_protection() {
    let large_reserve: u128 = u128::MAX / 2 + 1; // 2^127
    let reserve_b: u128 = 1_000;
    let lp_supply: u128 = 1_002; // must exceed MINIMUM_LIQUIDITY so remove_amount=2 passes the lock check

    let pool = AccountWithMetadata {
        account: Account {
            program_owner: ProgramId::default(),
            balance: 0,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: IdForTests::token_a_definition_id(),
                definition_token_b_id: IdForTests::token_b_definition_id(),
                vault_a_id: IdForTests::vault_a_id(),
                vault_b_id: IdForTests::vault_b_id(),
                liquidity_pool_id: IdForTests::token_lp_definition_id(),
                liquidity_pool_supply: lp_supply,
                reserve_a: large_reserve,
                reserve_b,
                fees: BalanceForTests::fee_tier(),
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };

    let vault_a = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_a_definition_id(),
                balance: large_reserve,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: IdForTests::vault_a_id(),
    };

    let vault_b = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_b_definition_id(),
                balance: reserve_b,
            }),
            nonce: Nonce(0),
        },
        is_authorized: false,
        account_id: IdForTests::vault_b_id(),
    };

    let user_lp = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_lp_definition_id(),
                balance: 2,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::user_token_lp_id(),
    };

    let (post_states, _chained_calls) = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        pool,
        vault_a,
        vault_b,
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        user_lp,
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(2).unwrap(), // remove_amount=2 → reserve_a * 2 = 2^128 (widened to U256)
        1,
        1,
        AMM_PROGRAM_ID,
    );

    // withdraw_a = floor(reserve_a * 2 / supply); withdraw_b = floor(1000 * 2 / 1002) = 1.
    let expected_withdraw_a = mul_div_floor(large_reserve, 2, lp_supply);
    let expected_withdraw_b = mul_div_floor(reserve_b, 2, lp_supply);
    let pool_def = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    assert_eq!(pool_def.reserve_a, large_reserve - expected_withdraw_a);
    assert_eq!(pool_def.reserve_b, reserve_b - expected_withdraw_b);
    assert_eq!(pool_def.liquidity_pool_supply, lp_supply - 2);
}

// Under Option A the `reserve_out * effective_amount_in` product is computed in U256, so a product
// that exceeds u128 no longer panics: `reserve_b * 2 = 2^128` is divided down to a valid u128
// withdraw. Previously this multiplication overflowed u128 and panicked.
#[test]
fn swap_exact_input_overflow_protection() {
    let large_reserve: u128 = u128::MAX / 2 + 1; // 2^127
    let reserve_b: u128 = 1_000;

    let pool = AccountWithMetadata {
        account: Account {
            program_owner: ProgramId::default(),
            balance: 0,
            data: Data::from(&PoolDefinition {
                definition_token_a_id: IdForTests::token_a_definition_id(),
                definition_token_b_id: IdForTests::token_b_definition_id(),
                vault_a_id: IdForTests::vault_a_id(),
                vault_b_id: IdForTests::vault_b_id(),
                liquidity_pool_id: IdForTests::token_lp_definition_id(),
                liquidity_pool_supply: MINIMUM_LIQUIDITY,
                reserve_a: 1_000,
                reserve_b: large_reserve,
                fees: BalanceForTests::fee_tier(),
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::pool_definition_id(),
    };

    let vault_a = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_a_definition_id(),
                balance: reserve_b,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::vault_a_id(),
    };

    let vault_b = AccountWithMetadata {
        account: Account {
            program_owner: TOKEN_PROGRAM_ID,
            balance: 0,
            data: Data::from(&TokenHolding::Fungible {
                definition_id: IdForTests::token_b_definition_id(),
                balance: large_reserve,
            }),
            nonce: Nonce(0),
        },
        is_authorized: true,
        account_id: IdForTests::vault_b_id(),
    };

    // Swap token_a in: withdraw_amount = reserve_b * effective_amount_in / (reserve_a +
    // effective_amount_in). With fee_bps=30: effective_amount_in = floor(3 * 9970 / 10000) = 2.
    // reserve_b is large, so `reserve_b * 2 = 2^128` is widened to U256 rather than overflowing.
    let (post_states, _chained_calls) = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        pool,
        vault_a,
        vault_b,
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        3,
        1,
        AMM_PROGRAM_ID,
    );

    let fee_multiplier = FEE_BPS_DENOMINATOR - BalanceForTests::fee_tier();
    let effective_amount_in = mul_div_floor(3, fee_multiplier, FEE_BPS_DENOMINATOR);
    let expected_withdraw = mul_div_floor(
        large_reserve,
        effective_amount_in,
        1_000 + effective_amount_in,
    );
    let pool_def = PoolDefinition::try_from(&post_states[1].account().data).unwrap();
    // token_a in: reserve_a grows by the full swap_amount_in (3); reserve_b shrinks by the
    // withdraw.
    assert_eq!(pool_def.reserve_a, 1_000 + 3);
    assert_eq!(pool_def.reserve_b, large_reserve - expected_withdraw);
}

#[test]
fn test_new_definition_supports_all_fee_tiers() {
    for fees in [
        FEE_TIER_BPS_1,
        FEE_TIER_BPS_5,
        FEE_TIER_BPS_30,
        FEE_TIER_BPS_100,
    ] {
        let (post_states, _) = new_definition(
            AccountWithMetadataForTests::config_init(),
            AccountWithMetadataForTests::pool_definition_uninit(),
            AccountWithMetadataForTests::vault_a_init(),
            AccountWithMetadataForTests::vault_b_init(),
            AccountWithMetadataForTests::pool_lp_uninit(),
            AccountWithMetadataForTests::lp_lock_holding_uninit(),
            AccountWithMetadataForTests::user_holding_a(),
            AccountWithMetadataForTests::user_holding_b(),
            AccountWithMetadataForTests::user_holding_lp_uninit(),
            AccountWithMetadataForTests::current_tick_account_uninit(),
            AccountWithMetadataForTests::clock(),
            NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
            NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
            fees,
            AMM_PROGRAM_ID,
        );

        let pool_post = post_states[1].clone();
        let pool_def = PoolDefinition::try_from(&pool_post.account().data).unwrap();
        assert_eq!(pool_def.fees, fees);
    }
}

#[should_panic(expected = "Fee tier must be one of 1, 5, 30, or 100 basis points")]
#[test]
fn test_new_definition_rejects_unsupported_fee_tier() {
    let _ = new_definition(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_uninit(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_uninit(),
        AccountWithMetadataForTests::lp_lock_holding_uninit(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_uninit(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::vault_a_reserve_init()).unwrap(),
        NonZero::new(BalanceForTests::vault_b_reserve_init()).unwrap(),
        2,
        AMM_PROGRAM_ID,
    );
}

// --- Token program ownership validation tests ---

#[should_panic(expected = "User Token A holding must be owned by the configured Token Program")]
#[test]
fn test_add_liquidity_rejects_user_holding_a_wrong_program() {
    let _ = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a_wrong_program(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).expect("test value must be nonzero"),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token B holding must be owned by the configured Token Program")]
#[test]
fn test_add_liquidity_rejects_user_holding_b_wrong_program() {
    let _ = add_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b_wrong_program(),
        AccountWithMetadataForTests::user_holding_lp_init(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::add_min_amount_lp()).expect("test value must be nonzero"),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::add_max_amount_b(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token A holding must be owned by the configured Token Program")]
#[test]
fn test_remove_liquidity_rejects_user_holding_a_wrong_program() {
    let _ = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a_wrong_program(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_with_balance(
            BalanceForTests::remove_amount_lp(),
        ),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).expect("test value must be nonzero"),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b_low(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token B holding must be owned by the configured Token Program")]
#[test]
fn test_remove_liquidity_rejects_user_holding_b_wrong_program() {
    let _ = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b_wrong_program(),
        AccountWithMetadataForTests::user_holding_lp_with_balance(
            BalanceForTests::remove_amount_lp(),
        ),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).expect("test value must be nonzero"),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b_low(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "Remove amount exceeds user LP balance")]
#[test]
fn test_remove_liquidity_rejects_amount_exceeding_user_lp_balance() {
    let lp_balance = BalanceForTests::remove_amount_lp() - 1;
    let _ = remove_liquidity(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::pool_lp_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::user_holding_lp_with_balance(lp_balance),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        NonZero::new(BalanceForTests::remove_amount_lp()).expect("test value must be nonzero"),
        BalanceForTests::remove_min_amount_a(),
        BalanceForTests::remove_min_amount_b_low(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token A holding must be owned by the configured Token Program")]
#[test]
fn test_swap_exact_input_rejects_user_holding_a_wrong_program() {
    let _ = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a_wrong_program(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token B holding must be owned by the configured Token Program")]
#[test]
fn test_swap_exact_input_rejects_user_holding_b_wrong_program() {
    let _ = swap_exact_input(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b_wrong_program(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        BalanceForTests::add_max_amount_a(),
        BalanceForTests::min_amount_out(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token A holding must be owned by the configured Token Program")]
#[test]
fn test_swap_exact_output_rejects_user_holding_a_wrong_program() {
    let _ = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_exact_output_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a_wrong_program(),
        AccountWithMetadataForTests::user_holding_b(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        166,
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}

#[should_panic(expected = "User Token B holding must be owned by the configured Token Program")]
#[test]
fn test_swap_exact_output_rejects_user_holding_b_wrong_program() {
    let _ = swap_exact_output(
        AccountWithMetadataForTests::config_init(),
        AccountWithMetadataForTests::pool_definition_swap_exact_output_init(),
        AccountWithMetadataForTests::vault_a_init(),
        AccountWithMetadataForTests::vault_b_init(),
        AccountWithMetadataForTests::user_holding_a(),
        AccountWithMetadataForTests::user_holding_b_wrong_program(),
        AccountWithMetadataForTests::current_tick_account_uninit(),
        AccountWithMetadataForTests::clock(),
        166,
        BalanceForTests::max_amount_in(),
        AMM_PROGRAM_ID,
    );
}
