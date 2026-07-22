use amm_program::{
    core::{
        spot_price_q64_64, PoolDefinition, FEE_TIER_BPS_1, FEE_TIER_BPS_100, FEE_TIER_BPS_30,
        FEE_TIER_BPS_5, MINIMUM_LIQUIDITY, SUPPORTED_FEE_TIERS,
    },
    quote::{self, PairOrder, PoolUpdate, QuoteErrorCode, SwapDirection},
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

fn assert_pool_update(
    update: PoolUpdate,
    liquidity_pool_supply: u128,
    reserve_a: u128,
    reserve_b: u128,
) {
    assert_eq!(update.liquidity_pool_supply, liquidity_pool_supply);
    assert_eq!(update.reserve_a, reserve_a);
    assert_eq!(update.reserve_b, reserve_b);
    assert_eq!(
        update.spot_price_q64_64,
        spot_price_q64_64(reserve_a, reserve_b)
    );
}

#[test]
fn create_pool_quotes_locked_and_user_liquidity() {
    let quoted = quote::create_pool(4_000, 9_000, FEE_TIER_BPS_30)
        .expect("valid initial liquidity should quote");

    assert_pool_update(quoted.pool, 6_000, 4_000, 9_000);
    assert_eq!(quoted.locked_liquidity, MINIMUM_LIQUIDITY);
    assert_eq!(quoted.user_liquidity, 5_000);
}

#[test]
fn create_pool_quote_preserves_spot_price_saturation() {
    let quoted = quote::create_pool(1, u128::MAX, FEE_TIER_BPS_30)
        .expect("spot-price range overflow should saturate, not reject the amount quote");

    assert_eq!(quoted.pool.spot_price_q64_64, u128::MAX);
}

#[test]
fn supported_fee_tiers_are_exposed_as_a_slice() {
    let tiers: &[u128] = SUPPORTED_FEE_TIERS;

    assert_eq!(
        tiers,
        &[
            FEE_TIER_BPS_1,
            FEE_TIER_BPS_5,
            FEE_TIER_BPS_30,
            FEE_TIER_BPS_100,
        ]
    );
}

#[test]
fn add_liquidity_quotes_program_rounding_and_post_pool() {
    let quoted = quote::add_liquidity(&pool(), 1_000, 500, 400, 100, 399)
        .expect("valid proportional deposit should quote");

    assert_eq!(quoted.actual_amount_a, 200);
    assert_eq!(quoted.actual_amount_b, 100);
    assert_eq!(quoted.liquidity_to_mint, 400);
    assert_pool_update(quoted.pool, 2_400, 1_200, 600);
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
    let quoted = quote::remove_liquidity(&pool(), 1_000, 500, 250, 125)
        .expect("valid proportional withdrawal should quote");

    assert_eq!(quoted.withdraw_amount_a, 250);
    assert_eq!(quoted.withdraw_amount_b, 125);
    assert_eq!(quoted.liquidity_to_burn, 500);
    assert_pool_update(quoted.pool, 1_500, 750, 375);
}

#[test]
fn exact_input_and_output_quotes_share_the_same_boundary() {
    let exact_input = quote::swap_exact_input(&pool(), 1_000, 500, SwapDirection::AToB, 100, 45)
        .expect("valid exact-input trade should quote");
    let exact_output = quote::swap_exact_output(&pool(), 1_000, 500, SwapDirection::AToB, 45, 100)
        .expect("valid exact-output trade should quote");

    assert_eq!(exact_input, exact_output);
    assert_eq!(exact_input.direction, SwapDirection::AToB);
    assert_eq!(exact_input.amount_in, 100);
    assert_eq!(exact_input.effective_amount_in, 99);
    assert_eq!(exact_input.fee_amount, 1);
    assert_eq!(exact_input.amount_out, 45);
    assert_pool_update(exact_input.pool, 2_000, 1_100, 455);
}

#[test]
fn reverse_swap_quote_keeps_pool_updates_in_stored_order() {
    let quoted = quote::swap_exact_input(&pool(), 1_000, 500, SwapDirection::BToA, 100, 165)
        .expect("valid reverse trade should quote");

    assert_eq!(quoted.direction, SwapDirection::BToA);
    assert_eq!(quoted.amount_in, 100);
    assert_eq!(quoted.effective_amount_in, 99);
    assert_eq!(quoted.fee_amount, 1);
    assert_eq!(quoted.amount_out, 165);
    assert_pool_update(quoted.pool, 2_000, 835, 600);
}

#[test]
fn sync_reserves_reports_donations_and_post_pool() {
    let quoted = quote::sync_reserves(&pool(), 1_100, 550)
        .expect("vault donations above reserves should quote");

    assert_eq!(quoted.donated_amount_a, 100);
    assert_eq!(quoted.donated_amount_b, 50);
    assert_pool_update(quoted.pool, 2_000, 1_100, 550);
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

    assert_eq!(error.kind(), QuoteErrorCode::MintedLiquidityBelowMinimum);
    assert_eq!(error.code(), "minted_liquidity_below_minimum");
    assert_eq!(
        error.message(),
        "Payable LP is less than provided minimum LP amount"
    );
}

#[test]
fn quote_error_codes_have_stable_strings() {
    let cases = [
        (QuoteErrorCode::ArithmeticOverflow, "arithmetic_overflow"),
        (QuoteErrorCode::DepositAmountZero, "deposit_amount_zero"),
        (
            QuoteErrorCode::EffectiveSwapInputZero,
            "effective_swap_input_zero",
        ),
        (
            QuoteErrorCode::ExactOutputExceedsReserve,
            "exact_output_exceeds_reserve",
        ),
        (QuoteErrorCode::ExactOutputZero, "exact_output_zero"),
        (
            QuoteErrorCode::InitialLiquidityTooLow,
            "initial_liquidity_too_low",
        ),
        (
            QuoteErrorCode::InputTokenNotInPool,
            "input_token_not_in_pool",
        ),
        (
            QuoteErrorCode::InvalidLiquidityAccount,
            "invalid_liquidity_account",
        ),
        (
            QuoteErrorCode::LiquiditySupplyBelowMinimum,
            "liquidity_supply_below_minimum",
        ),
        (QuoteErrorCode::MaximumDepositZero, "maximum_deposit_zero"),
        (
            QuoteErrorCode::MinimumLiquidityZero,
            "minimum_liquidity_zero",
        ),
        (
            QuoteErrorCode::MinimumWithdrawalZero,
            "minimum_withdrawal_zero",
        ),
        (
            QuoteErrorCode::MintedLiquidityBelowMinimum,
            "minted_liquidity_below_minimum",
        ),
        (QuoteErrorCode::MintedLiquidityZero, "minted_liquidity_zero"),
        (QuoteErrorCode::OraclePriceZero, "oracle_price_zero"),
        (
            QuoteErrorCode::OracleWindowTooShort,
            "oracle_window_too_short",
        ),
        (
            QuoteErrorCode::PoolContainsOnlyLockedLiquidity,
            "pool_contains_only_locked_liquidity",
        ),
        (
            QuoteErrorCode::RemoveAmountExceedsUnlockedLiquidity,
            "remove_amount_exceeds_unlocked_liquidity",
        ),
        (
            QuoteErrorCode::RemoveAmountExceedsUserBalance,
            "remove_amount_exceeds_user_balance",
        ),
        (
            QuoteErrorCode::RemoveLiquidityAmountZero,
            "remove_liquidity_amount_zero",
        ),
        (
            QuoteErrorCode::RequiredInputExceedsMaximum,
            "required_input_exceeds_maximum",
        ),
        (QuoteErrorCode::ReserveAZero, "reserve_a_zero"),
        (QuoteErrorCode::ReserveZero, "reserve_zero"),
        (
            QuoteErrorCode::SwapOutputBelowMinimum,
            "swap_output_below_minimum",
        ),
        (QuoteErrorCode::SwapOutputZero, "swap_output_zero"),
        (QuoteErrorCode::TokenAAmountZero, "token_a_amount_zero"),
        (QuoteErrorCode::TokenBAmountZero, "token_b_amount_zero"),
        (QuoteErrorCode::TokenPairNotInPool, "token_pair_not_in_pool"),
        (QuoteErrorCode::UnsupportedFeeTier, "unsupported_fee_tier"),
        (
            QuoteErrorCode::VaultABalanceBelowReserve,
            "vault_a_balance_below_reserve",
        ),
        (
            QuoteErrorCode::VaultBBalanceBelowReserve,
            "vault_b_balance_below_reserve",
        ),
        (
            QuoteErrorCode::WithdrawalABelowMinimum,
            "withdrawal_a_below_minimum",
        ),
        (
            QuoteErrorCode::WithdrawalBBelowMinimum,
            "withdrawal_b_below_minimum",
        ),
    ];

    assert_eq!(cases.len(), 33);
    for (kind, expected) in cases {
        assert_eq!(kind.as_str(), expected);
    }
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
