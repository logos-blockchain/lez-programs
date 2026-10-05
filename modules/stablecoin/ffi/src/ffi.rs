use std::{
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::api::{
    self, AccrueStabilityFeePlanRequest, CurrentGlobalStateRequest,
    DecodeProtocolParametersRequest, DecodeRedemptionPriceStateRequest,
    DecodeStabilityFeeAccumulatorRequest, DepositCollateralPlanRequest,
    InitializeProgramPlanRequest, OpenPositionPlanRequest, PositionAddressesRequest,
    ProgramInfoRequest, RedemptionRateUpdateQuoteRequest, RefreshGlobalsPlanRequest,
    RepayDebtPlanRequest, StablecoinResult, UpdateRedemptionRatePlanRequest,
};

#[derive(Serialize)]
struct Envelope {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Envelope {
    fn success(value: serde_json::Value) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

/// # Safety
/// `request` must be null or point to a live NUL-terminated byte string for
/// the duration of this call.
unsafe fn call<T: DeserializeOwned>(
    request: *const c_char,
    operation: fn(T) -> StablecoinResult,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Forwarded from the exported C function's caller contract.
        let request = unsafe { request_text(request) }?;
        let request =
            serde_json::from_str::<T>(&request).map_err(|_| String::from("bad_request"))?;
        operation(request).map_err(|error| error.to_string())
    }));

    let envelope = match result {
        Ok(Ok(value)) => Envelope::success(value),
        Ok(Err(error)) => Envelope::failure(error),
        Err(_) => Envelope::failure("backend_error"),
    };
    encode_envelope(&envelope)
}

/// # Safety
/// `request` must be null or point to a live NUL-terminated byte string for
/// the duration of this call.
unsafe fn request_text(request: *const c_char) -> Result<String, String> {
    if request.is_null() {
        return Err(String::from("bad_request"));
    }
    // SAFETY: The caller passes a live NUL-terminated UTF-8 buffer for this call.
    let request = unsafe { CStr::from_ptr(request) };
    request
        .to_str()
        .map(String::from)
        .map_err(|_| String::from("bad_request"))
}

fn encode_envelope(envelope: &Envelope) -> *mut c_char {
    let json = serde_json::to_string(envelope)
        .unwrap_or_else(|_| String::from(r#"{"ok":false,"error":"backend_error"}"#));
    match CString::new(json) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"backend_error"}"#)
            .map_or(std::ptr::null_mut(), CString::into_raw),
    }
}

#[unsafe(no_mangle)]
/// Resolves the stablecoin program ID and derives all singleton account IDs.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_program_info(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<ProgramInfoRequest>(request_json, api::program_info) }
}

#[unsafe(no_mangle)]
/// Decodes and validates the singleton `ProtocolParameters` account.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_decode_protocol_parameters(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<DecodeProtocolParametersRequest>(request_json, api::decode_protocol_parameters)
    }
}

#[unsafe(no_mangle)]
/// Decodes and validates the singleton `StabilityFeeAccumulator` account.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_decode_stability_fee_accumulator(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<DecodeStabilityFeeAccumulatorRequest>(
            request_json,
            api::decode_stability_fee_accumulator,
        )
    }
}

#[unsafe(no_mangle)]
/// Decodes and validates the singleton `RedemptionPriceState` account.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_decode_redemption_price_state(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<DecodeRedemptionPriceStateRequest>(request_json, api::decode_redemption_price_state)
    }
}

#[unsafe(no_mangle)]
/// Validates stablecoin global accounts and projects their current values at `CLOCK_01`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_current_global_state(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<CurrentGlobalStateRequest>(request_json, api::current_global_state) }
}

#[unsafe(no_mangle)]
/// Quotes the next redemption-rate controller update without submitting it.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_redemption_rate_update_quote(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<RedemptionRateUpdateQuoteRequest>(request_json, api::redemption_rate_update_quote)
    }
}

#[unsafe(no_mangle)]
/// Builds the exact wallet submission plan for `AccrueStabilityFee`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_accrue_stability_fee_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<AccrueStabilityFeePlanRequest>(request_json, api::accrue_stability_fee_plan) }
}

#[unsafe(no_mangle)]
/// Builds a preflighted wallet submission plan for `UpdateRedemptionRate`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_update_redemption_rate_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<UpdateRedemptionRatePlanRequest>(request_json, api::update_redemption_rate_plan)
    }
}

#[unsafe(no_mangle)]
/// Builds the best-effort wallet submission plan for `RefreshGlobals`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_refresh_globals_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<RefreshGlobalsPlanRequest>(request_json, api::refresh_globals_plan) }
}

#[unsafe(no_mangle)]
/// Builds the exact wallet submission plan for `InitializeProgram`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_initialize_program_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<InitializeProgramPlanRequest>(request_json, api::initialize_program_plan) }
}

#[unsafe(no_mangle)]
/// Builds the exact wallet submission plan for `OpenPosition`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_open_position_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<OpenPositionPlanRequest>(request_json, api::open_position_plan) }
}

#[unsafe(no_mangle)]
/// Derives a position and its collateral vault from owner and nonce.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_position_addresses(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<PositionAddressesRequest>(request_json, api::position_addresses) }
}

#[unsafe(no_mangle)]
/// Builds the exact wallet submission plan for `DepositCollateral`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_deposit_collateral_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<DepositCollateralPlanRequest>(request_json, api::deposit_collateral_plan) }
}

#[unsafe(no_mangle)]
/// Builds a preflighted wallet submission plan for `RepayDebt`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_repay_debt_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<RepayDebtPlanRequest>(request_json, api::repay_debt_plan) }
}

#[unsafe(no_mangle)]
/// Quotes position health using validated live state and wide arithmetic.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_position_health(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::PositionHealthRequest>(request_json, api::position_health) }
}

#[unsafe(no_mangle)]
/// Builds a preflighted owner-signed plan for `WithdrawCollateral`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_withdraw_collateral_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::WithdrawCollateralPlanRequest>(request_json, api::withdraw_collateral_plan)
    }
}

#[unsafe(no_mangle)]
/// Builds a preflighted owner-signed plan for `GenerateDebt`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_generate_debt_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::GenerateDebtPlanRequest>(request_json, api::generate_debt_plan) }
}

#[unsafe(no_mangle)]
/// Builds an owner-signed, zero-argument plan for closing a settled Position.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_close_position_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::ClosePositionPlanRequest>(request_json, api::close_position_plan) }
}

#[unsafe(no_mangle)]
/// Builds the current-admin plan for `SetMinimumCollateralizationRatio`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_minimum_collateralization_ratio_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::SetMinimumCollateralizationRatioPlanRequest>(
            request_json,
            api::set_minimum_collateralization_ratio_plan,
        )
    }
}

#[unsafe(no_mangle)]
/// Builds a paired signed-gain plan for `SetControllerGains`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_controller_gains_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::SetControllerGainsPlanRequest>(request_json, api::set_controller_gains_plan)
    }
}

#[unsafe(no_mangle)]
/// Builds a paired timing plan for `SetTimingParameters`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_timing_parameters_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::SetTimingParametersPlanRequest>(request_json, api::set_timing_parameters_plan)
    }
}

#[unsafe(no_mangle)]
/// Builds a one-step plan for `SetAdmin`, without a new-admin signer.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_admin_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::SetAdminPlanRequest>(request_json, api::set_admin_plan) }
}

#[unsafe(no_mangle)]
/// Builds an admin-authorized plan for `SetFreezeAuthority`.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_freeze_authority_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::SetFreezeAuthorityPlanRequest>(request_json, api::set_freeze_authority_plan)
    }
}

#[unsafe(no_mangle)]
/// Builds a zero-argument oracle-replacement plan with the replacement as account three.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_set_market_price_oracle_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<api::SetMarketPriceOraclePlanRequest>(
            request_json,
            api::set_market_price_oracle_plan,
        )
    }
}

#[unsafe(no_mangle)]
/// Builds the idempotent, current-authority plan for the unit `Freeze` instruction.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_freeze_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::FreezeAuthorityPlanRequest>(request_json, api::freeze_plan) }
}

#[unsafe(no_mangle)]
/// Builds the idempotent, current-authority plan for the unit `Unfreeze` instruction.
///
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn stablecoin_unfreeze_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<api::FreezeAuthorityPlanRequest>(request_json, api::unfreeze_plan) }
}

/// Releases a string returned by a `stablecoin_*` operation.
///
/// # Safety
/// `value` must be null or a pointer returned by this library that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stablecoin_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    // SAFETY: The caller contract requires a pointer produced by CString::into_raw above.
    drop(unsafe { CString::from_raw(value) });
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;

    /// # Safety
    /// `response` must be a live pointer returned by a `stablecoin_*` operation.
    unsafe fn assert_failure_response(response: *mut c_char, expected: &str) {
        assert!(!response.is_null());
        // SAFETY: Forwarded from this helper's caller contract.
        let text = unsafe { CStr::from_ptr(response) };
        let text = match text.to_str() {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        let value: serde_json::Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], expected);
        // SAFETY: response came from this library and has not been freed.
        unsafe { stablecoin_free(response) };
    }

    #[test]
    fn malformed_json_uses_boundary_failure_envelope() {
        let request = match CString::new("{") {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        // SAFETY: request is a live NUL-terminated CString for this call.
        let response = unsafe { stablecoin_program_info(request.as_ptr()) };
        // SAFETY: response was returned by stablecoin_program_info and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn null_request_uses_boundary_failure_envelope() {
        // SAFETY: null is explicitly accepted and mapped to bad_request.
        let response = unsafe { stablecoin_program_info(std::ptr::null()) };
        // SAFETY: response was returned by stablecoin_program_info and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn current_global_state_rejects_json_floats_at_the_boundary() {
        let request = match CString::new(r#"{"stablecoinProgramId":1.5}"#) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        // SAFETY: request is a live NUL-terminated CString for this call.
        let response = unsafe { stablecoin_current_global_state(request.as_ptr()) };
        // SAFETY: response was returned by stablecoin_current_global_state and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn redemption_rate_quote_rejects_malformed_and_float_requests_at_the_boundary() {
        for input in ["{", r#"{"stablecoinProgramId":1.5}"#] {
            let request = match CString::new(input) {
                Ok(value) => value,
                Err(error) => panic!("{error}"),
            };
            // SAFETY: request is a live NUL-terminated CString for this call.
            let response = unsafe { stablecoin_redemption_rate_update_quote(request.as_ptr()) };
            // SAFETY: response came from the quote operation and remains live.
            unsafe { assert_failure_response(response, "bad_request") };
        }
    }

    #[test]
    fn poke_plans_reject_malformed_requests_at_the_boundary() {
        let operations: [unsafe extern "C" fn(*const c_char) -> *mut c_char; 3] = [
            stablecoin_accrue_stability_fee_plan,
            stablecoin_update_redemption_rate_plan,
            stablecoin_refresh_globals_plan,
        ];
        for operation in operations {
            let request = match CString::new("{") {
                Ok(value) => value,
                Err(error) => panic!("{error}"),
            };
            // SAFETY: request is a live NUL-terminated CString for this call.
            let response = unsafe { operation(request.as_ptr()) };
            // SAFETY: response came from the selected poke-plan operation and remains live.
            unsafe { assert_failure_response(response, "bad_request") };
        }
    }

    #[test]
    fn open_position_requires_decimal_strings_at_the_boundary() {
        let account_read = serde_json::json!({
            "id": "1111111111111111111111111111111111111111111111111111111111111111",
            "status": "ok",
            "account": {
                "program_owner": "2222222222222222222222222222222222222222222222222222222222222222",
                "balance": "00000000000000000000000000000000",
                "nonce": "00000000000000000000000000000000",
                "data": ""
            }
        });
        for request in [
            serde_json::json!({
                "stablecoinProgramId": "1111111111111111111111111111111111111111111111111111111111111111",
                "ownerId": "2222222222222222222222222222222222222222222222222222222222222222",
                "positionNonce": 1,
                "initialCollateralAmount": "1",
                "userCollateralHoldingId": "3333333333333333333333333333333333333333333333333333333333333333",
                "userCollateralHolding": account_read,
                "collateralDefinition": account_read,
                "protocolParameters": account_read,
                "clock": account_read
            }),
            serde_json::json!({
                "stablecoinProgramId": "1111111111111111111111111111111111111111111111111111111111111111",
                "ownerId": "2222222222222222222222222222222222222222222222222222222222222222",
                "positionNonce": "1",
                "initialCollateralAmount": 1.5,
                "userCollateralHoldingId": "3333333333333333333333333333333333333333333333333333333333333333",
                "userCollateralHolding": account_read,
                "collateralDefinition": account_read,
                "protocolParameters": account_read,
                "clock": account_read
            }),
        ] {
            let request = match CString::new(request.to_string()) {
                Ok(value) => value,
                Err(error) => panic!("{error}"),
            };
            // SAFETY: request remains a live NUL-terminated string for this call.
            let response = unsafe { stablecoin_open_position_plan(request.as_ptr()) };
            // SAFETY: response came from stablecoin_open_position_plan and remains live.
            unsafe { assert_failure_response(response, "bad_request") };
        }
    }

    #[test]
    fn deposit_collateral_rejects_float_amounts_and_preserves_decimal_strings() {
        let not_found = serde_json::json!({
            "id": "1111111111111111111111111111111111111111111111111111111111111111",
            "status": "not_found"
        });
        let request = serde_json::json!({
            "stablecoinProgramId": "1111111111111111111111111111111111111111111111111111111111111111",
            "ownerId": "2222222222222222222222222222222222222222222222222222222222222222",
            "positionNonce": "18446744073709551615",
            "amount": 1.5,
            "userCollateralHoldingId": "3333333333333333333333333333333333333333333333333333333333333333",
            "position": not_found,
            "vault": not_found,
            "userCollateralHolding": not_found,
            "protocolParameters": not_found
        });
        let request = match CString::new(request.to_string()) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        // SAFETY: request remains a live NUL-terminated string for this call.
        let response = unsafe { stablecoin_deposit_collateral_plan(request.as_ptr()) };
        // SAFETY: response came from stablecoin_deposit_collateral_plan and remains live.
        unsafe { assert_failure_response(response, "invalid_numeric_value") };
    }

    #[test]
    fn position_address_derivation_requires_a_decimal_nonce_string() {
        let request = match CString::new(
            r#"{"stablecoinProgramId":"1111111111111111111111111111111111111111111111111111111111111111","ownerId":"2222222222222222222222222222222222222222222222222222222222222222","positionNonce":1}"#,
        ) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        // SAFETY: request remains a live NUL-terminated string for this call.
        let response = unsafe { stablecoin_position_addresses(request.as_ptr()) };
        // SAFETY: response came from stablecoin_position_addresses and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn null_free_is_safe() {
        // SAFETY: null is explicitly allowed by the function contract.
        unsafe { stablecoin_free(std::ptr::null_mut()) };
    }

    #[test]
    fn close_position_rejects_null_malformed_and_nonstring_nonce_requests() {
        // SAFETY: null is explicitly supported by the boundary contract.
        let response = unsafe { stablecoin_close_position_plan(std::ptr::null()) };
        // SAFETY: response is an unfreed pointer returned by this library.
        unsafe { assert_failure_response(response, "bad_request") };
        for nonce in [serde_json::json!(1), serde_json::json!(1.5)] {
            let missing = serde_json::json!({"id": "", "status": "not_found"});
            let payload = serde_json::json!({
                "stablecoinProgramId": "1111111111111111111111111111111111111111111111111111111111111111",
                "ownerId": "2222222222222222222222222222222222222222222222222222222222222222",
                "positionNonce": nonce, "position": missing, "vault": missing,
                "protocolParameters": missing,
            });
            let request = CString::new(payload.to_string()).expect("JSON has no NUL");
            // SAFETY: request remains a live NUL-terminated string for this call.
            let response = unsafe { stablecoin_close_position_plan(request.as_ptr()) };
            // SAFETY: response is an unfreed pointer returned by this library.
            unsafe { assert_failure_response(response, "bad_request") };
        }
        let request = CString::new("{").expect("no NUL");
        // SAFETY: request remains a live NUL-terminated string for this call.
        let response = unsafe { stablecoin_close_position_plan(request.as_ptr()) };
        // SAFETY: response is an unfreed pointer returned by this library.
        unsafe { assert_failure_response(response, "bad_request") };
    }
}
