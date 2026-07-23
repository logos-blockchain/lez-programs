use amm_client::{human_price_ratio_to_q64_64, wire::quote_json, Q64_64_ONE};
use amm_core::canonical_token_pair;
use nssa_core::account::AccountId;
use serde_json::json;

#[test]
fn human_price_conversion_handles_large_values_order_and_decimals() {
    let first_id = AccountId::new([1; 32]);
    let second_id = AccountId::new([2; 32]);
    let (stored_a_id, stored_b_id) =
        canonical_token_pair(first_id, second_id).expect("tokens are distinct");
    let amount_a = "9007199254740993";
    let amount_b = "18014398509481986";
    let expected = Q64_64_ONE
        .checked_mul(2_000_000_000_000)
        .expect("expected Q64.64 price fits");

    let stored = human_price_ratio_to_q64_64(stored_a_id, stored_b_id, amount_a, amount_b, 6, 18)
        .expect("stored-order price must convert");
    let reversed = human_price_ratio_to_q64_64(stored_b_id, stored_a_id, amount_b, amount_a, 18, 6)
        .expect("reversed caller price must convert");

    assert_eq!(stored, expected);
    assert_eq!(reversed, expected);

    let inverse_decimal_scale =
        human_price_ratio_to_q64_64(stored_a_id, stored_b_id, "1", "2", 18, 6)
            .expect("negative decimal exponent must convert");
    let inverse_expected = Q64_64_ONE
        .checked_mul(2)
        .and_then(|value| value.checked_div(1_000_000_000_000))
        .expect("inverse decimal-scale expectation fits");
    assert_eq!(inverse_decimal_scale, inverse_expected);

    let wire = quote_json(json!({
        "operation": "human_price_ratio_to_q64_64",
        "firstTokenDefinitionId": stored_b_id.to_string(),
        "secondTokenDefinitionId": stored_a_id.to_string(),
        "firstAmount": amount_b,
        "secondAmount": amount_a,
        "firstTokenDecimals": "18",
        "secondTokenDecimals": "6",
    }))
    .expect("wire price conversion must use canonical stored order");
    assert_eq!(wire["priceQ64_64"], expected.to_string());
}
