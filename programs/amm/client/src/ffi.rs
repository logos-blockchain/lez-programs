//! C ABI for the lossless JSON AMM client protocol.

#![allow(
    unsafe_code,
    reason = "raw C strings and paired allocation ownership are confined to this module"
)]

use std::{
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};

use serde::Serialize;
use serde_json::Value;

use crate::wire::{self, WireError};

type Operation = fn(Value) -> Result<Value, WireError>;

#[derive(Serialize)]
struct Envelope {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorPayload>,
}

impl Envelope {
    fn success(value: Value) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    fn failure(error: ErrorPayload) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error),
        }
    }
}

#[derive(Serialize)]
struct ErrorPayload {
    code: String,
    message: String,
}

impl ErrorPayload {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn from_wire(error: WireError) -> Self {
        Self::new(error.code(), error.to_string())
    }
}

/// Calls one JSON operation and converts every outcome into an owned C string.
///
/// # Safety
///
/// `request_json` must be null or point to a live NUL-terminated byte string for the duration of
/// this call. A non-null return value must be released exactly once with [`amm_client_free`].
unsafe fn call(request_json: *const c_char, operation: Operation) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: The exported caller contract establishes pointer validity and lifetime. The
        // helper validates nullness before constructing `CStr`.
        let request = unsafe { request_value(request_json) }?;
        operation(request).map_err(ErrorPayload::from_wire)
    }));

    let envelope = match result {
        Ok(Ok(value)) => Envelope::success(value),
        Ok(Err(error)) => Envelope::failure(error),
        Err(_) => Envelope::failure(ErrorPayload::new(
            "internal_panic",
            "AMM client operation panicked",
        )),
    };
    encode_envelope(&envelope)
}

/// Reads and parses one caller-owned JSON C string.
///
/// # Safety
///
/// `request_json` must be null or point to a live NUL-terminated byte string for this call.
unsafe fn request_value(request_json: *const c_char) -> Result<Value, ErrorPayload> {
    if request_json.is_null() {
        return Err(ErrorPayload::new("null_request", "request pointer is null"));
    }

    // SAFETY: Nullness was checked above. Remaining validity, lifetime, and NUL-termination are
    // required by the exported caller contract.
    let request = unsafe { CStr::from_ptr(request_json) };
    let request = request.to_str().map_err(|error| {
        ErrorPayload::new("invalid_utf8", format!("request is not UTF-8: {error}"))
    })?;
    serde_json::from_str(request).map_err(|error| {
        ErrorPayload::new(
            "invalid_json",
            format!("request is not valid JSON: {error}"),
        )
    })
}

fn encode_envelope(envelope: &Envelope) -> *mut c_char {
    let json = match serde_json::to_string(envelope) {
        Ok(json) => json,
        Err(_) => String::from(
            r#"{"ok":false,"error":{"code":"response_serialization_failed","message":"response serialization failed"}}"#,
        ),
    };

    match CString::new(json) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(
            r#"{"ok":false,"error":{"code":"response_contains_nul","message":"response contains NUL"}}"#,
        )
        .map_or(std::ptr::null_mut(), CString::into_raw),
    }
}

/// Builds a canonical AMM transaction plan from a tagged JSON request.
///
/// Returned JSON owns its memory and must be released with [`amm_client_free`].
///
/// # Safety
///
/// `request_json` must be null or point to a live NUL-terminated UTF-8 byte string for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amm_client_plan(request_json: *const c_char) -> *mut c_char {
    // SAFETY: This function exposes the same pointer contract as `call`.
    unsafe { call(request_json, wire::plan_json) }
}

/// Evaluates a canonical AMM economic quote from a tagged JSON request.
///
/// Returned JSON owns its memory and must be released with [`amm_client_free`].
///
/// # Safety
///
/// `request_json` must be null or point to a live NUL-terminated UTF-8 byte string for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amm_client_quote(request_json: *const c_char) -> *mut c_char {
    // SAFETY: This function exposes the same pointer contract as `call`.
    unsafe { call(request_json, wire::quote_json) }
}

/// Releases a response returned by [`amm_client_plan`] or [`amm_client_quote`].
///
/// # Safety
///
/// `value` must be null or a pointer returned by this library that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn amm_client_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    // SAFETY: The caller contract requires a unique, live pointer produced by
    // `CString::into_raw` in `encode_envelope`.
    drop(unsafe { CString::from_raw(value) });
}
