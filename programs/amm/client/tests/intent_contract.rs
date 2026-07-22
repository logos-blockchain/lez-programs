use amm_client::{
    caller_amounts_to_stored, paired_amount_from_token_a, paired_amount_from_token_b,
    pool_spot_change_bps, prepare_caller_opening_pair, prepare_minimum_opening_pair,
    prepare_opening_from_token_a, prepare_opening_from_token_b, stored_amounts_to_caller,
    validate_explicit_opening_pair, IntentError, OpeningLiquidityIntent, Q64_64_ONE,
};
use amm_core::{PoolDefinition, MINIMUM_LIQUIDITY};
use amm_program::quote::{self as program_quote, PairOrder, SwapDirection};
use nssa_core::account::AccountId;

const FEE_BPS: u128 = 30;

fn pool(reserve_a: u128, reserve_b: u128) -> PoolDefinition {
    PoolDefinition {
        definition_token_a_id: AccountId::new([1; 32]),
        definition_token_b_id: AccountId::new([2; 32]),
        vault_a_id: AccountId::new([3; 32]),
        vault_b_id: AccountId::new([4; 32]),
        liquidity_pool_id: AccountId::new([5; 32]),
        liquidity_pool_supply: MINIMUM_LIQUIDITY
            .checked_mul(100)
            .expect("test liquidity supply fits u128"),
        reserve_a,
        reserve_b,
        fees: FEE_BPS,
    }
}

#[test]
fn minimum_pair_handles_prices_below_equal_and_above_one() {
    for price in [Q64_64_ONE - 1, Q64_64_ONE, Q64_64_ONE + 1] {
        let prepared = prepare_minimum_opening_pair(price, FEE_BPS).unwrap();
        assert!(prepared.quote.user_liquidity > 0);
        assert!(prepared.token_a_amount > 0);
        assert!(prepared.token_b_amount > 0);

        if price >= Q64_64_ONE && prepared.token_a_amount > 1 {
            let previous_a = prepared.token_a_amount.checked_sub(1).unwrap();
            let previous_b = paired_amount_from_token_a(previous_a, price).unwrap();
            assert!(program_quote::create_pool(previous_a, previous_b, FEE_BPS).is_err());
        } else if price < Q64_64_ONE && prepared.token_b_amount > 1 {
            let previous_b = prepared.token_b_amount.checked_sub(1).unwrap();
            let previous_a = paired_amount_from_token_b(previous_b, price).unwrap();
            assert!(program_quote::create_pool(previous_a, previous_b, FEE_BPS).is_err());
        }
    }
}

#[test]
fn zero_price_and_zero_edited_amount_are_rejected() {
    assert_eq!(
        prepare_minimum_opening_pair(0, FEE_BPS),
        Err(IntentError::ZeroDesiredPrice)
    );
    assert_eq!(
        paired_amount_from_token_a(0, Q64_64_ONE),
        Err(IntentError::ZeroEditedAmount)
    );
    assert_eq!(
        paired_amount_from_token_b(1, 0),
        Err(IntentError::ZeroDesiredPrice)
    );
}

#[test]
fn pairing_uses_checked_widened_math_and_reports_overflow() {
    assert_eq!(
        paired_amount_from_token_a(u128::MAX, u128::MAX),
        Err(IntentError::ArithmeticOverflow {
            operation: "token-A to token-B pairing"
        })
    );
    assert_eq!(
        paired_amount_from_token_b(u128::MAX, 1),
        Err(IntentError::ArithmeticOverflow {
            operation: "token-B to token-A pairing"
        })
    );
}

#[test]
fn paired_and_explicit_amounts_are_validated_by_program_quote() {
    let from_a = prepare_opening_from_token_a(2_000, Q64_64_ONE * 2, FEE_BPS).unwrap();
    assert_eq!(from_a.token_b_amount, 4_000);
    assert_eq!(from_a.quote.pool.reserve_b, 4_000);

    let from_b = prepare_opening_from_token_b(4_000, Q64_64_ONE * 2, FEE_BPS).unwrap();
    assert_eq!(from_b.token_a_amount, 2_000);

    let explicit = validate_explicit_opening_pair(2_000, 4_000, Q64_64_ONE * 2, FEE_BPS).unwrap();
    assert_eq!(explicit.actual_price_q64_64, Q64_64_ONE * 2);

    let mismatch =
        validate_explicit_opening_pair(2_000, 4_001, Q64_64_ONE * 2, FEE_BPS).unwrap_err();
    assert!(matches!(mismatch, IntentError::SpotPriceMismatch { .. }));

    let too_small = prepare_opening_from_token_a(1, Q64_64_ONE, FEE_BPS).unwrap_err();
    assert!(matches!(too_small, IntentError::Quote { .. }));
}

#[test]
fn amounts_above_javascript_integer_range_remain_exact() {
    let amount_a = 1_u128 << 80;
    let amount_b = amount_a.checked_mul(2).unwrap();
    let prepared =
        validate_explicit_opening_pair(amount_a, amount_b, Q64_64_ONE * 2, FEE_BPS).unwrap();
    assert_eq!(prepared.token_a_amount, amount_a);
    assert_eq!(prepared.token_b_amount, amount_b);
    assert_eq!(prepared.quote.pool.reserve_a, amount_a);
    assert_eq!(prepared.quote.pool.reserve_b, amount_b);
}

#[test]
fn caller_and_stored_order_mapping_is_lossless() {
    assert_eq!(
        caller_amounts_to_stored(PairOrder::Stored, 11, 22),
        (11, 22)
    );
    assert_eq!(
        caller_amounts_to_stored(PairOrder::Reversed, 11, 22),
        (22, 11)
    );
    assert_eq!(
        stored_amounts_to_caller(PairOrder::Reversed, 22, 11),
        (11, 22)
    );
}

#[test]
fn caller_opening_intent_maps_both_token_orders_without_host_math() {
    let lower = AccountId::new([1; 32]);
    let higher = AccountId::new([2; 32]);
    let desired_price = Q64_64_ONE.checked_mul(2).unwrap();

    let reversed = prepare_caller_opening_pair(
        lower,
        higher,
        desired_price,
        FEE_BPS,
        OpeningLiquidityIntent::FirstAmount(4_000),
    )
    .unwrap();
    assert_eq!(reversed.caller_order(), PairOrder::Reversed);
    assert_eq!(reversed.first_amount(), 4_000);
    assert_eq!(reversed.second_amount(), 2_000);
    assert_eq!(reversed.stored().token_a_amount, 2_000);
    assert_eq!(reversed.stored().token_b_amount, 4_000);

    let stored = prepare_caller_opening_pair(
        higher,
        lower,
        desired_price,
        FEE_BPS,
        OpeningLiquidityIntent::Explicit {
            first_amount: 2_000,
            second_amount: 4_000,
        },
    )
    .unwrap();
    assert_eq!(stored.caller_order(), PairOrder::Stored);
    assert_eq!(stored.first_amount(), 2_000);
    assert_eq!(stored.second_amount(), 4_000);

    assert_eq!(
        prepare_caller_opening_pair(
            lower,
            lower,
            desired_price,
            FEE_BPS,
            OpeningLiquidityIntent::Minimum,
        ),
        Err(IntentError::IdenticalTokenDefinitions)
    );
}

#[test]
fn pool_spot_change_is_directional_exact_and_floored_once() {
    let before = pool(10_000, 20_000);
    let quote = program_quote::preview_swap_exact_input(
        &before,
        before.reserve_a,
        before.reserve_b,
        SwapDirection::AToB,
        100,
    )
    .unwrap();

    assert_eq!(quote.pool.reserve_a, 10_100);
    assert_eq!(quote.pool.reserve_b, 19_804);
    assert_eq!(pool_spot_change_bps(&before, &quote).unwrap(), 199);

    let large_quote = program_quote::preview_swap_exact_input(
        &before,
        before.reserve_a,
        before.reserve_b,
        SwapDirection::AToB,
        9_000,
    )
    .unwrap();
    assert!(pool_spot_change_bps(&before, &large_quote).unwrap() > 10_000);
}

#[test]
fn pool_spot_change_handles_reserves_above_javascript_range() {
    let scale = 1_u128 << 60;
    let reserve_a = scale.checked_mul(10).unwrap();
    let reserve_b = scale.checked_mul(20).unwrap();
    let before = pool(reserve_a, reserve_b);
    let quote = program_quote::preview_swap_exact_input(
        &before,
        before.reserve_a,
        before.reserve_b,
        SwapDirection::BToA,
        scale,
    )
    .unwrap();

    let change = pool_spot_change_bps(&before, &quote).unwrap();
    assert!(change > 0);
    assert!(change <= 10_000);
}

#[test]
fn pool_spot_change_rejects_zero_directional_reserve() {
    let valid_before = pool(10_000, 20_000);
    let quote = program_quote::preview_swap_exact_input(
        &valid_before,
        valid_before.reserve_a,
        valid_before.reserve_b,
        SwapDirection::AToB,
        100,
    )
    .unwrap();
    let zero_before = pool(0, 20_000);

    assert_eq!(
        pool_spot_change_bps(&zero_before, &quote),
        Err(IntentError::ZeroDirectionalReserve)
    );
    assert_eq!(
        IntentError::ZeroDirectionalReserve.code(),
        "zero_directional_reserve"
    );
}
