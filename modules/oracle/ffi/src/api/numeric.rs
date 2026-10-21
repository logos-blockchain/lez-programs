//! Exact decimal wire values. JSON numbers never enter these parsers.

use super::OracleApiError;

fn parse<T: std::str::FromStr>(value: &str, signed: bool) -> Result<T, OracleApiError> {
    let digits = if signed {
        value.strip_prefix('-').unwrap_or(value)
    } else {
        value
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(OracleApiError::new("invalid_numeric_value"));
    }
    value
        .parse()
        .map_err(|_| OracleApiError::new("invalid_numeric_value"))
}

pub fn parse_u64(value: &str) -> Result<u64, OracleApiError> {
    parse(value, false)
}

pub fn parse_u128(value: &str) -> Result<u128, OracleApiError> {
    parse(value, false)
}

pub fn parse_i32(value: &str) -> Result<i32, OracleApiError> {
    parse(value, true)
}

pub fn parse_i64(value: &str) -> Result<i64, OracleApiError> {
    parse(value, true)
}
