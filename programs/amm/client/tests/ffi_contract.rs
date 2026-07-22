#![allow(
    unsafe_code,
    reason = "contract tests call the exported C ABI and release its owned pointers"
)]

mod common;

use std::ffi::{c_char, CStr, CString};

use amm_client::{amm_client_free, amm_client_plan, amm_client_quote, wire::WIRE_SCHEMA};
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_pool_pda, compute_vault_pda,
    AmmConfig, Instruction, PoolDefinition, FEE_TIER_BPS_30, MINIMUM_LIQUIDITY,
};
use common::program_id_hex;
use nssa_core::{
    account::{Account, AccountId, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::{TokenDefinition, TokenHolding};

type Operation = unsafe extern "C" fn(*const c_char) -> *mut c_char;

fn call(operation: Operation, request: Option<&CStr>) -> Value {
    let request = request.map_or(std::ptr::null(), CStr::as_ptr);
    // SAFETY: `request` is null or points into the borrowed `CStr`, which remains live through the
    // call. The returned pointer is checked and released exactly once below.
    let response = unsafe { operation(request) };
    assert!(!response.is_null());

    // SAFETY: A non-null response is a live NUL-terminated string owned by the AMM client until
    // `amm_client_free` below.
    let text = unsafe { CStr::from_ptr(response) }
        .to_str()
        .expect("FFI response must be UTF-8");
    let value = serde_json::from_str(text).expect("FFI response must be JSON");
    // SAFETY: `response` came from this library and has not been released yet.
    unsafe { amm_client_free(response) };
    value
}

fn call_json(operation: Operation, request: &Value) -> Value {
    let request = CString::new(request.to_string()).expect("JSON has no interior NUL");
    call(operation, Some(&request))
}

fn snapshot(id: AccountId, account: &Account) -> Value {
    json!({
        "id": id.to_string(),
        "programOwner": program_id_hex(account.program_owner),
        "balance": account.balance.to_string(),
        "nonce": account.nonce.0.to_string(),
        "data": hex(account.data.as_ref()),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn account(program_owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner,
        balance: 0,
        data,
        nonce: Nonce(0),
    }
}

fn fungible_definition(
    program_owner: ProgramId,
    total_supply: u128,
    authority: Option<AccountId>,
) -> Account {
    account(
        program_owner,
        Data::from(&TokenDefinition::Fungible {
            name: String::from("Token"),
            total_supply,
            metadata_id: None,
            authority,
        }),
    )
}

fn fungible_holding(program_owner: ProgramId, definition_id: AccountId, balance: u128) -> Account {
    account(
        program_owner,
        Data::from(&TokenHolding::Fungible {
            definition_id,
            balance,
        }),
    )
}

#[test]
fn null_request_returns_structured_error() {
    let response = call(amm_client_plan, None);

    assert_eq!(response["schema"], WIRE_SCHEMA);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "null_request");
}

#[test]
fn malformed_json_returns_structured_error() {
    let request = CString::new("{").expect("literal has no NUL");
    let response = call(amm_client_quote, Some(&request));

    assert_eq!(response["schema"], WIRE_SCHEMA);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "invalid_json");
}

#[test]
fn invalid_utf8_returns_structured_error() {
    let request = CStr::from_bytes_with_nul(&[0xff, 0]).expect("bytes are NUL-terminated");
    let response = call(amm_client_quote, Some(request));

    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "invalid_utf8");
}

#[test]
fn free_accepts_null() {
    // SAFETY: Null is explicitly accepted by the deallocator contract.
    unsafe { amm_client_free(std::ptr::null_mut()) };
}

#[test]
fn protocol_constants_are_exposed_without_numeric_json_values() {
    let response = call_json(
        amm_client_quote,
        &json!({"operation": "protocol_constants"}),
    );

    assert_eq!(response["schema"], WIRE_SCHEMA);
    assert_eq!(response["ok"], true);
    assert_eq!(response["value"]["schema"], WIRE_SCHEMA);
    assert_eq!(
        response["value"]["minimumLiquidity"],
        MINIMUM_LIQUIDITY.to_string()
    );
    assert_eq!(response["value"]["feeBpsDenominator"], "10000");
    assert_eq!(response["value"]["slippageBpsDenominator"], "10000");
    assert_eq!(
        response["value"]["supportedFeeTiers"],
        json!(["1", "5", "30", "100"])
    );
}

#[test]
fn program_ids_require_canonical_lowercase_hex_strings() {
    let canonical = program_id_hex([0xabcdef01; 8]);
    let authority = AccountId::new([44; 32]).to_string();

    for invalid in [
        json!([1, 1, 1, 1, 1, 1, 1, 1]),
        json!(canonical.to_uppercase()),
    ] {
        let response = call_json(
            amm_client_plan,
            &json!({
                "operation": "initialize",
                "ammProgramId": invalid,
                "tokenProgramId": canonical,
                "twapOracleProgramId": canonical,
                "authority": authority,
            }),
        );

        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "invalid_request");
    }
}

#[test]
fn successful_plan_preserves_u64_above_javascript_range_in_guest_words() {
    let amm_program_id: ProgramId = [11; 8];
    let token_program_id: ProgramId = [22; 8];
    let twap_oracle_program_id: ProgramId = [33; 8];
    let authority = AccountId::new([44; 32]);
    let pool_id = AccountId::new([55; 32]);
    let window_duration = 9_007_199_254_740_993_u64;
    let response = call_json(
        amm_client_plan,
        &json!({
            "operation": "create_price_observations",
            "context": {
                "ammProgramId": program_id_hex(amm_program_id),
                "tokenProgramId": program_id_hex(token_program_id),
                "twapOracleProgramId": program_id_hex(twap_oracle_program_id),
                "authority": authority.to_string(),
            },
            "poolId": pool_id.to_string(),
            "windowDuration": window_duration.to_string(),
        }),
    );

    assert_eq!(response["ok"], true);
    assert_eq!(
        response["value"]["instruction"],
        "create_price_observations"
    );
    assert_eq!(
        response["value"]["programId"],
        program_id_hex(amm_program_id)
    );
    assert!(response["value"]["accounts"].is_array());
    let words: Vec<u32> = serde_json::from_value(response["value"]["instructionWords"].clone())
        .expect("instruction words must be u32 JSON numbers");
    let instruction: Instruction =
        risc0_zkvm::serde::from_slice(&words).expect("guest codec must decode plan words");
    match instruction {
        Instruction::CreatePriceObservations {
            window_duration: decoded,
        } => assert_eq!(decoded, window_duration),
        Instruction::Initialize { .. }
        | Instruction::UpdateConfig { .. }
        | Instruction::CreateOraclePriceAccount { .. }
        | Instruction::NewDefinition { .. }
        | Instruction::AddLiquidity { .. }
        | Instruction::RemoveLiquidity { .. }
        | Instruction::SwapExactInput { .. }
        | Instruction::SwapExactOutput { .. }
        | Instruction::SyncReserves => panic!("expected CreatePriceObservations"),
    }
}

#[test]
fn successful_quote_preserves_u128_above_javascript_range_as_decimal() {
    let amm_program_id: ProgramId = [11; 8];
    let token_program_id: ProgramId = [22; 8];
    let twap_oracle_program_id: ProgramId = [33; 8];
    let authority = AccountId::new([44; 32]);
    let config = AmmConfig {
        token_program_id,
        twap_oracle_program_id,
        authority,
    };
    let config_account = Account {
        program_owner: amm_program_id,
        balance: 0,
        data: Data::from(&config),
        nonce: Nonce(0),
    };
    let definition = |name: &str| Account {
        program_owner: token_program_id,
        balance: 0,
        data: Data::from(&TokenDefinition::Fungible {
            name: String::from(name),
            total_supply: 0,
            metadata_id: None,
            authority: None,
        }),
        nonce: Nonce(0),
    };
    let token_a_id = AccountId::new([61; 32]);
    let token_b_id = AccountId::new([62; 32]);
    let amount = 9_007_199_254_740_993_u128;
    let response = call_json(
        amm_client_quote,
        &json!({
            "operation": "create_pool",
            "ammProgramId": program_id_hex(amm_program_id),
            "config": snapshot(compute_config_pda(amm_program_id), &config_account),
            "tokenADefinition": snapshot(token_a_id, &definition("A")),
            "tokenBDefinition": snapshot(token_b_id, &definition("B")),
            "tokenAAmount": amount.to_string(),
            "tokenBAmount": amount.to_string(),
            "feeBps": "30",
        }),
    );

    assert_eq!(response["ok"], true);
    assert_eq!(response["value"]["pool"]["reserveA"], amount.to_string());
    assert_eq!(response["value"]["pool"]["reserveB"], amount.to_string());
    assert_eq!(
        response["value"]["userLiquidity"],
        amount
            .checked_sub(MINIMUM_LIQUIDITY)
            .expect("test amount exceeds liquidity lock")
            .to_string()
    );
    assert!(response["value"]["pool"]["reserveA"].is_string());

    let prepared = call_json(
        amm_client_quote,
        &json!({
            "operation": "prepare_create_pool",
            "ammProgramId": program_id_hex(amm_program_id),
            "config": snapshot(compute_config_pda(amm_program_id), &config_account),
            "tokenADefinition": snapshot(token_a_id, &definition("A")),
            "tokenBDefinition": snapshot(token_b_id, &definition("B")),
            "tokenAAmount": amount.to_string(),
            "tokenBAmount": amount.to_string(),
            "feeBps": "30",
        }),
    );
    assert_eq!(prepared["ok"], true);
    assert_eq!(
        prepared["value"]["instructionArgs"]["tokenAAmount"],
        amount.to_string()
    );
    assert_eq!(
        prepared["value"]["instructionArgs"]["tokenBAmount"],
        amount.to_string()
    );
    assert!(prepared["value"]["instructionArgs"]["tokenAAmount"].is_string());
}

#[test]
fn swap_quote_rejects_unrelated_output_holding() {
    let amm_program_id: ProgramId = [11; 8];
    let token_program_id: ProgramId = [22; 8];
    let twap_oracle_program_id: ProgramId = [33; 8];
    let token_a_id = AccountId::new([1; 32]);
    let token_b_id = AccountId::new([2; 32]);
    let unrelated_token_id = AccountId::new([3; 32]);
    let pool_id = compute_pool_pda(amm_program_id, token_a_id, token_b_id);
    let vault_a_id = compute_vault_pda(amm_program_id, pool_id, token_a_id);
    let vault_b_id = compute_vault_pda(amm_program_id, pool_id, token_b_id);
    let liquidity_id = compute_liquidity_token_pda(amm_program_id, pool_id);
    let config = AmmConfig {
        token_program_id,
        twap_oracle_program_id,
        authority: AccountId::new([9; 32]),
    };
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
    let response = call_json(
        amm_client_quote,
        &json!({
            "operation": "preview_swap_exact_input",
            "ammProgramId": program_id_hex(amm_program_id),
            "config": snapshot(
                compute_config_pda(amm_program_id),
                &account(amm_program_id, Data::from(&config)),
            ),
            "snapshot": {
                "pool": snapshot(
                    pool_id,
                    &account(amm_program_id, Data::from(&pool)),
                ),
                "tokenADefinition": snapshot(
                    token_a_id,
                    &fungible_definition(token_program_id, 100_000, None),
                ),
                "tokenBDefinition": snapshot(
                    token_b_id,
                    &fungible_definition(token_program_id, 100_000, None),
                ),
                "vaultA": snapshot(
                    vault_a_id,
                    &fungible_holding(token_program_id, token_a_id, 1_100),
                ),
                "vaultB": snapshot(
                    vault_b_id,
                    &fungible_holding(token_program_id, token_b_id, 550),
                ),
                "liquidityDefinition": snapshot(
                    liquidity_id,
                    &fungible_definition(token_program_id, 2_000, Some(liquidity_id)),
                ),
            },
            "userInputHolding": snapshot(
                AccountId::new([20; 32]),
                &fungible_holding(token_program_id, token_a_id, 1_000),
            ),
            "userOutputHolding": snapshot(
                AccountId::new([21; 32]),
                &fungible_holding(token_program_id, unrelated_token_id, 0),
            ),
            "inputTokenDefinitionId": token_a_id.to_string(),
            "amountIn": "100",
        }),
    );

    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "token_definition_mismatch");
}
