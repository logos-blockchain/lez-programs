use std::ffi::{c_char, CStr, CString};

use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    program::ProgramId,
};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stablecoin_definition_pda, math::FIXED_POINT_ONE, Instruction, ProtocolParameters,
    RedemptionPriceState,
};
use twap_oracle_core::OraclePriceAccount;

use super::*;
use crate::{
    account::{account_id_hex, decode_account, program_id_bytes},
    AccountRead,
};

const PROGRAM: ProgramId = [0x11; 8];
const OTHER_PRODUCER: ProgramId = [0x99; 8];

fn id(seed: u8) -> AccountId {
    AccountId::new([seed; 32])
}

fn read_value(account_id: AccountId, owner: ProgramId, data: Data) -> Value {
    json!({"id": account_id_hex(account_id), "status": "ok", "account": {
        "program_owner": hex::encode(program_id_bytes(owner)),
        "balance": hex::encode(17_u128.to_le_bytes()),
        "nonce": hex::encode(7_u128.to_le_bytes()), "data": hex::encode(data.as_ref()),
    }})
}

#[derive(Clone, Copy, Debug)]
enum Setter {
    Ratio,
    Gains,
    Timing,
    Admin,
    FreezeAuthority,
    Oracle,
}

impl Setter {
    const ALL: [Self; 6] = [
        Self::Ratio,
        Self::Gains,
        Self::Timing,
        Self::Admin,
        Self::FreezeAuthority,
        Self::Oracle,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Ratio => "set_minimum_collateralization_ratio",
            Self::Gains => "set_controller_gains",
            Self::Timing => "set_timing_parameters",
            Self::Admin => "set_admin",
            Self::FreezeAuthority => "set_freeze_authority",
            Self::Oracle => "set_market_price_oracle",
        }
    }

    fn plan(self, request: Value) -> StablecoinResult {
        match self {
            Self::Ratio => set_minimum_collateralization_ratio_plan(
                serde_json::from_value(request).expect("ratio request"),
            ),
            Self::Gains => {
                set_controller_gains_plan(serde_json::from_value(request).expect("gains request"))
            }
            Self::Timing => {
                set_timing_parameters_plan(serde_json::from_value(request).expect("timing request"))
            }
            Self::Admin => set_admin_plan(serde_json::from_value(request).expect("admin request")),
            Self::FreezeAuthority => {
                set_freeze_authority_plan(serde_json::from_value(request).expect("freeze request"))
            }
            Self::Oracle => set_market_price_oracle_plan(
                serde_json::from_value(request).expect("oracle request"),
            ),
        }
    }

    fn ffi(self) -> unsafe extern "C" fn(*const c_char) -> *mut c_char {
        match self {
            Self::Ratio => crate::ffi::stablecoin_set_minimum_collateralization_ratio_plan,
            Self::Gains => crate::ffi::stablecoin_set_controller_gains_plan,
            Self::Timing => crate::ffi::stablecoin_set_timing_parameters_plan,
            Self::Admin => crate::ffi::stablecoin_set_admin_plan,
            Self::FreezeAuthority => crate::ffi::stablecoin_set_freeze_authority_plan,
            Self::Oracle => crate::ffi::stablecoin_set_market_price_oracle_plan,
        }
    }
}

struct Fixture {
    parameters: ProtocolParameters,
    oracle: OraclePriceAccount,
}

impl Fixture {
    fn new(frozen: bool) -> Self {
        let parameters = ProtocolParameters {
            admin_account_id: id(1),
            freeze_authority_account_id: id(2),
            stablecoin_definition_id: compute_stablecoin_definition_pda(PROGRAM),
            collateral_definition_id: id(4),
            market_price_oracle_id: id(5),
            stability_fee_per_millisecond: FIXED_POINT_ONE,
            controller_proportional_gain: 0,
            controller_integral_gain: 0,
            minimum_collateralization_ratio: FIXED_POINT_ONE * 110 / 100,
            minimum_milliseconds_between_rate_updates: 50,
            maximum_oracle_price_age_milliseconds: 50,
            is_frozen: frozen,
        };
        let oracle = OraclePriceAccount {
            base_asset: parameters.stablecoin_definition_id,
            quote_asset: parameters.collateral_definition_id,
            price: 0,
            timestamp: 0,
            source_id: id(9),
            confidence_interval: 0,
        };
        Self { parameters, oracle }
    }

    fn request(&self, setter: Setter) -> Value {
        let mut request = json!({
            "stablecoinProgramId": hex::encode(program_id_bytes(PROGRAM)),
            "adminId": account_id_hex(self.parameters.admin_account_id),
            "protocolParameters": read_value(compute_protocol_parameters_pda(PROGRAM), PROGRAM, Data::from(&self.parameters)),
        });
        let fields = match setter {
            Setter::Ratio => json!({"newRatio": (FIXED_POINT_ONE * 10).to_string()}),
            Setter::Gains => {
                json!({"newProportionalGain": "-9007199254740993", "newIntegralGain": "9007199254740993"})
            }
            Setter::Timing => {
                json!({"newMinimumMillisecondsBetweenRateUpdates": "1", "newMaximumOraclePriceAgeMilliseconds": "86400000"})
            }
            Setter::Admin => json!({"newAdminId": account_id_hex(id(6))}),
            Setter::FreezeAuthority => json!({"newFreezeAuthorityId": account_id_hex(id(1))}),
            Setter::Oracle => json!({"newOracleId": account_id_hex(id(8)),
                "newOracle": read_value(id(8), OTHER_PRODUCER, Data::from(&self.oracle))}),
        };
        request
            .as_object_mut()
            .expect("request")
            .extend(fields.as_object().expect("fields").clone());
        request
    }
}

fn failure(setter: Setter, request: Value, expected: &str) {
    assert_eq!(
        setter
            .plan(request)
            .expect_err("preflight must reject")
            .code(),
        expected,
        "{setter:?}"
    );
}

fn native_execute(plan: &Value, request: &Value) -> Account {
    let ids: Vec<String> = serde_json::from_value(plan["accountIds"].clone()).expect("ids");
    let signs: Vec<bool> =
        serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
    let parameters_read: AccountRead =
        serde_json::from_value(request["protocolParameters"].clone()).expect("parameters read");
    let (_, old_parameters) = decode_account(&parameters_read).expect("parameters");
    let owner = Account {
        balance: 11,
        nonce: Nonce(3),
        ..Account::default()
    };
    let admin = AccountWithMetadata::new(
        owner.clone(),
        signs[0],
        plan::parse_account_id(&ids[0]).expect("admin id"),
    );
    let parameters = AccountWithMetadata::new(
        old_parameters.clone(),
        signs[1],
        compute_protocol_parameters_pda(PROGRAM),
    );
    let words: Vec<u32> = serde_json::from_value(plan["instruction"].clone()).expect("words");
    let instruction: Instruction = risc0_zkvm::serde::from_slice(&words).expect("instruction");
    let (posts, calls) = match instruction {
        Instruction::SetMinimumCollateralizationRatio { new_ratio } => {
            stablecoin_program::admin::set_minimum_collateralization_ratio(
                admin, parameters, PROGRAM, new_ratio,
            )
        }
        Instruction::SetControllerGains {
            new_proportional_gain,
            new_integral_gain,
        } => stablecoin_program::admin::set_controller_gains(
            admin,
            parameters,
            PROGRAM,
            new_proportional_gain,
            new_integral_gain,
        ),
        Instruction::SetTimingParameters {
            new_minimum_milliseconds_between_rate_updates,
            new_maximum_oracle_price_age_milliseconds,
        } => stablecoin_program::admin::set_timing_parameters(
            admin,
            parameters,
            PROGRAM,
            new_minimum_milliseconds_between_rate_updates,
            new_maximum_oracle_price_age_milliseconds,
        ),
        Instruction::SetAdmin {
            new_admin_account_id,
        } => stablecoin_program::admin::set_admin(admin, parameters, PROGRAM, new_admin_account_id),
        Instruction::SetFreezeAuthority {
            new_freeze_authority_account_id,
        } => stablecoin_program::admin::set_freeze_authority(
            admin,
            parameters,
            PROGRAM,
            new_freeze_authority_account_id,
        ),
        Instruction::SetMarketPriceOracle => {
            let read: AccountRead =
                serde_json::from_value(request["newOracle"].clone()).expect("oracle read");
            let (oracle_id, oracle) = decode_account(&read).expect("oracle");
            assert_eq!(ids[2], account_id_hex(oracle_id));
            let result = stablecoin_program::admin::set_market_price_oracle(
                admin,
                parameters,
                AccountWithMetadata::new(oracle.clone(), signs[2], oracle_id),
                PROGRAM,
            );
            assert_eq!(result.0.get(2).expect("oracle post").account(), &oracle);
            result
        }
        other => panic!("unexpected setter {other:?}"),
    };
    assert!(calls.is_empty());
    assert_eq!(posts.len(), ids.len());
    assert_eq!(posts.first().expect("admin post").account(), &owner);
    let updated = posts.get(1).expect("parameters post").account();
    assert_eq!(updated.program_owner, old_parameters.program_owner);
    assert_eq!(updated.nonce, old_parameters.nonce);
    assert_eq!(updated.balance, old_parameters.balance);
    updated.clone()
}

#[test]
fn all_setters_pin_arguments_account_order_and_guest_flags_and_match_native_mutations() {
    let idl: Value =
        serde_json::from_str(include_str!("../../../../../artifacts/stablecoin-idl.json"))
            .expect("IDL");
    for frozen in [false, true] {
        let fixture = Fixture::new(frozen);
        for setter in Setter::ALL {
            let request = fixture.request(setter);
            let plan = setter
                .plan(request.clone())
                .expect("valid frozen/unfrozen setter");
            assert_eq!(plan["programId"], request["stablecoinProgramId"]);
            let mut ids = vec![
                request["adminId"].clone(),
                request["protocolParameters"]["id"].clone(),
            ];
            let mut signers = vec![true, false];
            let mut flags = json!([
                {"name":"admin", "writable":false, "signer":true, "init":false},
                {"name":"protocol_parameters", "writable":true, "signer":false, "init":false},
            ]);
            if matches!(setter, Setter::Oracle) {
                ids.push(request["newOracleId"].clone());
                signers.push(false);
                flags.as_array_mut().expect("flags").push(
                    json!({"name":"new_oracle", "writable":false, "signer":false, "init":false}),
                );
            }
            assert_eq!(plan["accountIds"], json!(ids));
            assert_eq!(plan["signingRequirements"], json!(signers));
            let entry = idl["instructions"]
                .as_array()
                .expect("instructions")
                .iter()
                .find(|entry| entry["name"] == setter.name())
                .expect("setter IDL");
            assert_eq!(entry["accounts"], flags);
            let words: Vec<u32> =
                serde_json::from_value(plan["instruction"].clone()).expect("words");
            let mut expected = fixture.parameters.clone();
            let instruction = match setter {
                Setter::Ratio => {
                    expected.minimum_collateralization_ratio = FIXED_POINT_ONE * 10;
                    Instruction::SetMinimumCollateralizationRatio {
                        new_ratio: expected.minimum_collateralization_ratio,
                    }
                }
                Setter::Gains => {
                    expected.controller_proportional_gain = -9_007_199_254_740_993;
                    expected.controller_integral_gain = 9_007_199_254_740_993;
                    Instruction::SetControllerGains {
                        new_proportional_gain: expected.controller_proportional_gain,
                        new_integral_gain: expected.controller_integral_gain,
                    }
                }
                Setter::Timing => {
                    expected.minimum_milliseconds_between_rate_updates = 1;
                    expected.maximum_oracle_price_age_milliseconds = 86_400_000;
                    Instruction::SetTimingParameters {
                        new_minimum_milliseconds_between_rate_updates: 1,
                        new_maximum_oracle_price_age_milliseconds: 86_400_000,
                    }
                }
                Setter::Admin => {
                    expected.admin_account_id = id(6);
                    Instruction::SetAdmin {
                        new_admin_account_id: id(6),
                    }
                }
                Setter::FreezeAuthority => {
                    expected.freeze_authority_account_id = id(1);
                    Instruction::SetFreezeAuthority {
                        new_freeze_authority_account_id: id(1),
                    }
                }
                Setter::Oracle => {
                    expected.market_price_oracle_id = id(8);
                    assert_eq!(words.len(), 1);
                    assert_eq!(entry["args"], json!([]));
                    Instruction::SetMarketPriceOracle
                }
            };
            assert_eq!(
                words,
                risc0_zkvm::serde::to_vec(&instruction).expect("encode")
            );
            let updated = native_execute(&plan, &request);
            assert_eq!(
                ProtocolParameters::try_from(&updated.data).expect("updated"),
                expected
            );
        }
    }
}

#[test]
fn ratio_signed_gains_and_timing_accept_inclusive_bands_and_reject_one_unit_outside() {
    let fixture = Fixture::new(false);
    for ratio in [FIXED_POINT_ONE * 110 / 100, FIXED_POINT_ONE * 10] {
        let mut request = fixture.request(Setter::Ratio);
        request["newRatio"] = json!(ratio.to_string());
        let plan = Setter::Ratio
            .plan(request.clone())
            .expect("inclusive ratio");
        native_execute(&plan, &request);
    }
    for ratio in [FIXED_POINT_ONE * 110 / 100 - 1, FIXED_POINT_ONE * 10 + 1] {
        let mut request = fixture.request(Setter::Ratio);
        request["newRatio"] = json!(ratio.to_string());
        failure(
            Setter::Ratio,
            request,
            "collateralization_ratio_out_of_band",
        );
    }
    for (field, cap) in [
        ("newProportionalGain", (FIXED_POINT_ONE * 1_000) as i128),
        ("newIntegralGain", FIXED_POINT_ONE as i128),
    ] {
        for gain in [-cap, 0, cap] {
            let mut request = fixture.request(Setter::Gains);
            request[field] = json!(gain.to_string());
            let plan = Setter::Gains
                .plan(request.clone())
                .expect("inclusive signed gains");
            native_execute(&plan, &request);
        }
        for gain in [-cap - 1, cap + 1, i128::MIN, i128::MAX] {
            let mut request = fixture.request(Setter::Gains);
            request[field] = json!(gain.to_string());
            failure(Setter::Gains, request, "controller_gains_out_of_band");
        }
    }
    for field in [
        "newMinimumMillisecondsBetweenRateUpdates",
        "newMaximumOraclePriceAgeMilliseconds",
    ] {
        for value in [1, 86_400_000] {
            let mut request = fixture.request(Setter::Timing);
            request[field] = json!(value);
            let plan = Setter::Timing
                .plan(request.clone())
                .expect("inclusive timing");
            native_execute(&plan, &request);
        }
        for value in [0, 86_400_001, u64::MAX] {
            let mut request = fixture.request(Setter::Timing);
            request[field] = json!(value.to_string());
            failure(Setter::Timing, request, "timing_parameters_out_of_band");
        }
    }
}

#[test]
fn setters_reject_floats_malformed_decimals_and_out_of_type_values_without_narrowing() {
    let fixture = Fixture::new(false);
    for (setter, field, overflow) in [
        (
            Setter::Ratio,
            "newRatio",
            "340282366920938463463374607431768211456",
        ),
        (
            Setter::Gains,
            "newProportionalGain",
            "170141183460469231731687303715884105728",
        ),
        (
            Setter::Gains,
            "newIntegralGain",
            "-170141183460469231731687303715884105729",
        ),
        (
            Setter::Timing,
            "newMinimumMillisecondsBetweenRateUpdates",
            "18446744073709551616",
        ),
        (
            Setter::Timing,
            "newMaximumOraclePriceAgeMilliseconds",
            "18446744073709551616",
        ),
    ] {
        for invalid in [
            json!(1.5),
            json!("1.5"),
            json!("+1"),
            json!(""),
            json!("invalid"),
            json!(overflow),
            json!(null),
        ] {
            let mut request = fixture.request(setter);
            request[field] = invalid;
            failure(setter, request, "invalid_numeric_value");
        }
    }
    let mut request = fixture.request(Setter::Gains);
    request["newProportionalGain"] = json!(u64::MAX);
    request["newIntegralGain"] = json!(-9_007_199_254_740_993_i64);
    let plan = Setter::Gains
        .plan(request.clone())
        .expect("lossless signed and unsigned JSON integers");
    let parameters = native_execute(&plan, &request);
    let parameters = ProtocolParameters::try_from(&parameters.data).expect("parameters");
    assert_eq!(
        parameters.controller_proportional_gain,
        i128::from(u64::MAX)
    );
    assert_eq!(parameters.controller_integral_gain, -9_007_199_254_740_993);
}

#[test]
fn all_setters_validate_current_admin_and_exact_parameter_pda_ownership_and_reads() {
    let fixture = Fixture::new(true);
    for setter in Setter::ALL {
        let mut request = fixture.request(setter);
        request["adminId"] = json!(account_id_hex(id(2)));
        failure(setter, request, "admin_mismatch");
        // Deliberately substitute or corrupt RPC observations. These exercise
        // initialized-account invariants, not valid native state mutations.
        for (mutation, expected) in [
            (0, "protocol_parameters_pda_mismatch"),
            (1, "stablecoin_program_mismatch"),
            (2, "invalid_protocol_parameters_data"),
            (3, "invalid_protocol_parameters_data"),
            (4, "account_read_failed"),
            (5, "account_read_failed"),
        ] {
            let mut request = fixture.request(setter);
            let read = &mut request["protocolParameters"];
            match mutation {
                0 => read["id"] = json!(account_id_hex(id(99))),
                1 => {
                    read["account"]["program_owner"] =
                        json!(hex::encode(program_id_bytes(OTHER_PRODUCER)))
                }
                2 => read["account"]["data"] = json!(""),
                3 => {
                    let data = read["account"]["data"].as_str().expect("data");
                    read["account"]["data"] = json!(format!("{data}00"));
                }
                4 => read["status"] = json!("not_found"),
                _ => read["account"] = json!(null),
            }
            failure(setter, request, expected);
        }
        let mut request = fixture.request(setter);
        request["adminId"] = json!(id(1).to_string());
        assert!(setter.plan(request).is_ok());
        let mut request = fixture.request(setter);
        request["adminId"] = json!("invalid");
        failure(setter, request, "invalid_account_id");
    }
}

#[test]
fn immediate_role_rotation_changes_authority_and_does_not_require_distinct_handles() {
    let mut fixture = Fixture::new(true);
    let request = fixture.request(Setter::Admin);
    let plan = Setter::Admin
        .plan(request.clone())
        .expect("old admin may rotate");
    let updated = native_execute(&plan, &request);
    fixture.parameters = ProtocolParameters::try_from(&updated.data).expect("rotated");
    let mut old = fixture.request(Setter::Ratio);
    old["adminId"] = json!(account_id_hex(id(1)));
    failure(Setter::Ratio, old, "admin_mismatch");
    let request = fixture.request(Setter::Ratio);
    let plan = Setter::Ratio
        .plan(request.clone())
        .expect("new admin succeeds");
    native_execute(&plan, &request);
    let mut request = fixture.request(Setter::FreezeAuthority);
    request["newFreezeAuthorityId"] = request["adminId"].clone();
    let plan = Setter::FreezeAuthority
        .plan(request.clone())
        .expect("roles may share a handle");
    native_execute(&plan, &request);
    for setter in [Setter::Admin, Setter::FreezeAuthority] {
        let field = if matches!(setter, Setter::Admin) {
            "newAdminId"
        } else {
            "newFreezeAuthorityId"
        };
        for role in [id(7), AccountId::new([0; 32])] {
            let mut request = fixture.request(setter);
            request[field] = json!(role.to_string());
            let plan = setter
                .plan(request.clone())
                .expect("native role values are unrestricted");
            native_execute(&plan, &request);
        }
        let mut request = fixture.request(setter);
        request[field] = json!("invalid");
        failure(setter, request, "invalid_account_id");
    }
}

#[test]
fn gains_leave_redemption_state_and_integral_term_outside_the_execution_write_set() {
    let fixture = Fixture::new(true);
    let mut request = fixture.request(Setter::Gains);
    request["newProportionalGain"] = json!("-42");
    request["newIntegralGain"] = json!("7");
    let plan = Setter::Gains.plan(request.clone()).expect("gains");
    let redemption_id = compute_redemption_price_state_pda(PROGRAM);
    let redemption = Account {
        program_owner: PROGRAM,
        data: Data::from(&RedemptionPriceState {
            redemption_price_at_last_update: FIXED_POINT_ONE,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: -9_007_199_254_740_993,
            last_updated_at: 1_000,
        }),
        ..Account::default()
    };
    let parameters_id = compute_protocol_parameters_pda(PROGRAM);
    let mut ledger = std::collections::HashMap::from([(redemption_id, redemption.clone())]);
    let updated = native_execute(&plan, &request);
    // The native execution has only admin/parameters post-states and no calls.
    // Apply its parameters write to the ledger that also holds redemption data.
    ledger.insert(parameters_id, updated);
    assert_eq!(ledger.get(&redemption_id), Some(&redemption));
    assert!(!plan["accountIds"]
        .as_array()
        .expect("ids")
        .contains(&json!(account_id_hex(redemption_id))));
}

#[test]
fn oracle_replacement_accepts_other_producers_and_stale_zero_observations_but_rejects_wrong_data() {
    let fixture = Fixture::new(true);
    let request = fixture.request(Setter::Oracle);
    let plan = Setter::Oracle
        .plan(request.clone())
        .expect("producer-agnostic stale zero oracle");
    native_execute(&plan, &request);
    for base in [false, true] {
        let mut oracle = fixture.oracle.clone();
        if base {
            oracle.base_asset = id(99);
        } else {
            oracle.quote_asset = id(99);
        }
        let mut request = fixture.request(Setter::Oracle);
        request["newOracle"] = read_value(id(8), OTHER_PRODUCER, Data::from(&oracle));
        failure(Setter::Oracle, request, "oracle_asset_mismatch");
    }
    for invalid in ["", "00", "0102"] {
        let mut request = fixture.request(Setter::Oracle);
        request["newOracle"]["account"]["data"] = json!(invalid);
        failure(Setter::Oracle, request, "invalid_market_price_oracle");
    }
    let mut request = fixture.request(Setter::Oracle);
    let raw = request["newOracle"]["account"]["data"]
        .as_str()
        .expect("data");
    request["newOracle"]["account"]["data"] = json!(format!("{raw}00"));
    failure(Setter::Oracle, request, "invalid_market_price_oracle");
    let mut request = fixture.request(Setter::Oracle);
    request["newOracle"]["status"] = json!("not_found");
    failure(Setter::Oracle, request, "account_read_failed");
    let mut request = fixture.request(Setter::Oracle);
    request["newOracle"]["id"] = json!(account_id_hex(id(99)));
    failure(Setter::Oracle, request, "market_price_oracle_mismatch");
    // A role rotation can make a public oracle holding the current admin handle.
    // Reusing that same account as input three would duplicate input one.
    let mut request = fixture.request(Setter::Oracle);
    request["newOracleId"] = request["adminId"].clone();
    request["newOracle"] = read_value(id(1), OTHER_PRODUCER, Data::from(&fixture.oracle));
    failure(Setter::Oracle, request, "invalid_market_price_oracle");
}

#[test]
fn all_six_c_boundaries_roundtrip_flat_requests_and_reject_null_or_incomplete_json() {
    let fixture = Fixture::new(false);
    for setter in Setter::ALL {
        let payload = CString::new(fixture.request(setter).to_string()).expect("JSON no NUL");
        // SAFETY: payload is live and NUL-terminated for the operation.
        let pointer = unsafe { setter.ffi()(payload.as_ptr()) };
        assert!(!pointer.is_null());
        // SAFETY: pointer is the live, unfreed response returned by this library.
        let response: Value = serde_json::from_slice(unsafe { CStr::from_ptr(pointer) }.to_bytes())
            .expect("response");
        assert_eq!(response["ok"], true);
        assert_eq!(
            response["value"],
            setter.plan(fixture.request(setter)).expect("pure plan")
        );
        // SAFETY: response pointer belongs to this library and has not been freed.
        unsafe { crate::ffi::stablecoin_free(pointer) };
        let incomplete = CString::new("{}").expect("no NUL");
        for pointer in [std::ptr::null(), incomplete.as_ptr()] {
            // SAFETY: null and live NUL-terminated JSON are both allowed inputs.
            let response_pointer = unsafe { setter.ffi()(pointer) };
            assert!(!response_pointer.is_null());
            // SAFETY: response is a live, unfreed library allocation.
            let response: Value =
                serde_json::from_slice(unsafe { CStr::from_ptr(response_pointer) }.to_bytes())
                    .expect("response");
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"], "bad_request");
            // SAFETY: response is a live, unfreed library allocation.
            unsafe { crate::ffi::stablecoin_free(response_pointer) };
        }
    }
}
