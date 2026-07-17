use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    os::raw::c_char,
    panic::{catch_unwind, AssertUnwindSafe},
};

use base58::FromBase58;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use spel_framework_core::{decode::decode_account_data_try_all, idl::SpelIdl};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecodeRequest {
    idl: SpelIdl,
    accounts: Vec<AccountInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInput {
    id: String,
    data_hex: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DecodeResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
    accounts: Vec<AccountOutput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountOutput {
    id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    account_ids: BTreeMap<String, String>,
}

fn decode_request(request: DecodeRequest) -> DecodeResponse {
    let accounts = request
        .accounts
        .into_iter()
        .map(|account| decode_account(account, &request.idl))
        .collect();
    DecodeResponse {
        status: "ok",
        error: None,
        accounts,
    }
}

fn decode_account(account: AccountInput, idl: &SpelIdl) -> AccountOutput {
    let Ok(data) = hex::decode(&account.data_hex) else {
        return AccountOutput {
            id: account.id,
            status: "invalid_data",
            type_name: None,
            value: None,
            account_ids: BTreeMap::new(),
        };
    };
    let Some((type_name, value)) = decode_account_data_try_all(&data, idl) else {
        return AccountOutput {
            id: account.id,
            status: "unknown_type",
            type_name: None,
            value: None,
            account_ids: BTreeMap::new(),
        };
    };
    let mut account_ids = BTreeMap::new();
    collect_account_ids(&value, &mut account_ids);
    AccountOutput {
        id: account.id,
        status: "decoded",
        type_name: Some(type_name),
        value: Some(value),
        account_ids,
    }
}

fn collect_account_ids(value: &Value, output: &mut BTreeMap<String, String>) {
    match value {
        Value::String(encoded) => {
            let Some(base58) = encoded.strip_prefix("Public/") else {
                return;
            };
            if let Ok(bytes) = base58.from_base58() {
                if bytes.len() == 32 {
                    output.insert(encoded.clone(), hex::encode(bytes));
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_account_ids(nested, output);
            }
        }
        Value::Object(values) => {
            for nested in values.values() {
                collect_account_ids(nested, output);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn error_response(error: &'static str) -> DecodeResponse {
    DecodeResponse {
        status: "error",
        error: Some(error),
        accounts: Vec::new(),
    }
}

fn response_pointer(response: &DecodeResponse) -> *mut c_char {
    let json = serde_json::to_string(response).unwrap_or_else(|_| {
        String::from(r#"{"status":"error","error":"serialization_failed","accounts":[]}"#)
    });
    CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw)
}

#[expect(
    unsafe_code,
    reason = "C ABI input requires reading a caller-owned C string"
)]
fn decode_pointer(request_json: *const c_char) -> DecodeResponse {
    if request_json.is_null() {
        return error_response("null_request");
    }
    let bytes = unsafe {
        // SAFETY: Caller owns a non-null NUL-terminated C string for this call.
        CStr::from_ptr(request_json)
    };
    let Ok(json) = bytes.to_str() else {
        return error_response("invalid_utf8");
    };
    match serde_json::from_str::<DecodeRequest>(json) {
        Ok(request) => decode_request(request),
        Err(_) => error_response("invalid_request"),
    }
}

/// Decodes a JSON batch request using its embedded SPEL IDL.
///
/// Returns a library-owned JSON C string. Release it with
/// [`wallet_idl_decoder_free`].
#[no_mangle]
#[expect(unsafe_code, reason = "C ABI requires a stable exported symbol")]
pub extern "C" fn wallet_idl_decode_accounts(request_json: *const c_char) -> *mut c_char {
    let response = catch_unwind(AssertUnwindSafe(|| decode_pointer(request_json)))
        .unwrap_or_else(|_| error_response("panic"));
    response_pointer(&response)
}

/// Frees a response allocated by [`wallet_idl_decode_accounts`].
///
/// # Safety
///
/// `value` must be null or a pointer returned by this library that has not
/// already been freed.
#[no_mangle]
#[expect(unsafe_code, reason = "C ABI deallocator reconstructs its CString")]
pub unsafe extern "C" fn wallet_idl_decoder_free(value: *mut c_char) {
    if !value.is_null() {
        unsafe {
            // SAFETY: Pointer must come from CString::into_raw in this library and be freed once.
            drop(CString::from_raw(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_idl() -> SpelIdl {
        match serde_json::from_str(include_str!("../../../artifacts/token-idl.json")) {
            Ok(idl) => idl,
            Err(error) => panic!("committed token IDL should parse: {error}"),
        }
    }

    #[test]
    fn decodes_fungible_definition() {
        let request = DecodeRequest {
            idl: token_idl(),
            accounts: vec![AccountInput {
                id: "definition".to_owned(),
                data_hex: concat!(
                    "00", // Fungible variant
                    "04000000",
                    "54455354",                         // TEST
                    "0a000000000000000000000000000000", // supply 10
                    "00",                               // metadata_id None
                    "00"                                // authority None
                )
                .to_owned(),
            }],
        };
        let response = decode_request(request);
        let Some(account) = response.accounts.first() else {
            panic!("decoder should return one account");
        };
        assert_eq!(account.status, "decoded");
        assert_eq!(account.type_name.as_deref(), Some("TokenDefinition"));
        assert_eq!(
            account
                .value
                .as_ref()
                .and_then(|value| value.get("Fungible"))
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str),
            Some("TEST")
        );
    }

    #[test]
    fn maps_decoded_public_ids_to_hex() {
        let request = DecodeRequest {
            idl: token_idl(),
            accounts: vec![AccountInput {
                id: "holding".to_owned(),
                data_hex: format!("00{}19000000000000000000000000000000", "01".repeat(32)),
            }],
        };
        let response = decode_request(request);
        let Some(account) = response.accounts.first() else {
            panic!("decoder should return one account");
        };
        let expected = "01".repeat(32);
        assert_eq!(account.status, "decoded");
        assert_eq!(account.type_name.as_deref(), Some("TokenHolding"));
        assert_eq!(
            account.account_ids.values().next().map(String::as_str),
            Some(expected.as_str())
        );
    }

    #[test]
    fn rejects_invalid_hex_per_account() {
        let response = decode_request(DecodeRequest {
            idl: token_idl(),
            accounts: vec![AccountInput {
                id: "broken".to_owned(),
                data_hex: "xyz".to_owned(),
            }],
        });
        assert_eq!(response.status, "ok");
        assert_eq!(
            response.accounts.first().map(|account| account.status),
            Some("invalid_data")
        );
    }

    #[test]
    #[expect(unsafe_code, reason = "test verifies the exported C allocator pair")]
    fn ffi_allocates_json_and_accepts_its_pointer_on_free() {
        let response = wallet_idl_decode_accounts(std::ptr::null());
        assert!(!response.is_null());
        let json = match unsafe { CStr::from_ptr(response) }.to_str() {
            Ok(json) => json,
            Err(error) => panic!("response should be UTF-8 JSON: {error}"),
        };
        assert!(json.contains("null_request"));
        unsafe { wallet_idl_decoder_free(response) };
    }
}
