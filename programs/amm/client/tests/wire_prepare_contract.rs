mod common;

use amm_client::{maximum_guard_amount, minimum_guard_amount, wire::quote_json, SlippageTolerance};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_pool_pda, compute_vault_pda,
    AmmConfig, PoolDefinition, FEE_TIER_BPS_30,
};
use common::program_id_hex;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{TokenDefinition, TokenHolding};

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn snapshot(id: AccountId, account: &Account) -> Value {
    json!({
        "id": id.to_string(),
        "programOwner": program_id_hex(account.program_owner),
        "balance": account.balance.to_string(),
        "nonce": account.nonce.0.to_string(),
        "data": account
            .data
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    })
}

fn definition(total_supply: u128, authority: Option<AccountId>) -> Account {
    account(
        TOKEN_PROGRAM_ID,
        Data::from(&TokenDefinition::Fungible {
            name: String::from("Token"),
            total_supply,
            metadata_id: None,
            authority,
        }),
    )
}

fn holding(definition_id: AccountId, balance: u128) -> Account {
    account(
        TOKEN_PROGRAM_ID,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance,
        }),
    )
}

struct WireFixture {
    token_a_id: AccountId,
    token_b_id: AccountId,
    config: Value,
    state: Value,
    user_a: Value,
    user_b: Value,
    user_lp: Value,
}

impl WireFixture {
    fn new() -> Self {
        let token_a_id = AccountId::new([1; 32]);
        let token_b_id = AccountId::new([2; 32]);
        let pool_id = compute_pool_pda(AMM_PROGRAM_ID, token_a_id, token_b_id);
        let vault_a_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, token_a_id);
        let vault_b_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, token_b_id);
        let liquidity_id = compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id);
        let config = AmmConfig {
            token_program_id: TOKEN_PROGRAM_ID,
            twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
            authority: AccountId::new([9; 32]),
        };
        let config = snapshot(
            compute_config_pda(AMM_PROGRAM_ID),
            &account(AMM_PROGRAM_ID, Data::from(&config)),
        );
        let pool = PoolDefinition {
            definition_token_a_id: token_a_id,
            definition_token_b_id: token_b_id,
            vault_a_id,
            vault_b_id,
            liquidity_pool_id: liquidity_id,
            liquidity_pool_supply: 2_000,
            reserve_a: 1_000,
            reserve_b: 500,
            fees: FEE_TIER_BPS_30,
        };
        let state = json!({
            "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
            "config": config,
            "snapshot": {
                "pool": snapshot(pool_id, &account(AMM_PROGRAM_ID, Data::from(&pool))),
                "tokenADefinition": snapshot(token_a_id, &definition(100_000, None)),
                "tokenBDefinition": snapshot(token_b_id, &definition(100_000, None)),
                "vaultA": snapshot(vault_a_id, &holding(token_a_id, 1_100)),
                "vaultB": snapshot(vault_b_id, &holding(token_b_id, 550)),
                "liquidityDefinition": snapshot(
                    liquidity_id,
                    &definition(2_000, Some(liquidity_id)),
                ),
            },
        });

        Self {
            token_a_id,
            token_b_id,
            config,
            state,
            user_a: snapshot(AccountId::new([20; 32]), &holding(token_a_id, 10_000)),
            user_b: snapshot(AccountId::new([21; 32]), &holding(token_b_id, 10_000)),
            user_lp: snapshot(AccountId::new([22; 32]), &holding(liquidity_id, 1_000)),
        }
    }

    fn request(&self, operation: &str) -> Value {
        let mut request = self.state.clone();
        insert(
            &mut request,
            "operation",
            Value::String(String::from(operation)),
        );
        request
    }
}

fn insert(object: &mut Value, field: &str, value: Value) {
    drop(
        object
            .as_object_mut()
            .expect("fixture request must be an object")
            .insert(String::from(field), value),
    );
}

fn decimal(value: &Value) -> u128 {
    value
        .as_str()
        .expect("chain amounts must be JSON strings")
        .parse()
        .expect("chain amounts must be decimal u128")
}

#[test]
fn prepare_wire_operations_return_lossless_instruction_args() {
    let fixture = WireFixture::new();
    let tolerance = SlippageTolerance::new(100).expect("one percent is valid");
    let large = 9_007_199_254_740_993_u128;

    let create = quote_json(json!({
        "operation": "prepare_create_pool",
        "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
        "config": fixture.config.clone(),
        "tokenADefinition": snapshot(fixture.token_a_id, &definition(100_000, None)),
        "tokenBDefinition": snapshot(fixture.token_b_id, &definition(100_000, None)),
        "tokenAAmount": large.to_string(),
        "tokenBAmount": large.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
    }))
    .expect("create pool must prepare");
    assert_eq!(create["instructionArgs"]["tokenAAmount"], large.to_string());
    assert_eq!(create["instructionArgs"]["tokenBAmount"], large.to_string());
    assert_eq!(create["instructionArgs"]["fees"], "30");

    let mut add_request = fixture.request("prepare_add_liquidity");
    insert(&mut add_request, "maxAmountA", json!("400"));
    insert(&mut add_request, "maxAmountB", json!("100"));
    insert(&mut add_request, "slippageBps", json!("100"));
    let add = quote_json(add_request).expect("add liquidity must prepare");
    assert_eq!(
        decimal(&add["instructionArgs"]["minAmountLiquidity"]),
        minimum_guard_amount(decimal(&add["quote"]["liquidityToMint"]), tolerance)
            .expect("minimum LP guard must fit")
    );
    assert_eq!(add["instructionArgs"]["maxAmountToAddTokenA"], "400");
    assert_eq!(add["instructionArgs"]["maxAmountToAddTokenB"], "100");

    let mut remove_request = fixture.request("prepare_remove_liquidity");
    insert(
        &mut remove_request,
        "userLiquidityHolding",
        fixture.user_lp.clone(),
    );
    insert(&mut remove_request, "removeLiquidityAmount", json!("500"));
    insert(&mut remove_request, "slippageBps", json!("100"));
    let remove = quote_json(remove_request).expect("remove liquidity must prepare");
    assert_eq!(remove["instructionArgs"]["removeLiquidityAmount"], "500");
    assert_eq!(
        decimal(&remove["instructionArgs"]["minAmountToRemoveTokenA"]),
        minimum_guard_amount(decimal(&remove["quote"]["withdrawAmountA"]), tolerance)
            .expect("minimum A guard must fit")
    );
    assert_eq!(
        decimal(&remove["instructionArgs"]["minAmountToRemoveTokenB"]),
        minimum_guard_amount(decimal(&remove["quote"]["withdrawAmountB"]), tolerance)
            .expect("minimum B guard must fit")
    );

    let mut exact_input_request = fixture.request("prepare_swap_exact_input");
    insert(
        &mut exact_input_request,
        "userInputHolding",
        fixture.user_a.clone(),
    );
    insert(
        &mut exact_input_request,
        "userOutputHolding",
        fixture.user_b.clone(),
    );
    insert(
        &mut exact_input_request,
        "inputTokenDefinitionId",
        json!(fixture.token_a_id.to_string()),
    );
    insert(&mut exact_input_request, "amountIn", json!("100"));
    insert(&mut exact_input_request, "slippageBps", json!("100"));
    let exact_input = quote_json(exact_input_request).expect("exact-input swap must prepare");
    assert_eq!(exact_input["instructionArgs"]["swapAmountIn"], "100");
    assert_eq!(
        decimal(&exact_input["instructionArgs"]["minAmountOut"]),
        minimum_guard_amount(decimal(&exact_input["quote"]["amountOut"]), tolerance)
            .expect("minimum output guard must fit")
    );

    let mut exact_output_request = fixture.request("prepare_swap_exact_output");
    insert(
        &mut exact_output_request,
        "userInputHolding",
        fixture.user_a,
    );
    insert(
        &mut exact_output_request,
        "userOutputHolding",
        fixture.user_b,
    );
    insert(
        &mut exact_output_request,
        "inputTokenDefinitionId",
        json!(fixture.token_a_id.to_string()),
    );
    insert(&mut exact_output_request, "exactAmountOut", json!("45"));
    insert(&mut exact_output_request, "slippageBps", json!("100"));
    let exact_output = quote_json(exact_output_request).expect("exact-output swap must prepare");
    assert_eq!(exact_output["instructionArgs"]["exactAmountOut"], "45");
    assert_eq!(
        decimal(&exact_output["instructionArgs"]["maxAmountIn"]),
        maximum_guard_amount(decimal(&exact_output["quote"]["amountIn"]), tolerance)
            .expect("maximum input guard must fit")
    );
}

#[test]
fn prepare_wire_rejects_out_of_range_slippage() {
    let fixture = WireFixture::new();
    let mut request = fixture.request("prepare_add_liquidity");
    insert(&mut request, "maxAmountA", json!("400"));
    insert(&mut request, "maxAmountB", json!("100"));
    insert(&mut request, "slippageBps", json!("10001"));

    let error = quote_json(request).expect_err("invalid slippage must be rejected");
    assert_eq!(error.code(), "slippage_tolerance_out_of_range");
}
