mod common;

use amm_client::wire::{plan_json, quote_json};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, AmmConfig, Instruction, PoolDefinition, FEE_TIER_BPS_30, MINIMUM_LIQUIDITY,
};
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use common::program_id_hex;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

const AMM_PROGRAM_ID: ProgramId = [42; 8];
const TOKEN_PROGRAM_ID: ProgramId = [15; 8];
const TWAP_ORACLE_PROGRAM_ID: ProgramId = [77; 8];
const LARGE: u128 = 9_007_199_254_740_993;
const DEADLINE: u64 = 9_007_199_254_740_993;

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
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

struct TransactionFixture {
    first_token_id: AccountId,
    second_token_id: AccountId,
    pool_id: AccountId,
    first_vault_id: AccountId,
    second_vault_id: AccountId,
    liquidity_definition_id: AccountId,
    lp_lock_holding_id: AccountId,
    current_tick_id: AccountId,
    first_holding_id: AccountId,
    second_holding_id: AccountId,
    liquidity_holding_id: AccountId,
    fresh_liquidity_holding_id: AccountId,
    config: Value,
    active_snapshots: Value,
    missing_snapshots: Value,
    first_holding: Value,
    second_holding: Value,
    liquidity_holding: Value,
    fresh_liquidity_holding: Value,
}

impl TransactionFixture {
    fn new() -> Self {
        let first_token_id = AccountId::new([1; 32]);
        let second_token_id = AccountId::new([2; 32]);
        let pool_id = compute_pool_pda(AMM_PROGRAM_ID, second_token_id, first_token_id);
        let first_vault_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, first_token_id);
        let second_vault_id = compute_vault_pda(AMM_PROGRAM_ID, pool_id, second_token_id);
        let liquidity_definition_id = compute_liquidity_token_pda(AMM_PROGRAM_ID, pool_id);
        let lp_lock_holding_id = compute_lp_lock_holding_pda(AMM_PROGRAM_ID, pool_id);
        let current_tick_id = compute_current_tick_account_pda(TWAP_ORACLE_PROGRAM_ID, pool_id);
        let first_holding_id = AccountId::new([20; 32]);
        let second_holding_id = AccountId::new([21; 32]);
        let liquidity_holding_id = AccountId::new([22; 32]);
        let fresh_liquidity_holding_id = AccountId::new([30; 32]);
        let total_supply = LARGE.checked_mul(10).expect("test supply fits");
        let config = snapshot(
            compute_config_pda(AMM_PROGRAM_ID),
            &account(
                AMM_PROGRAM_ID,
                Data::from(&AmmConfig {
                    token_program_id: TOKEN_PROGRAM_ID,
                    twap_oracle_program_id: TWAP_ORACLE_PROGRAM_ID,
                    authority: AccountId::new([9; 32]),
                }),
            ),
        );
        let clock_bytes = ClockAccountData {
            block_id: 123,
            timestamp: 456,
        }
        .to_bytes();
        let clock = snapshot(
            CLOCK_01_PROGRAM_ACCOUNT_ID,
            &account(
                [88; 8],
                Data::try_from(clock_bytes).expect("clock data must fit"),
            ),
        );
        let first_definition = snapshot(first_token_id, &definition(total_supply, None));
        let second_definition = snapshot(second_token_id, &definition(total_supply, None));

        let pool = PoolDefinition {
            definition_token_a_id: second_token_id,
            definition_token_b_id: first_token_id,
            vault_a_id: second_vault_id,
            vault_b_id: first_vault_id,
            liquidity_pool_id: liquidity_definition_id,
            liquidity_pool_supply: 2_000,
            reserve_a: 1_000,
            reserve_b: 500,
            fees: FEE_TIER_BPS_30,
        };
        let active_snapshots = json!({
            "pool": snapshot(pool_id, &account(AMM_PROGRAM_ID, Data::from(&pool))),
            "firstTokenDefinition": first_definition.clone(),
            "secondTokenDefinition": second_definition.clone(),
            "firstTokenVault": snapshot(first_vault_id, &holding(first_token_id, 550)),
            "secondTokenVault": snapshot(second_vault_id, &holding(second_token_id, 1_100)),
            "liquidityDefinition": snapshot(
                liquidity_definition_id,
                &definition(2_000, Some(liquidity_definition_id)),
            ),
            "lpLockHolding": snapshot(
                lp_lock_holding_id,
                &holding(liquidity_definition_id, MINIMUM_LIQUIDITY),
            ),
            "currentTick": snapshot(
                current_tick_id,
                &account(
                    TWAP_ORACLE_PROGRAM_ID,
                    Data::from(&CurrentTickAccount {
                        tick: -1,
                        last_updated: 400,
                    }),
                ),
            ),
            "clock": clock.clone(),
        });
        let missing_snapshots = json!({
            "pool": snapshot(pool_id, &Account::default()),
            "firstTokenDefinition": first_definition,
            "secondTokenDefinition": second_definition,
            "firstTokenVault": snapshot(first_vault_id, &Account::default()),
            "secondTokenVault": snapshot(second_vault_id, &Account::default()),
            "liquidityDefinition": snapshot(liquidity_definition_id, &Account::default()),
            "lpLockHolding": snapshot(lp_lock_holding_id, &Account::default()),
            "currentTick": snapshot(current_tick_id, &Account::default()),
            "clock": clock,
        });
        let holding_balance = LARGE.checked_mul(3).expect("test balance fits");

        Self {
            first_token_id,
            second_token_id,
            pool_id,
            first_vault_id,
            second_vault_id,
            liquidity_definition_id,
            lp_lock_holding_id,
            current_tick_id,
            first_holding_id,
            second_holding_id,
            liquidity_holding_id,
            fresh_liquidity_holding_id,
            config,
            active_snapshots,
            missing_snapshots,
            first_holding: snapshot(first_holding_id, &holding(first_token_id, holding_balance)),
            second_holding: snapshot(
                second_holding_id,
                &holding(second_token_id, holding_balance),
            ),
            liquidity_holding: snapshot(
                liquidity_holding_id,
                &holding(liquidity_definition_id, 1_000),
            ),
            fresh_liquidity_holding: snapshot(fresh_liquidity_holding_id, &Account::default()),
        }
    }

    fn active_common(&self, operation: &str) -> Value {
        json!({
            "operation": operation,
            "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
            "config": self.config.clone(),
            "snapshots": self.active_snapshots.clone(),
            "firstTokenDefinitionId": self.first_token_id.to_string(),
            "secondTokenDefinitionId": self.second_token_id.to_string(),
            "firstTokenHolding": self.first_holding.clone(),
            "secondTokenHolding": self.second_holding.clone(),
            "liquidityHolding": self.liquidity_holding.clone(),
            "slippageBps": "100",
            "expectedFeeBps": FEE_TIER_BPS_30.to_string(),
            "deadline": DEADLINE.to_string(),
        })
    }

    fn swap_common(&self, operation: &str) -> Value {
        json!({
            "operation": operation,
            "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
            "config": self.config.clone(),
            "snapshots": self.active_snapshots.clone(),
            "inputTokenDefinitionId": self.first_token_id.to_string(),
            "outputTokenDefinitionId": self.second_token_id.to_string(),
            "inputHolding": self.first_holding.clone(),
            "outputHolding": self.second_holding.clone(),
            "slippageBps": "100",
            "expectedFeeBps": FEE_TIER_BPS_30.to_string(),
            "deadline": DEADLINE.to_string(),
        })
    }
}

fn insert(value: &mut Value, field: &str, inserted: Value) {
    drop(
        value
            .as_object_mut()
            .expect("request must be an object")
            .insert(String::from(field), inserted),
    );
}

fn decode_instruction(response: &Value) -> Instruction {
    let words = response
        .pointer("/plan/instructionWords")
        .expect("plan must contain instruction words")
        .clone();
    let words: Vec<u32> =
        serde_json::from_value(words).expect("plan instruction words must be u32 JSON values");
    risc0_zkvm::serde::from_slice(&words).expect("guest codec must decode wire plan")
}

fn assert_instruction_arg(response: &Value, name: &str, expected: impl ToString) {
    let pointer = format!("/plan/instructionArgs/{name}");
    assert_eq!(
        response
            .pointer(&pointer)
            .and_then(Value::as_str)
            .expect("typed instruction argument must be a string"),
        expected.to_string()
    );
}

fn assert_common_contract(response: &Value, operation: &str, expect_spot_change: bool) {
    assert_eq!(response["operation"], operation);
    assert_eq!(response["deadline"], DEADLINE.to_string());
    assert!(response["quote"].is_object());
    assert!(response
        .pointer("/callerAmounts/first")
        .is_some_and(Value::is_string));
    assert!(response
        .pointer("/callerAmounts/second")
        .is_some_and(Value::is_string));
    assert!(response
        .pointer("/plan/accounts")
        .is_some_and(Value::is_array));
    assert!(response
        .pointer("/plan/instructionArgs")
        .is_some_and(Value::is_object));
    assert_eq!(
        response["affectedAccountIds"],
        *response
            .pointer("/plan/affectedAccountIds")
            .expect("plan must contain affected account IDs")
    );
    assert!(response
        .pointer("/walletPrerequisites/signerAccountIds")
        .is_some_and(Value::is_array));
    assert!(response
        .pointer("/walletPrerequisites/freshAccountIds")
        .is_some_and(Value::is_array));
    assert!(response
        .pointer("/walletPrerequisites/funding")
        .is_some_and(Value::is_array));

    let commitment = response["quoteCommitment"]
        .as_str()
        .expect("commitment must be a hex string");
    assert_eq!(commitment.len(), 64);
    assert!(commitment
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_eq!(
        response["poolSpotChangeBps"].is_string(),
        expect_spot_change
    );
    assert_eq!(response["poolSpotChangeBps"].is_null(), !expect_spot_change);
}

#[test]
fn five_transaction_operations_emit_exact_plans_and_task_artifacts() {
    let fixture = TransactionFixture::new();

    let second_amount = LARGE.checked_mul(2).expect("test amount fits");
    let create = plan_json(json!({
        "operation": "prepare_create_pool_transaction",
        "ammProgramId": program_id_hex(AMM_PROGRAM_ID),
        "config": fixture.config.clone(),
        "snapshots": fixture.missing_snapshots.clone(),
        "firstTokenDefinitionId": fixture.first_token_id.to_string(),
        "secondTokenDefinitionId": fixture.second_token_id.to_string(),
        "firstTokenHolding": fixture.first_holding.clone(),
        "secondTokenHolding": fixture.second_holding.clone(),
        "liquidityHolding": fixture.fresh_liquidity_holding.clone(),
        "firstAmount": LARGE.to_string(),
        "secondAmount": second_amount.to_string(),
        "feeBps": FEE_TIER_BPS_30.to_string(),
        "deadline": DEADLINE.to_string(),
    }))
    .expect("create transaction must prepare");
    assert_common_contract(&create, "create_pool", false);
    assert_eq!(create["callerAmounts"]["first"], LARGE.to_string());
    assert_eq!(create["callerAmounts"]["second"], second_amount.to_string());
    assert_eq!(
        create["walletPrerequisites"]["freshAccountIds"],
        json!([fixture.fresh_liquidity_holding_id.to_string()])
    );
    assert_eq!(
        create["walletPrerequisites"]["funding"][0]["required"],
        LARGE.to_string()
    );
    match decode_instruction(&create) {
        Instruction::NewDefinition {
            token_a_amount,
            token_b_amount,
            deadline,
            ..
        } => {
            assert_eq!(token_a_amount, second_amount);
            assert_eq!(token_b_amount, LARGE);
            assert_eq!(deadline, DEADLINE);
            assert_instruction_arg(&create, "tokenAAmount", token_a_amount);
            assert_instruction_arg(&create, "tokenBAmount", token_b_amount);
            assert_instruction_arg(&create, "fees", FEE_TIER_BPS_30);
            assert_instruction_arg(&create, "deadline", deadline);
        }
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreatePriceObservations { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::AddLiquidity { .. }
        | Instruction::RemoveLiquidity { .. }
        | Instruction::SwapExactInput { .. }
        | Instruction::SwapExactOutput { .. }
        | Instruction::SyncReserves => {
            panic!("create wire operation emitted wrong instruction")
        }
    }

    let mut add_request = fixture.active_common("prepare_add_liquidity_transaction");
    insert(&mut add_request, "maxFirstAmount", json!("100"));
    insert(&mut add_request, "maxSecondAmount", json!("400"));
    let quote_error = quote_json(add_request.clone())
        .expect_err("quote endpoint must reject transaction preparation");
    assert_eq!(quote_error.code(), "invalid_request");
    let add = plan_json(add_request).expect("add transaction must prepare");
    assert_common_contract(&add, "add_liquidity", false);
    assert_eq!(add["callerAmounts"]["first"], "100");
    assert_eq!(add["callerAmounts"]["second"], "200");
    assert_eq!(
        add.pointer("/walletPrerequisites/funding")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        add.pointer("/walletPrerequisites/funding/0/required"),
        Some(&json!("100"))
    );
    assert_eq!(
        add.pointer("/walletPrerequisites/funding/1/required"),
        Some(&json!("400"))
    );
    match decode_instruction(&add) {
        Instruction::AddLiquidity {
            min_amount_liquidity,
            max_amount_to_add_token_a,
            max_amount_to_add_token_b,
            deadline,
        } => {
            assert_eq!(max_amount_to_add_token_a, 400);
            assert_eq!(max_amount_to_add_token_b, 100);
            assert_instruction_arg(&add, "minAmountLiquidity", min_amount_liquidity);
            assert_instruction_arg(&add, "maxAmountToAddTokenA", max_amount_to_add_token_a);
            assert_instruction_arg(&add, "maxAmountToAddTokenB", max_amount_to_add_token_b);
            assert_instruction_arg(&add, "deadline", deadline);
        }
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreatePriceObservations { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::NewDefinition { .. }
        | Instruction::RemoveLiquidity { .. }
        | Instruction::SwapExactInput { .. }
        | Instruction::SwapExactOutput { .. }
        | Instruction::SyncReserves => panic!("add wire operation emitted wrong instruction"),
    }

    let mut remove_request = fixture.active_common("prepare_remove_liquidity_transaction");
    insert(&mut remove_request, "removeLiquidityAmount", json!("500"));
    let remove = plan_json(remove_request).expect("remove transaction must prepare");
    assert_common_contract(&remove, "remove_liquidity", false);
    assert_eq!(remove["callerAmounts"]["first"], "125");
    assert_eq!(remove["callerAmounts"]["second"], "250");
    assert_eq!(
        remove["walletPrerequisites"]["funding"][0]["holdingAccountId"],
        fixture.liquidity_holding_id.to_string()
    );
    match decode_instruction(&remove) {
        Instruction::RemoveLiquidity {
            remove_liquidity_amount,
            min_amount_to_remove_token_a,
            min_amount_to_remove_token_b,
            deadline,
        } => {
            assert_instruction_arg(&remove, "removeLiquidityAmount", remove_liquidity_amount);
            assert_instruction_arg(
                &remove,
                "minAmountToRemoveTokenA",
                min_amount_to_remove_token_a,
            );
            assert_instruction_arg(
                &remove,
                "minAmountToRemoveTokenB",
                min_amount_to_remove_token_b,
            );
            assert_instruction_arg(&remove, "deadline", deadline);
        }
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreatePriceObservations { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::NewDefinition { .. }
        | Instruction::AddLiquidity { .. }
        | Instruction::SwapExactInput { .. }
        | Instruction::SwapExactOutput { .. }
        | Instruction::SyncReserves => panic!("remove wire operation emitted wrong instruction"),
    }

    let mut exact_input_request = fixture.swap_common("prepare_swap_exact_input_transaction");
    insert(&mut exact_input_request, "amountIn", json!("100"));
    let exact_input = plan_json(exact_input_request).expect("exact-input transaction must prepare");
    assert_common_contract(&exact_input, "swap_exact_input", true);
    assert_eq!(exact_input["callerAmounts"]["first"], "100");
    assert_eq!(
        exact_input["walletPrerequisites"]["funding"][0]["holdingAccountId"],
        fixture.first_holding_id.to_string()
    );
    match decode_instruction(&exact_input) {
        Instruction::SwapExactInput {
            swap_amount_in,
            min_amount_out,
            deadline,
        } => {
            assert_instruction_arg(&exact_input, "swapAmountIn", swap_amount_in);
            assert_instruction_arg(&exact_input, "minAmountOut", min_amount_out);
            assert_instruction_arg(&exact_input, "deadline", deadline);
        }
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreatePriceObservations { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::NewDefinition { .. }
        | Instruction::AddLiquidity { .. }
        | Instruction::RemoveLiquidity { .. }
        | Instruction::SwapExactOutput { .. }
        | Instruction::SyncReserves => {
            panic!("exact-input wire operation emitted wrong instruction")
        }
    }

    let mut exact_output_request = fixture.swap_common("prepare_swap_exact_output_transaction");
    insert(&mut exact_output_request, "exactAmountOut", json!("100"));
    let exact_output =
        plan_json(exact_output_request).expect("exact-output transaction must prepare");
    assert_common_contract(&exact_output, "swap_exact_output", true);
    assert_eq!(exact_output["callerAmounts"]["second"], "100");
    match decode_instruction(&exact_output) {
        Instruction::SwapExactOutput {
            exact_amount_out,
            max_amount_in,
            deadline,
        } => {
            assert_instruction_arg(&exact_output, "exactAmountOut", exact_amount_out);
            assert_instruction_arg(&exact_output, "maxAmountIn", max_amount_in);
            assert_instruction_arg(&exact_output, "deadline", deadline);
            assert_eq!(
                exact_output.pointer("/walletPrerequisites/funding/0/required"),
                Some(&json!(max_amount_in.to_string()))
            );
        }
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreatePriceObservations { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::NewDefinition { .. }
        | Instruction::AddLiquidity { .. }
        | Instruction::RemoveLiquidity { .. }
        | Instruction::SwapExactInput { .. }
        | Instruction::SyncReserves => {
            panic!("exact-output wire operation emitted wrong instruction")
        }
    }

    assert_eq!(
        fixture.pool_id.to_string(),
        add["plan"]["accounts"][1]["id"]
    );
    assert_ne!(fixture.first_vault_id, fixture.second_vault_id);
    assert_ne!(fixture.lp_lock_holding_id, fixture.current_tick_id);
    assert_eq!(
        fixture.liquidity_definition_id.to_string(),
        remove["walletPrerequisites"]["funding"][0]["tokenDefinitionId"]
    );
    let output_holding = exact_input["plan"]["accounts"]
        .as_array()
        .expect("plan accounts must be an array")
        .iter()
        .find(|account| account["role"] == "user_output_holding")
        .expect("swap plan must contain output holding");
    assert_eq!(fixture.second_holding_id.to_string(), output_holding["id"]);
}

#[test]
fn transaction_wire_rejects_expected_fee_mismatch() {
    let fixture = TransactionFixture::new();
    let mut request = fixture.active_common("prepare_add_liquidity_transaction");
    insert(&mut request, "maxFirstAmount", json!("100"));
    insert(&mut request, "maxSecondAmount", json!("400"));
    insert(&mut request, "expectedFeeBps", json!("100"));

    let error = plan_json(request).expect_err("wrong expected fee must fail");
    assert_eq!(error.code(), "fee_mismatch");
}
