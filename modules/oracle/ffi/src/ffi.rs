use std::{
    ffi::{c_char, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::api::{self, FeedAddressesRequest, OracleResult, ProgramInfoRequest};

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
    operation: fn(T) -> OracleResult,
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

/// Resolves the deployed program address, optional verified ImageID, and clock.
///
/// # Safety
/// `request_json` must be null or a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oracle_program_info(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from the caller contract.
    unsafe { call::<ProgramInfoRequest>(request_json, api::program_info) }
}

/// Derives TWAP source/window addresses without asserting account existence.
///
/// # Safety
/// `request_json` must be null or a live NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oracle_feed_addresses(request_json: *const c_char) -> *mut c_char {
    // SAFETY: Forwarded from the caller contract.
    unsafe { call::<FeedAddressesRequest>(request_json, api::feed_addresses) }
}

/// Releases a string returned by this library. Null is accepted.
///
/// # Safety
/// A non-null pointer must be an unreleased allocation returned by this library.
/// Call exactly once for each allocation; do not use another allocator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oracle_free(value: *mut c_char) {
    if !value.is_null() {
        // SAFETY: The caller transfers ownership of our CString allocation.
        drop(unsafe { CString::from_raw(value) });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    /// Returns ownership through the matching ABI function, including errors.
    ///
    /// # Safety
    /// `pointer` must be a live, unreleased allocation from this library.
    unsafe fn response(pointer: *mut c_char) -> Value {
        assert!(!pointer.is_null());
        // SAFETY: pointer is a live allocation returned by one of our C entries.
        let text = unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .expect("UTF-8")
            .to_owned();
        // SAFETY: Ownership is returned exactly once, after copying the string.
        unsafe { oracle_free(pointer) };
        serde_json::from_str(&text).expect("JSON envelope")
    }

    #[test]
    fn malformed_requests_return_owned_error_envelopes() {
        for operation in [oracle_program_info, oracle_feed_addresses] {
            // SAFETY: Null is an explicitly accepted invalid request.
            assert_eq!(
                unsafe { response(operation(std::ptr::null())) },
                json!({"ok": false, "error": "bad_request"})
            );
            for request in ["{", "[]", "null", "true", r#"{"unknown":1}"#] {
                let request = CString::new(request).expect("request");
                // SAFETY: CString lives through the call; response is freed once.
                assert_eq!(
                    unsafe { response(operation(request.as_ptr())) }["error"],
                    "bad_request"
                );
            }
            let non_utf8 = [0xff_u8, 0];
            // SAFETY: This is a live NUL-terminated byte string; invalid UTF-8 is rejected.
            assert_eq!(
                unsafe { response(operation(non_utf8.as_ptr().cast())) }["error"],
                "bad_request"
            );
        }
        // SAFETY: Null is explicitly supported.
        unsafe { oracle_free(std::ptr::null_mut()) };
    }

    #[test]
    fn success_and_domain_failures_survive_the_c_boundary() {
        let request =
            CString::new(json!({"oracleProgramId": "01".repeat(32)}).to_string()).expect("request");
        // SAFETY: CString is live; returned pointer is freed exactly once.
        let result = unsafe { response(oracle_program_info(request.as_ptr())) };
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"]["programIdHex"], "01".repeat(32));
        assert_eq!(result["value"]["imageIdHex"], "");
        for (window, code) in [
            (json!("1"), "invalid_window"),
            (json!("+2048"), "invalid_numeric_value"),
            (json!(2048), "bad_request"),
            (json!(2048.0), "bad_request"),
            (json!("2e4"), "invalid_numeric_value"),
        ] {
            let request = CString::new(json!({"oracleProgramId":"01".repeat(32), "priceSourceId":"02".repeat(32), "windowDurationMs":window}).to_string()).expect("request");
            // SAFETY: CString is live; response owns its allocation until freed.
            assert_eq!(
                unsafe { response(oracle_feed_addresses(request.as_ptr())) },
                json!({"ok":false,"error":code})
            );
        }
    }

    #[test]
    fn panics_do_not_unwind_into_c() {
        fn panicking(_: ProgramInfoRequest) -> OracleResult {
            panic!("test boundary containment")
        }
        let request = CString::new("{}").expect("request");
        // SAFETY: The request lives through this call; the helper frees its result.
        assert_eq!(
            unsafe { response(call(request.as_ptr(), panicking)) },
            json!({"ok":false,"error":"backend_error"})
        );
    }
}
