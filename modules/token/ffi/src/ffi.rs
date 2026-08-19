use std::{
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::api::{
    self, BurnPlanRequest, CreateFungiblePlanRequest, CreateFungibleWithMetadataPlanRequest,
    CreateNonFungiblePlanRequest, DecodeAccountRequest, DecodeDefinitionRequest,
    DecodeHoldingRequest, DecodeMetadataRequest, InitializeHoldingPlanRequest, MintPlanRequest,
    MintWithAuthorityPlanRequest, PrintNftPlanRequest, ProgramIdRequest, SetAuthorityPlanRequest,
    SetAuthorityWithAuthorityPlanRequest, TokenResult, TransferPlanRequest,
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
    operation: fn(T) -> TokenResult,
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
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_program_id(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<ProgramIdRequest>(request_json, api::program_id) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_decode_definition(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<DecodeDefinitionRequest>(request_json, api::decode_definition) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_decode_holding(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<DecodeHoldingRequest>(request_json, api::decode_holding) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_decode_metadata(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<DecodeMetadataRequest>(request_json, api::decode_metadata) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_decode_account(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<DecodeAccountRequest>(request_json, api::decode_account) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_create_fungible_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<CreateFungiblePlanRequest>(request_json, api::create_fungible_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_create_fungible_with_metadata_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<CreateFungibleWithMetadataPlanRequest>(
            request_json,
            api::create_fungible_with_metadata_plan,
        )
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_create_non_fungible_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<CreateNonFungiblePlanRequest>(request_json, api::create_non_fungible_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_initialize_holding_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<InitializeHoldingPlanRequest>(request_json, api::initialize_holding_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_transfer_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<TransferPlanRequest>(request_json, api::transfer_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_burn_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<BurnPlanRequest>(request_json, api::burn_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_mint_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<MintPlanRequest>(request_json, api::mint_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_mint_with_authority_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<MintWithAuthorityPlanRequest>(request_json, api::mint_with_authority_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_set_authority_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<SetAuthorityPlanRequest>(request_json, api::set_authority_plan) }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_set_authority_with_authority_plan(
    request_json: *const c_char,
) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe {
        call::<SetAuthorityWithAuthorityPlanRequest>(
            request_json,
            api::set_authority_with_authority_plan,
        )
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// `request_json` must be null or point to a live NUL-terminated byte string.
pub unsafe extern "C" fn token_print_nft_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from this function's caller contract.
    unsafe { call::<PrintNftPlanRequest>(request_json, api::print_nft_plan) }
}

/// Releases a string returned by a `token_*` operation.
///
/// # Safety
/// `value` must be null or a pointer returned by this library that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn token_free(value: *mut c_char) {
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
    /// `response` must be a live pointer returned by a `token_*` operation.
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
        unsafe { token_free(response) };
    }

    #[test]
    fn malformed_json_uses_boundary_failure_envelope() {
        let request = match CString::new("{") {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        };
        // SAFETY: request is a live NUL-terminated CString for this call.
        let response = unsafe { token_program_id(request.as_ptr()) };
        // SAFETY: response was returned by token_program_id and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn null_request_uses_boundary_failure_envelope() {
        // SAFETY: null is explicitly accepted and mapped to bad_request.
        let response = unsafe { token_program_id(std::ptr::null()) };
        // SAFETY: response was returned by token_program_id and remains live.
        unsafe { assert_failure_response(response, "bad_request") };
    }

    #[test]
    fn null_free_is_safe() {
        // SAFETY: null is explicitly allowed by the function contract.
        unsafe { token_free(std::ptr::null_mut()) };
    }
}
