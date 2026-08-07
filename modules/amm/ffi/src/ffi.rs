use std::{
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::api::{
    self, AmmApiError, AmmResult, ConfigIdRequest, ContextRequest, CreatePoolPlanRequest,
    LiquidityQuoteRequest, PairIdsRequest, PlanRequest, PoolIdRequest, ProgramIdRequest,
    QuoteRequest, ResolvePoolRequest, SwapExactInPlanRequest, SwapExactInQuoteRequest,
    SwapExactOutPlanRequest, SwapExactOutQuoteRequest, SwapPairRequest, TokenIdsRequest,
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

fn call<T: DeserializeOwned>(request: *const c_char, operation: fn(T) -> AmmResult) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let request = request_text(request).map_err(AmmApiError::from)?;
        let request = serde_json::from_str::<T>(&request)
            .map_err(|error| AmmApiError::from(format!("invalid request JSON: {error}")))?;
        operation(request)
    }));

    let envelope = match result {
        Ok(Ok(value)) => Envelope::success(value),
        Ok(Err(error)) => Envelope::failure(error.to_string()),
        Err(_) => Envelope::failure("internal panic"),
    };
    encode_envelope(&envelope)
}

fn request_text(request: *const c_char) -> Result<String, String> {
    if request.is_null() {
        return Err(String::from("request pointer is null"));
    }
    // SAFETY: The C++ caller passes a live NUL-terminated UTF-8 buffer for this call.
    let request = unsafe { CStr::from_ptr(request) };
    request
        .to_str()
        .map(String::from)
        .map_err(|error| format!("request is not UTF-8: {error}"))
}

fn encode_envelope(envelope: &Envelope) -> *mut c_char {
    let json = serde_json::to_string(envelope).unwrap_or_else(|_| {
        String::from(r#"{"ok":false,"error":"response serialization failed"}"#)
    });
    match CString::new(json) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"response contains NUL"}"#)
            .map_or(std::ptr::null_mut(), CString::into_raw),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_config_id(request_json: *const c_char) -> *mut c_char {
    call::<ConfigIdRequest>(request_json, api::config_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_token_ids(request_json: *const c_char) -> *mut c_char {
    call::<TokenIdsRequest>(request_json, api::token_ids)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_pair_ids(request_json: *const c_char) -> *mut c_char {
    call::<PairIdsRequest>(request_json, api::pair_ids)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_context(request_json: *const c_char) -> *mut c_char {
    call::<ContextRequest>(request_json, api::context)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_quote(request_json: *const c_char) -> *mut c_char {
    call::<QuoteRequest>(request_json, api::quote)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_plan(request_json: *const c_char) -> *mut c_char {
    call::<PlanRequest>(request_json, api::plan)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_swap_pair(request_json: *const c_char) -> *mut c_char {
    call::<SwapPairRequest>(request_json, api::swap_pair)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_resolve_pool(request_json: *const c_char) -> *mut c_char {
    call::<ResolvePoolRequest>(request_json, api::resolve_pool)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_pool_id(request_json: *const c_char) -> *mut c_char {
    call::<PoolIdRequest>(request_json, api::pool_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_swap_exact_in_quote(request_json: *const c_char) -> *mut c_char {
    call::<SwapExactInQuoteRequest>(request_json, api::swap_exact_in_quote)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_swap_exact_out_quote(request_json: *const c_char) -> *mut c_char {
    call::<SwapExactOutQuoteRequest>(request_json, api::swap_exact_out_quote)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_swap_exact_in_plan(request_json: *const c_char) -> *mut c_char {
    call::<SwapExactInPlanRequest>(request_json, api::swap_exact_in_plan)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_swap_exact_out_plan(request_json: *const c_char) -> *mut c_char {
    call::<SwapExactOutPlanRequest>(request_json, api::swap_exact_out_plan)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_liquidity_quote(request_json: *const c_char) -> *mut c_char {
    call::<LiquidityQuoteRequest>(request_json, api::liquidity_quote)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_create_pool_plan(request_json: *const c_char) -> *mut c_char {
    call::<CreatePoolPlanRequest>(request_json, api::create_pool_plan)
}

#[unsafe(no_mangle)]
pub extern "C" fn amm_program_id(request_json: *const c_char) -> *mut c_char {
    call::<ProgramIdRequest>(request_json, api::program_id)
}

/// Releases a string returned by an `amm_*` operation.
///
/// # Safety
/// `value` must be null or a pointer returned by this library that has not been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amm_free(value: *mut c_char) {
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

    #[test]
    fn malformed_json_uses_boundary_failure_envelope() {
        let request = CString::new("{").unwrap();
        let response = amm_config_id(request.as_ptr());
        assert!(!response.is_null());
        // SAFETY: response was returned by amm_config_id and remains live until amm_free.
        let text = unsafe { CStr::from_ptr(response) }.to_str().unwrap();
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["ok"], false);
        // SAFETY: response was allocated by this library and has not been freed.
        unsafe { amm_free(response) };
    }
}
