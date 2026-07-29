use serde_json::{json, Value};

use super::position::{QuoteComputation, QuoteFailure};

pub(super) fn issue(code: &str, message: &str, fields: &[&str], details: Value) -> Value {
    json!({
        "code": code,
        "message": message,
        "details": details,
        "recoverable": true,
        "blockingFields": fields,
    })
}

pub(super) fn fatal_quote(
    code: &'static str,
    fields: &[&'static str],
    details: Value,
) -> QuoteComputation {
    QuoteComputation::Failed(QuoteFailure {
        code,
        fields: fields.to_vec(),
        details,
    })
}
