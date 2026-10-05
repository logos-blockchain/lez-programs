use std::ffi::{c_char, CStr, CString};

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, math::FIXED_POINT_ONE, Instruction, Position,
    ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use super::*;
use crate::account::{account_id_hex, account_read, program_id_bytes};

const PROGRAM: ProgramId = [0x11; 8];
const TOKEN_PROGRAM: ProgramId = [0x22; 8];
fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn parameters() -> ProtocolParameters {
    ProtocolParameters {
        admin_account_id: id(1),
        freeze_authority_account_id: id(2),
        stablecoin_definition_id: compute_stablecoin_definition_pda(PROGRAM),
        collateral_definition_id: id(4),
        market_price_oracle_id: id(5),
        stability_fee_per_millisecond: FIXED_POINT_ONE,
        controller_proportional_gain: 0,
        controller_integral_gain: 0,
        minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
        minimum_milliseconds_between_rate_updates: 50,
        maximum_oracle_price_age_milliseconds: 900_000,
        is_frozen: false,
    }
}

fn parameters_account(parameters: &ProtocolParameters) -> Account {
    Account {
        program_owner: PROGRAM,
        balance: 23,
        nonce: Nonce(9),
        data: Data::from(parameters),
    }
}

fn request(account: &Account, authority: AccountId) -> FreezeAuthorityPlanRequest {
    FreezeAuthorityPlanRequest {
        stablecoin_program_id: hex::encode(program_id_bytes(PROGRAM)),
        freeze_authority_id: account_id_hex(authority),
        protocol_parameters: account_read(compute_protocol_parameters_pda(PROGRAM), account),
    }
}

fn planner(frozen: bool) -> fn(FreezeAuthorityPlanRequest) -> StablecoinResult {
    if frozen {
        freeze_plan
    } else {
        unfreeze_plan
    }
}

fn execute(account: Account, authority: AccountId, frozen: bool) -> Account {
    let request = request(&account, authority);
    let plan = planner(frozen)(request.clone()).expect("authorized operation");
    assert_eq!(plan["programId"], request.stablecoin_program_id);
    assert_eq!(
        plan["accountIds"],
        json!([account_id_hex(authority), request.protocol_parameters.id])
    );
    assert_eq!(plan["signingRequirements"], json!([true, false]));
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    assert_eq!(words.len(), 1);
    let authority_account = Account {
        balance: 11,
        nonce: Nonce(3),
        ..Account::default()
    };
    let inputs = (
        AccountWithMetadata::new(authority_account.clone(), true, authority),
        AccountWithMetadata::new(
            account.clone(),
            false,
            compute_protocol_parameters_pda(PROGRAM),
        ),
    );
    let (posts, calls) =
        match risc0_zkvm::serde::from_slice::<Instruction, u32>(&words).expect("instruction") {
            Instruction::Freeze => {
                assert!(frozen);
                stablecoin_program::freeze::freeze(inputs.0, inputs.1, PROGRAM)
            }
            Instruction::Unfreeze => {
                assert!(!frozen);
                stablecoin_program::freeze::unfreeze(inputs.0, inputs.1, PROGRAM)
            }
            instruction => panic!("unexpected operation {instruction:?}"),
        };
    assert!(calls.is_empty());
    assert_eq!(posts.len(), 2);
    assert_eq!(
        posts.first().expect("authority post").account(),
        &authority_account
    );
    let post = posts.get(1).expect("parameters post").account();
    let mut expected = ProtocolParameters::try_from(&account.data).expect("parameters");
    expected.is_frozen = frozen;
    assert_eq!(
        post,
        &Account {
            data: Data::from(&expected),
            ..account
        }
    );
    post.clone()
}

#[test]
fn freeze_repeat_unfreeze_repeat_execute_native_and_change_only_the_flag() {
    let mut account = parameters_account(&parameters());
    for frozen in [true, true, false, false] {
        account = execute(account, id(2), frozen);
    }
    assert_eq!(account, parameters_account(&parameters()));
}

#[test]
fn both_unit_instructions_pin_current_guest_flags_pda_program_and_serialization() {
    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    let account = parameters_account(&parameters());
    for (frozen, name, instruction) in [
        (true, "freeze", Instruction::Freeze),
        (false, "unfreeze", Instruction::Unfreeze),
    ] {
        let plan = planner(frozen)(request(&account, id(2))).expect("plan");
        assert_eq!(
            plan["instruction"],
            json!(risc0_zkvm::serde::to_vec(&instruction).expect("encode"))
        );
        let entry = idl["instructions"]
            .as_array()
            .expect("instructions")
            .iter()
            .find(|entry| entry["name"] == name)
            .expect("entry");
        assert_eq!(entry["args"], json!([]));
        assert_eq!(
            entry["accounts"],
            json!([
                {"name":"freeze_authority", "writable":false, "signer":true, "init":false},
                {"name":"protocol_parameters", "writable":true, "signer":false, "init":false},
            ])
        );
    }
}

#[test]
fn admin_alone_is_rejected_and_rotated_or_shared_authority_is_read_from_current_state() {
    let mut account = parameters_account(&parameters());
    for frozen in [false, true] {
        assert_eq!(
            planner(frozen)(request(&account, id(1)))
                .expect_err("distinct admin cannot freeze")
                .code(),
            "freeze_authority_mismatch"
        );
    }
    let admin = AccountWithMetadata::new(Account::default(), true, id(1));
    let (posts, calls) = stablecoin_program::admin::set_freeze_authority(
        admin.clone(),
        AccountWithMetadata::new(account, false, compute_protocol_parameters_pda(PROGRAM)),
        PROGRAM,
        id(3),
    );
    assert!(calls.is_empty());
    account = posts.get(1).expect("parameters post").account().clone();
    for frozen in [true, false] {
        assert_eq!(
            planner(frozen)(request(&account, id(2)))
                .expect_err("old authority loses role")
                .code(),
            "freeze_authority_mismatch"
        );
        account = execute(account, id(3), frozen);
    }
    let (posts, _) = stablecoin_program::admin::set_freeze_authority(
        admin,
        AccountWithMetadata::new(account, false, compute_protocol_parameters_pda(PROGRAM)),
        PROGRAM,
        id(1),
    );
    account = execute(
        posts.get(1).expect("parameters post").account().clone(),
        id(1),
        true,
    );
    execute(account, id(1), false);
}

#[test]
fn repeats_still_reject_wrong_pda_owner_missing_and_inexact_data_and_malformed_ids() {
    for frozen in [false, true] {
        let mut parameters = parameters();
        parameters.is_frozen = frozen;
        let valid = request(&parameters_account(&parameters), id(2));
        // Deliberately corrupt RPC observations to exercise initialized-account
        // invariants, not to claim these are valid native state mutations.
        for (mutation, expected) in [
            (0, "protocol_parameters_pda_mismatch"),
            (1, "stablecoin_program_mismatch"),
            (2, "invalid_protocol_parameters_data"),
            (3, "invalid_protocol_parameters_data"),
            (4, "invalid_protocol_parameters_data"),
            (5, "account_read_failed"),
            (6, "account_read_failed"),
        ] {
            let mut request = valid.clone();
            let read = &mut request.protocol_parameters;
            match mutation {
                0 => read.id = account_id_hex(id(99)),
                1 => {
                    read.account.as_mut().expect("account").program_owner =
                        hex::encode(program_id_bytes(TOKEN_PROGRAM))
                }
                2 => read.account.as_mut().expect("account").data.clear(),
                3 => {
                    let data = &mut read.account.as_mut().expect("account").data;
                    data.truncate(data.len() - 2);
                }
                4 => read.account.as_mut().expect("account").data.push_str("00"),
                5 => read.status = String::from("not_found"),
                _ => read.account = None,
            }
            for operation in [freeze_plan, unfreeze_plan] {
                assert_eq!(
                    operation(request.clone()).expect_err("must reject").code(),
                    expected
                );
            }
        }
        for encoding in [
            account_id_hex(id(2)).to_ascii_uppercase(),
            id(2).to_string(),
        ] {
            let mut request = valid.clone();
            request.freeze_authority_id = format!(" {encoding} ");
            assert!(planner(frozen)(request).is_ok());
        }
        for invalid in [
            "",
            "invalid",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            let mut request = valid.clone();
            request.freeze_authority_id = String::from(invalid);
            assert_eq!(
                planner(frozen)(request).expect_err("malformed id").code(),
                "invalid_account_id"
            );
        }
        let mut request = valid;
        request.stablecoin_program_id = String::from("invalid");
        assert_eq!(
            planner(frozen)(request)
                .expect_err("invalid program")
                .code(),
            "invalid_program_id"
        );
    }
}

fn read_value(account_id: AccountId, program: ProgramId, data: Data) -> Value {
    json!({"id": account_id_hex(account_id), "status": "ok", "account": {
        "program_owner": hex::encode(program_id_bytes(program)), "balance": "00000000000000000000000000000000",
        "nonce": "00000000000000000000000000000000", "data": hex::encode(data.as_ref()),
    }})
}
fn typed<T: DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).expect("typed request")
}

fn related_plans(parameters: &ProtocolParameters) -> Vec<(&'static str, bool, StablecoinResult)> {
    let definition_id = parameters.stablecoin_definition_id;
    let position_id = compute_position_pda(PROGRAM, id(20), 7);
    let vault_id = compute_position_vault_pda(PROGRAM, position_id);
    let holding = |account_id, definition_id| {
        read_value(
            account_id,
            TOKEN_PROGRAM,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance: 0,
            }),
        )
    };
    let common = json!({
        "stablecoinProgramId": hex::encode(program_id_bytes(PROGRAM)), "ownerId": account_id_hex(id(20)),
        "callerId": account_id_hex(id(20)), "positionNonce": "7", "amount": "0", "initialCollateralAmount": "0",
        "protocolParameters": read_value(compute_protocol_parameters_pda(PROGRAM), PROGRAM, Data::from(parameters)),
        "position": read_value(position_id, PROGRAM, Data::from(&Position { owner_account_id: id(20), position_nonce: 7,
            vault_account_id: vault_id, collateral_amount: 0, normalized_debt_amount: 0, opened_at: 1_000 })),
        "vault": holding(vault_id, id(4)),
        "userCollateralHoldingId": account_id_hex(id(22)), "userCollateralHolding": holding(id(22), id(4)),
        "userStablecoinHoldingId": account_id_hex(id(21)), "userStablecoinHolding": holding(id(21), definition_id),
        "collateralDefinition": read_value(id(4), TOKEN_PROGRAM, Data::from(&TokenDefinition::Fungible {
            name: String::from("Collateral"), total_supply: 0, metadata_id: None, authority: None })),
        "stablecoinDefinition": read_value(definition_id, TOKEN_PROGRAM, Data::from(&TokenDefinition::Fungible {
            name: String::from("Stablecoin"), total_supply: 0, metadata_id: None, authority: Some(definition_id) })),
        "stabilityFeeAccumulator": read_value(compute_stability_fee_accumulator_pda(PROGRAM), PROGRAM,
            Data::from(&StabilityFeeAccumulator { accumulated_rate_at_last_accrual: FIXED_POINT_ONE, last_accrued_at: 1_000 })),
        "redemptionPriceState": read_value(compute_redemption_price_state_pda(PROGRAM), PROGRAM,
            Data::from(&RedemptionPriceState { redemption_price_at_last_update: FIXED_POINT_ONE,
                redemption_rate_per_millisecond: FIXED_POINT_ONE, controller_integral_term: 0, last_updated_at: 1_000 })),
        "marketPriceOracle": read_value(id(5), [0x33; 8], Data::from(&OraclePriceAccount {
            base_asset: definition_id, quote_asset: id(4), price: FIXED_POINT_ONE, timestamp: 1_500,
            source_id: id(6), confidence_interval: 0 })),
        "clock": read_value(CLOCK_01_PROGRAM_ACCOUNT_ID, [0x44; 8], Data::try_from(
            ClockAccountData { block_id: 1, timestamp: 1_500 }.to_bytes()).expect("clock fits")),
    });
    // Open uses a fresh nonce distinct from the settled Position used by the
    // other planners, so its target PDAs are not already initialized.
    let mut open = common.clone();
    open["positionNonce"] = json!("8");
    vec![
        ("open", true, open_position_plan(typed(&open))),
        ("withdraw", true, withdraw_collateral_plan(typed(&common))),
        ("generate", true, generate_debt_plan(typed(&common))),
        ("deposit", false, deposit_collateral_plan(typed(&common))),
        ("repay", false, repay_debt_plan(typed(&common))),
        ("close", false, close_position_plan(typed(&common))),
        ("accrue", false, accrue_stability_fee_plan(typed(&common))),
        ("update", false, update_redemption_rate_plan(typed(&common))),
        ("refresh", false, refresh_globals_plan(typed(&common))),
    ]
}

#[test]
fn native_freeze_gates_risk_increasing_plans_keeps_recovery_and_pokes_and_unfreeze_restores_them() {
    let mut account = parameters_account(&parameters());
    for frozen in [false, true, false] {
        if frozen
            || ProtocolParameters::try_from(&account.data)
                .expect("parameters")
                .is_frozen
        {
            account = execute(account, id(2), frozen);
        }
        let parameters = ProtocolParameters::try_from(&account.data).expect("parameters");
        for (name, increases_risk, result) in related_plans(&parameters) {
            if frozen && increases_risk {
                assert_eq!(result.expect_err(name).code(), "protocol_frozen", "{name}");
            } else {
                assert!(result.is_ok(), "{name}: {result:?}");
            }
        }
    }
}

#[test]
fn c_boundaries_reject_bad_requests_and_never_use_a_caller_boolean_to_select_the_instruction() {
    let account = parameters_account(&parameters());
    let parameters_read = request(&account, id(2)).protocol_parameters;
    let source = parameters_read.account.as_ref().expect("account");
    for (operation, frozen) in [
        (
            crate::ffi::stablecoin_freeze_plan
                as unsafe extern "C" fn(*const c_char) -> *mut c_char,
            true,
        ),
        (crate::ffi::stablecoin_unfreeze_plan, false),
    ] {
        let payload = json!({"stablecoinProgramId": hex::encode(program_id_bytes(PROGRAM)),
        "freezeAuthorityId": id(2).to_string(), "isFrozen": !frozen,
        "protocolParametersId": account_id_hex(id(99)), "protocolParameters": {
            "id": parameters_read.id, "status": "ok", "account": {"program_owner": source.program_owner,
                "balance": source.balance, "nonce": source.nonce, "data": source.data},
        }});
        let text = CString::new(payload.to_string()).expect("JSON no NUL");
        // SAFETY: text is live and NUL-terminated for this operation.
        let response = unsafe { operation(text.as_ptr()) };
        assert!(!response.is_null());
        // SAFETY: response is the live library allocation returned above.
        let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(response) }.to_bytes())
            .expect("response");
        assert_eq!(value["ok"], true);
        assert_eq!(
            value["value"],
            planner(frozen)(request(&account, id(2))).expect("pure plan")
        );
        // SAFETY: response has not been freed and belongs to this library.
        unsafe { crate::ffi::stablecoin_free(response) };
        for input in ["{}", "{", r#"{"freezeAuthorityId":1}"#] {
            let text = CString::new(input).expect("no NUL");
            // SAFETY: text is live and NUL-terminated for this call.
            let pointer = unsafe { operation(text.as_ptr()) };
            assert!(!pointer.is_null());
            // SAFETY: pointer is the unfreed library allocation returned above.
            let value: Value =
                serde_json::from_slice(unsafe { CStr::from_ptr(pointer) }.to_bytes())
                    .expect("response");
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"], "bad_request");
            // SAFETY: pointer is unfreed and belongs to this library.
            unsafe { crate::ffi::stablecoin_free(pointer) };
        }
        // SAFETY: null is explicitly accepted and reported as bad_request.
        let pointer = unsafe { operation(std::ptr::null()) };
        assert!(!pointer.is_null());
        // SAFETY: pointer is the unfreed library allocation returned above.
        let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(pointer) }.to_bytes())
            .expect("response");
        assert_eq!(value["error"], "bad_request");
        // SAFETY: pointer is unfreed and belongs to this library.
        unsafe { crate::ffi::stablecoin_free(pointer) };
    }
}
