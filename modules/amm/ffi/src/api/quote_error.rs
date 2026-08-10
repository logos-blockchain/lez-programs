use serde_json::{json, Value};

pub(super) fn issue(code: &str, message: &str, fields: &[&str], details: Value) -> Value {
    json!({
        "code": code,
        "message": message,
        "details": details,
        "recoverable": true,
        "blockingFields": fields,
    })
}
