use std::ffi::{CStr, CString};

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_stability_fee_accumulator_pda,
    math::{
        compute_current_accumulated_rate, FIXED_POINT_ONE, MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
    },
    Instruction, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};

use super::*;
use crate::account::{account_id_hex, program_id_bytes};

const PROGRAM: ProgramId = [0x11; 8];
const START: u64 = 1_000;

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn account(owner: ProgramId, data: Data) -> Account {
    Account {
        program_owner: owner,
        balance: 17,
        nonce: Nonce(7),
        data,
    }
}

fn read_value(account_id: AccountId, source: &Account) -> Value {
    json!({"id": account_id_hex(account_id), "status": "ok", "account": {
        "program_owner": hex::encode(program_id_bytes(source.program_owner)),
        "balance": hex::encode(source.balance.to_le_bytes()),
        "nonce": hex::encode(source.nonce.0.to_le_bytes()), "data": hex::encode(source.data.as_ref()),
    }})
}

#[derive(Clone)]
struct Fixture {
    parameters: ProtocolParameters,
    accumulator: StabilityFeeAccumulator,
    redemption: RedemptionPriceState,
    now: u64,
}

impl Fixture {
    fn new() -> Self {
        Self {
            parameters: ProtocolParameters {
                admin_account_id: id(1),
                freeze_authority_account_id: id(2),
                stablecoin_definition_id: id(3),
                collateral_definition_id: id(4),
                market_price_oracle_id: id(5),
                stability_fee_per_millisecond: FIXED_POINT_ONE + 1_500_000_000_000_000,
                controller_proportional_gain: -17,
                controller_integral_gain: 19,
                minimum_collateralization_ratio: FIXED_POINT_ONE * 150 / 100,
                minimum_milliseconds_between_rate_updates: 100,
                maximum_oracle_price_age_milliseconds: 200,
                is_frozen: false,
            },
            accumulator: StabilityFeeAccumulator {
                accumulated_rate_at_last_accrual: FIXED_POINT_ONE + 9_007_199_254_740_993,
                last_accrued_at: START,
            },
            redemption: RedemptionPriceState {
                redemption_price_at_last_update: FIXED_POINT_ONE + 23,
                redemption_rate_per_millisecond: FIXED_POINT_ONE + 31,
                controller_integral_term: -29,
                last_updated_at: 997,
            },
            now: START + 10,
        }
    }

    fn accounts(&self) -> [Account; 4] {
        [
            account([0; 8], Data::default()),
            account(PROGRAM, Data::from(&self.parameters)),
            account(PROGRAM, Data::from(&self.accumulator)),
            account(
                [0; 8],
                Data::try_from(
                    ClockAccountData {
                        block_id: 1,
                        timestamp: self.now,
                    }
                    .to_bytes(),
                )
                .expect("clock bytes"),
            ),
        ]
    }

    fn request(&self, new_rate: u128) -> Value {
        let accounts = self.accounts();
        json!({
            "stablecoinProgramId": hex::encode(program_id_bytes(PROGRAM)),
            "adminId": account_id_hex(self.parameters.admin_account_id),
            "newRate": new_rate.to_string(),
            "protocolParameters": read_value(compute_protocol_parameters_pda(PROGRAM), &accounts[1]),
            "stabilityFeeAccumulator": read_value(compute_stability_fee_accumulator_pda(PROGRAM), &accounts[2]),
            "clock": read_value(CLOCK_01_PROGRAM_ACCOUNT_ID, &accounts[3]),
        })
    }

    fn execute(&mut self, response: &Value) {
        let ids: Vec<String> = serde_json::from_value(response["accountIds"].clone()).expect("IDs");
        let signs: Vec<bool> =
            serde_json::from_value(response["signingRequirements"].clone()).expect("signers");
        let before = self.accounts();
        let metadata: Vec<AccountWithMetadata> = before
            .iter()
            .enumerate()
            .map(|(index, source)| {
                AccountWithMetadata::new(
                    source.clone(),
                    signs[index],
                    plan::parse_account_id(&ids[index]).expect("ID"),
                )
            })
            .collect();
        let words: Vec<u32> =
            serde_json::from_value(response["instruction"].clone()).expect("instruction words");
        let instruction: Instruction = risc0_zkvm::serde::from_slice(&words).expect("instruction");
        let (posts, calls) = match instruction {
            Instruction::SetStabilityFeePerMillisecond { new_rate } => {
                stablecoin_program::admin::set_stability_fee_per_millisecond(
                    metadata[0].clone(),
                    metadata[1].clone(),
                    metadata[2].clone(),
                    metadata[3].clone(),
                    PROGRAM,
                    new_rate,
                )
            }
            Instruction::AccrueStabilityFee => {
                stablecoin_program::accrue_stability_fee::accrue_stability_fee(
                    metadata[0].clone(),
                    metadata[1].clone(),
                    metadata[2].clone(),
                    metadata[3].clone(),
                    PROGRAM,
                )
            }
            other => panic!("unexpected instruction: {other:?}"),
        };
        assert!(calls.is_empty());
        assert_eq!(posts.len(), 4);
        assert_eq!(posts[0].account(), &before[0]);
        assert_eq!(posts[3].account(), &before[3]);
        for index in [1, 2] {
            assert_eq!(posts[index].account().balance, before[index].balance);
            assert_eq!(posts[index].account().nonce, before[index].nonce);
            assert_eq!(
                posts[index].account().program_owner,
                before[index].program_owner
            );
        }
        self.parameters =
            ProtocolParameters::try_from(&posts[1].account().data).expect("parameters post");
        self.accumulator =
            StabilityFeeAccumulator::try_from(&posts[2].account().data).expect("accumulator post");
    }
}

fn plan(request: Value) -> StablecoinResult {
    set_stability_fee_per_millisecond_plan(serde_json::from_value(request).expect("request"))
}

fn failure(request: Value, expected: &str) {
    assert_eq!(
        plan(request).expect_err("preflight must reject").code(),
        expected
    );
}

#[test]
fn fee_setter_pins_configured_program_ids_serialization_and_guest_account_contract() {
    let fixture = Fixture::new();
    let request = fixture.request(FIXED_POINT_ONE + 9_007_199_254_740_993);
    let result = plan(request.clone()).expect("plan");
    assert_eq!(result["programId"], request["stablecoinProgramId"]);
    assert_eq!(
        result["accountIds"],
        json!([
            request["adminId"],
            request["protocolParameters"]["id"],
            request["stabilityFeeAccumulator"]["id"],
            account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        ])
    );
    assert_eq!(
        result["signingRequirements"],
        json!([true, false, false, false])
    );
    assert_eq!(
        result["instruction"],
        json!(
            risc0_zkvm::serde::to_vec(&Instruction::SetStabilityFeePerMillisecond {
                new_rate: FIXED_POINT_ONE + 9_007_199_254_740_993
            })
            .expect("encode")
        )
    );
    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    let entry = idl["instructions"]
        .as_array()
        .expect("instructions")
        .iter()
        .find(|entry| entry["name"] == "set_stability_fee_per_millisecond")
        .expect("fee setter IDL");
    assert_eq!(
        entry["accounts"],
        json!([
            {"name":"admin", "writable":false, "signer":true, "init":false},
            {"name":"protocol_parameters", "writable":true, "signer":false, "init":false},
            {"name":"stability_fee_accumulator", "writable":true, "signer":false, "init":false},
            {"name":"clock", "writable":false, "signer":false, "init":false},
        ])
    );
    assert_eq!(entry["args"], json!([{"name":"new_rate", "type":"u128"}]));
    let mut base58 = request.clone();
    base58["adminId"] = json!(fixture.parameters.admin_account_id.to_string());
    assert_eq!(plan(base58).expect("base58 admin"), result);
    for program_id in [[0x11; 8], [0x42; 8]] {
        let mut configured = request.clone();
        let program_hex = hex::encode(program_id_bytes(program_id));
        configured["stablecoinProgramId"] = json!(program_hex);
        for (field, derived) in [
            (
                "protocolParameters",
                compute_protocol_parameters_pda(program_id),
            ),
            (
                "stabilityFeeAccumulator",
                compute_stability_fee_accumulator_pda(program_id),
            ),
        ] {
            configured[field]["id"] = json!(account_id_hex(derived));
            configured[field]["account"]["program_owner"] = json!(program_hex);
        }
        assert_eq!(
            plan(configured).expect("configured deployment")["programId"],
            program_hex
        );
    }
}

#[test]
fn fee_setter_uses_lossless_u128_parser_and_inclusive_protocol_band() {
    let fixture = Fixture::new();
    for rate in [FIXED_POINT_ONE, FIXED_POINT_ONE + 1, FIXED_POINT_ONE * 2] {
        let request = fixture.request(rate);
        let response = plan(request).expect("inclusive valid rate");
        fixture.clone().execute(&response);
    }
    for rate in [0, FIXED_POINT_ONE - 1, FIXED_POINT_ONE * 2 + 1, u128::MAX] {
        failure(fixture.request(rate), "stability_fee_out_of_band");
    }
    for value in [json!(0), json!(9_007_199_254_740_993_u64), json!(u64::MAX)] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request["newRate"] = value;
        failure(request, "stability_fee_out_of_band");
    }
    for value in [
        json!(-1),
        json!(1.0),
        json!(1e27),
        json!(null),
        json!(true),
        json!(""),
        json!("+1000000000000000000000000000"),
        json!("1e27"),
        json!("1.0"),
        json!("-1"),
        json!("340282366920938463463374607431768211456"),
    ] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request["newRate"] = value;
        failure(request, "invalid_numeric_value");
    }
}

#[test]
fn fee_setter_native_journey_accrues_old_rate_then_new_rate_and_preserves_other_state() {
    let mut fixture = Fixture::new();
    fixture.parameters.is_frozen = true;
    let before = fixture.clone();
    let new_rate = FIXED_POINT_ONE * 2;
    let expected = compute_current_accumulated_rate(
        before.accumulator.accumulated_rate_at_last_accrual,
        before.parameters.stability_fee_per_millisecond,
        before.accumulator.last_accrued_at,
        before.now,
    );
    let response = plan(fixture.request(new_rate)).expect("frozen admin plan");
    fixture.execute(&response);
    assert_eq!(
        fixture.accumulator.accumulated_rate_at_last_accrual,
        expected
    );
    assert_eq!(fixture.accumulator.last_accrued_at, before.now);
    let mut expected_parameters = before.parameters.clone();
    expected_parameters.stability_fee_per_millisecond = new_rate;
    assert_eq!(fixture.parameters, expected_parameters);
    assert_eq!(fixture.redemption, before.redemption);

    fixture.now += 2;
    let request = fixture.request(new_rate);
    let accrue = accrue_stability_fee_plan(serde_json::from_value(json!({
        "stablecoinProgramId": request["stablecoinProgramId"], "callerId": request["adminId"],
        "protocolParameters": request["protocolParameters"],
        "stabilityFeeAccumulator": request["stabilityFeeAccumulator"], "clock": request["clock"],
    })).expect("accrual request")).expect("subsequent accrual");
    fixture.execute(&accrue);
    assert_eq!(
        fixture.accumulator.accumulated_rate_at_last_accrual,
        expected * 4
    );
    assert_eq!(fixture.accumulator.last_accrued_at, before.now + 2);
    assert_eq!(fixture.parameters, expected_parameters);
    assert_eq!(fixture.redemption, before.redemption);
}

#[test]
fn fee_setter_same_rate_same_time_inverted_time_and_clamp_match_native_accrual() {
    for now in [
        START - 1,
        START,
        START + 10,
        START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
        START + MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS + 1,
        u64::MAX,
    ] {
        let mut fixture = Fixture::new();
        fixture.now = now;
        let rate = fixture.parameters.stability_fee_per_millisecond;
        let expected = compute_current_accumulated_rate(
            fixture.accumulator.accumulated_rate_at_last_accrual,
            rate,
            START,
            now,
        );
        let response = plan(fixture.request(rate)).expect("unchanged rate still returns a plan");
        fixture.execute(&response);
        assert_eq!(
            fixture.accumulator.accumulated_rate_at_last_accrual,
            expected
        );
        assert_eq!(fixture.accumulator.last_accrued_at, now);
        assert_eq!(fixture.parameters.stability_fee_per_millisecond, rate);
    }
}

#[test]
fn fee_setter_rejects_unrepresentable_old_rate_accrual_even_when_new_rate_is_safe() {
    // These states are reachable by setting the allowed upper endpoint, then
    // waiting. A lower replacement rate cannot retroactively avoid the overflow.
    let mut fixture = Fixture::new();
    fixture.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE * 2;
    fixture.now = START + 100;
    failure(
        fixture.request(FIXED_POINT_ONE),
        "stability_fee_arithmetic_error",
    );
    // A representable factor can also overflow when multiplied by the anchor.
    fixture.now = START + 1;
    fixture.accumulator.accumulated_rate_at_last_accrual = u128::MAX / 2 + 1;
    failure(
        fixture.request(FIXED_POINT_ONE),
        "stability_fee_arithmetic_error",
    );
    fixture.now = START;
    assert!(plan(fixture.request(FIXED_POINT_ONE)).is_ok());
}

#[test]
fn fee_setter_rechecks_current_admin_after_native_rotation() {
    let mut fixture = Fixture::new();
    let old_admin = fixture.parameters.admin_account_id;
    let accounts = fixture.accounts();
    let (posts, calls) = stablecoin_program::admin::set_admin(
        AccountWithMetadata::new(accounts[0].clone(), true, old_admin),
        AccountWithMetadata::new(
            accounts[1].clone(),
            false,
            compute_protocol_parameters_pda(PROGRAM),
        ),
        PROGRAM,
        id(6),
    );
    assert!(calls.is_empty());
    fixture.parameters =
        ProtocolParameters::try_from(&posts[1].account().data).expect("rotated parameters");
    let mut request = fixture.request(FIXED_POINT_ONE);
    request["adminId"] = json!(account_id_hex(old_admin));
    failure(request, "admin_mismatch");
    let response = plan(fixture.request(FIXED_POINT_ONE)).expect("current admin");
    fixture.execute(&response);
    assert_eq!(fixture.parameters.admin_account_id, id(6));
}

#[test]
fn fee_setter_validates_singletons_exact_data_clock_and_signer_before_planning() {
    let fixture = Fixture::new();
    for (field, error) in [
        ("protocolParameters", "protocol_parameters_pda_mismatch"),
        (
            "stabilityFeeAccumulator",
            "stability_fee_accumulator_pda_mismatch",
        ),
        ("clock", "invalid_clock"),
    ] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request[field]["id"] = json!(account_id_hex(id(99)));
        failure(request, error);
        for status in ["not_found", "private", "error"] {
            let mut request = fixture.request(FIXED_POINT_ONE);
            request[field]["status"] = json!(status);
            failure(request, "account_read_failed");
        }
        let mut request = fixture.request(FIXED_POINT_ONE);
        request[field]["account"] = Value::Null;
        failure(request, "account_read_failed");
    }
    for (field, error) in [
        ("protocolParameters", "invalid_protocol_parameters_data"),
        (
            "stabilityFeeAccumulator",
            "invalid_stability_fee_accumulator_data",
        ),
        ("clock", "invalid_clock"),
    ] {
        for trailing in [false, true] {
            let mut request = fixture.request(FIXED_POINT_ONE);
            let data = request[field]["account"]["data"].as_str().expect("data");
            request[field]["account"]["data"] = json!(if trailing {
                format!("{data}00")
            } else {
                String::new()
            });
            failure(request, error);
        }
    }
    for field in ["protocolParameters", "stabilityFeeAccumulator"] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request[field]["account"]["program_owner"] =
            json!(hex::encode(program_id_bytes([0x22; 8])));
        failure(request, "stablecoin_program_mismatch");
    }
    for (field, value, error) in [
        ("adminId", account_id_hex(id(2)), "admin_mismatch"),
        ("adminId", String::from("invalid"), "invalid_account_id"),
        ("stablecoinProgramId", "00".repeat(32), "invalid_program_id"),
    ] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request[field] = json!(value);
        failure(request, error);
    }
}

fn boundary(request: &str) -> Value {
    let request = CString::new(request).expect("request CString");
    // SAFETY: The request remains a live NUL-terminated UTF-8 string.
    let response =
        unsafe { crate::ffi::stablecoin_set_stability_fee_per_millisecond_plan(request.as_ptr()) };
    assert!(!response.is_null());
    // SAFETY: The library returned this live response string.
    let value =
        serde_json::from_slice(unsafe { CStr::from_ptr(response) }.to_bytes()).expect("envelope");
    // SAFETY: This response came from the library and has not been freed.
    unsafe { crate::ffi::stablecoin_free(response) };
    value
}

#[test]
fn fee_setter_c_boundary_preserves_exact_instruction_and_stable_failures() {
    let fixture = Fixture::new();
    let request = fixture.request(FIXED_POINT_ONE + 9_007_199_254_740_993);
    let response = boundary(&request.to_string());
    assert_eq!(
        response,
        json!({"ok": true, "value": plan(request).expect("plan")})
    );
    for malformed in ["{", "{}"] {
        assert_eq!(
            boundary(malformed),
            json!({"ok": false, "error": "bad_request"})
        );
    }
    for (value, error) in [
        (json!(1e27), "invalid_numeric_value"),
        (json!(u128::MAX.to_string()), "stability_fee_out_of_band"),
        (
            json!("340282366920938463463374607431768211456"),
            "invalid_numeric_value",
        ),
    ] {
        let mut request = fixture.request(FIXED_POINT_ONE);
        request["newRate"] = value;
        assert_eq!(
            boundary(&request.to_string()),
            json!({"ok": false, "error": error})
        );
    }
    let mut overflowing = fixture;
    overflowing.parameters.stability_fee_per_millisecond = FIXED_POINT_ONE * 2;
    overflowing.now = START + 100;
    assert_eq!(
        boundary(&overflowing.request(FIXED_POINT_ONE).to_string()),
        json!({"ok": false, "error": "stability_fee_arithmetic_error"})
    );
}
