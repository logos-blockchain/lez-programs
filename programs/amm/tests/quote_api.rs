use amm_program::{
    core::{spot_price_q64_64, PoolDefinition, FEE_TIER_BPS_30, MINIMUM_LIQUIDITY},
    quote::{
        self, AddLiquidityQuote, CreatePoolQuote, PairOrder, PoolUpdate, RemoveLiquidityQuote,
        SwapDirection, SwapQuote, SyncReservesQuote,
    },
};
use nssa_core::account::AccountId;
use twap_oracle_core::OBSERVATIONS_CAPACITY;

fn token_a_id() -> AccountId {
    AccountId::new([1; 32])
}

fn token_b_id() -> AccountId {
    AccountId::new([2; 32])
}

fn pool() -> PoolDefinition {
    PoolDefinition {
        definition_token_a_id: token_a_id(),
        definition_token_b_id: token_b_id(),
        vault_a_id: AccountId::new([3; 32]),
        vault_b_id: AccountId::new([4; 32]),
        liquidity_pool_id: AccountId::new([5; 32]),
        liquidity_pool_supply: 2_000,
        reserve_a: 1_000,
        reserve_b: 500,
        fees: FEE_TIER_BPS_30,
    }
}

#[test]
fn create_pool_quotes_locked_and_user_liquidity() {
    assert_eq!(
        quote::create_pool(4_000, 9_000, FEE_TIER_BPS_30),
        Ok(CreatePoolQuote {
            pool: PoolUpdate {
                liquidity_pool_supply: 6_000,
                reserve_a: 4_000,
                reserve_b: 9_000,
                spot_price_q64_64: spot_price_q64_64(4_000, 9_000),
            },
            locked_liquidity: MINIMUM_LIQUIDITY,
            user_liquidity: 5_000,
        })
    );
}

#[test]
fn add_liquidity_quotes_program_rounding_and_post_pool() {
    assert_eq!(
        quote::add_liquidity(&pool(), 1_000, 500, 400, 100, 399),
        Ok(AddLiquidityQuote {
            actual_amount_a: 200,
            actual_amount_b: 100,
            liquidity_to_mint: 400,
            pool: PoolUpdate {
                liquidity_pool_supply: 2_400,
                reserve_a: 1_200,
                reserve_b: 600,
                spot_price_q64_64: spot_price_q64_64(1_200, 600),
            },
        })
    );
}

#[test]
fn preview_helpers_return_amounts_before_client_slippage_policy() {
    let add = quote::preview_add_liquidity(&pool(), 1_000, 500, 400, 100)
        .expect("valid add should preview");
    let remove =
        quote::preview_remove_liquidity(&pool(), 1_000, 500).expect("valid removal should preview");
    let exact_input =
        quote::preview_swap_exact_input(&pool(), 1_000, 500, SwapDirection::AToB, 100)
            .expect("valid exact-input trade should preview");
    let exact_output =
        quote::preview_swap_exact_output(&pool(), 1_000, 500, SwapDirection::AToB, 45)
            .expect("valid exact-output trade should preview");

    assert_eq!(add.liquidity_to_mint, 400);
    assert_eq!(remove.withdraw_amount_a, 250);
    assert_eq!(exact_input.amount_out, 45);
    assert_eq!(exact_output.amount_in, 100);
}

#[test]
fn remove_liquidity_quotes_program_rounding_and_post_pool() {
    assert_eq!(
        quote::remove_liquidity(&pool(), 1_000, 500, 250, 125),
        Ok(RemoveLiquidityQuote {
            withdraw_amount_a: 250,
            withdraw_amount_b: 125,
            liquidity_to_burn: 500,
            pool: PoolUpdate {
                liquidity_pool_supply: 1_500,
                reserve_a: 750,
                reserve_b: 375,
                spot_price_q64_64: spot_price_q64_64(750, 375),
            },
        })
    );
}

#[test]
fn exact_input_and_output_quotes_share_the_same_boundary() {
    let expected = SwapQuote {
        direction: SwapDirection::AToB,
        amount_in: 100,
        effective_amount_in: 99,
        fee_amount: 1,
        amount_out: 45,
        pool: PoolUpdate {
            liquidity_pool_supply: 2_000,
            reserve_a: 1_100,
            reserve_b: 455,
            spot_price_q64_64: spot_price_q64_64(1_100, 455),
        },
    };

    assert_eq!(
        quote::swap_exact_input(&pool(), 1_000, 500, SwapDirection::AToB, 100, 45),
        Ok(expected)
    );
    assert_eq!(
        quote::swap_exact_output(&pool(), 1_000, 500, SwapDirection::AToB, 45, 100),
        Ok(expected)
    );
}

#[test]
fn reverse_swap_quote_keeps_pool_updates_in_stored_order() {
    assert_eq!(
        quote::swap_exact_input(&pool(), 1_000, 500, SwapDirection::BToA, 100, 165),
        Ok(SwapQuote {
            direction: SwapDirection::BToA,
            amount_in: 100,
            effective_amount_in: 99,
            fee_amount: 1,
            amount_out: 165,
            pool: PoolUpdate {
                liquidity_pool_supply: 2_000,
                reserve_a: 835,
                reserve_b: 600,
                spot_price_q64_64: spot_price_q64_64(835, 600),
            },
        })
    );
}

#[test]
fn sync_reserves_reports_donations_and_post_pool() {
    assert_eq!(
        quote::sync_reserves(&pool(), 1_100, 550),
        Ok(SyncReservesQuote {
            donated_amount_a: 100,
            donated_amount_b: 50,
            pool: PoolUpdate {
                liquidity_pool_supply: 2_000,
                reserve_a: 1_100,
                reserve_b: 550,
                spot_price_q64_64: spot_price_q64_64(1_100, 550),
            },
        })
    );
}

#[test]
fn pair_and_swap_direction_follow_stored_pool_order() {
    let pool = pool();

    assert_eq!(
        quote::pair_order(&pool, token_a_id(), token_b_id()),
        Ok(PairOrder::Stored)
    );
    assert_eq!(
        quote::pair_order(&pool, token_b_id(), token_a_id()),
        Ok(PairOrder::Reversed)
    );
    assert_eq!(
        quote::swap_direction(&pool, token_a_id()),
        Ok(SwapDirection::AToB)
    );
    assert_eq!(
        quote::swap_direction(&pool, token_b_id()),
        Ok(SwapDirection::BToA)
    );
}

#[test]
fn oracle_price_quote_uses_pool_assets_and_spot_price() {
    let window_duration = u64::from(OBSERVATIONS_CAPACITY);
    let result = quote::create_oracle_price_account(&pool(), window_duration)
        .expect("valid pool and window should quote");

    assert_eq!(result.base_asset, token_a_id());
    assert_eq!(result.quote_asset, token_b_id());
    assert_eq!(result.initial_price_q64_64, spot_price_q64_64(1_000, 500));
    assert_eq!(result.window_duration, window_duration);
}

#[test]
fn quote_errors_expose_stable_machine_codes() {
    let error = quote::add_liquidity(&pool(), 1_000, 500, 400, 100, 401)
        .expect_err("minimum above minted liquidity must fail");

    assert_eq!(error.code(), "minted_liquidity_below_minimum");
    assert_eq!(
        error.message(),
        "Payable LP is less than provided minimum LP amount"
    );
}

#[test]
fn exact_quotes_apply_instruction_slippage_guards() {
    let add = quote::add_liquidity(&pool(), 1_000, 500, 400, 100, 401)
        .expect_err("minimum LP above quote must fail");
    let remove = quote::remove_liquidity(&pool(), 1_000, 500, 251, 125)
        .expect_err("minimum token A above quote must fail");
    let exact_input = quote::swap_exact_input(&pool(), 1_000, 500, SwapDirection::AToB, 100, 46)
        .expect_err("minimum output above quote must fail");
    let exact_output = quote::swap_exact_output(&pool(), 1_000, 500, SwapDirection::AToB, 45, 99)
        .expect_err("maximum input below quote must fail");

    assert_eq!(add.code(), "minted_liquidity_below_minimum");
    assert_eq!(remove.code(), "withdrawal_a_below_minimum");
    assert_eq!(exact_input.code(), "swap_output_below_minimum");
    assert_eq!(exact_output.code(), "required_input_exceeds_maximum");
}

#[test]
fn arithmetic_overflow_is_returned_instead_of_panicking() {
    let mut extreme_pool = pool();
    extreme_pool.reserve_a = u128::MAX;
    extreme_pool.reserve_b = 1;

    let error = quote::add_liquidity(&extreme_pool, u128::MAX, 1, u128::MAX, u128::MAX, 1)
        .expect_err("unrepresentable ideal amount must fail");

    assert_eq!(error.code(), "arithmetic_overflow");
}
