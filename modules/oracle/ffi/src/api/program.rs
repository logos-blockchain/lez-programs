use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{account::AccountId, program::ProgramId};
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Map, Value};

use super::{
    parse_oracle_program_id, twap::insert_id, OracleApiError, OracleResult, ProgramInfoRequest,
};
use crate::account::{program_id_bytes, program_id_hex};

/// The program id is the ImageID: configured directly, derived from the binary,
/// or both, in which case they must agree.
pub fn program_info(request: ProgramInfoRequest) -> OracleResult {
    let configured = request
        .oracle_program_id
        .as_deref()
        .map(parse_oracle_program_id)
        .transpose()?;
    let derived = request
        .elf
        .as_deref()
        .map(image_id_from_binary)
        .transpose()?;
    let program_id = match (configured, derived) {
        (Some(configured), Some(derived)) if configured != derived => {
            return Err(OracleApiError::new("program_binary_mismatch"));
        }
        (Some(program_id), _) | (_, Some(program_id)) => program_id,
        (None, None) => return Err(OracleApiError::new("config_missing")),
    };
    let mut result = Map::new();
    let hex = program_id_hex(program_id);
    result.insert("programId".to_owned(), json!(base58_program_id(program_id)));
    result.insert("programIdHex".to_owned(), json!(hex));
    result.insert("imageIdHex".to_owned(), json!(hex));
    insert_id(&mut result, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);
    Ok(Value::Object(result))
}

pub(crate) fn base58_program_id(program_id: ProgramId) -> String {
    AccountId::new(program_id_bytes(program_id)).to_string()
}

fn image_id_from_binary(value: &str) -> Result<ProgramId, OracleApiError> {
    let invalid = || OracleApiError::new("invalid_program_binary");
    let bytes = hex::decode(value).map_err(|_| invalid())?;
    let binary = ProgramBinary::decode(&bytes).map_err(|_| invalid())?;
    binary
        .compute_image_id()
        .map(Into::into)
        .map_err(|_| invalid())
}
