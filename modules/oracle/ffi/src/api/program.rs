use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::AccountId,
    program::{ProgramHeader, ProgramId, PROGRAM_LOADER_ACCOUNT_ID},
};
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Map, Value};

use super::{parse_id, twap::insert_id, OracleApiError, OracleResult, ProgramInfoRequest};
use crate::account::{account_id_hex, decode_account, AccountRead};

/// The configured ID names a deployed loader header. A binary only supplies an
/// ImageID to compare with that live header; it can never provide the address.
pub fn program_info(request: ProgramInfoRequest) -> OracleResult {
    let program_id = request
        .oracle_program_id
        .as_deref()
        .map(|value| parse_id(value, "invalid_program_id"))
        .transpose()?
        .ok_or_else(|| OracleApiError::new("config_missing"))?;
    let image_id = match request.elf.as_deref() {
        None => None,
        Some(elf) => {
            let image_id = image_id_from_binary(elf)?;
            let header = request
                .program_header
                .as_ref()
                .ok_or_else(|| OracleApiError::new("program_header_unavailable"))?;
            if deployed_image_id(header, program_id)? != image_id {
                return Err(OracleApiError::new("program_binary_mismatch"));
            }
            Some(image_id)
        }
    };
    let mut result = Map::new();
    insert_id(&mut result, "programId", program_id);
    insert_id(&mut result, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);
    result.insert(
        "imageIdHex".to_owned(),
        json!(image_id.map(account_id_hex).unwrap_or_default()),
    );
    Ok(Value::Object(result))
}

fn image_id_from_binary(value: &str) -> Result<AccountId, OracleApiError> {
    let invalid = || OracleApiError::new("invalid_program_binary");
    let bytes = hex::decode(value).map_err(|_| invalid())?;
    let binary = ProgramBinary::decode(&bytes).map_err(|_| invalid())?;
    let image_id: ProgramId = binary
        .compute_image_id()
        .map(Into::into)
        .map_err(|_| invalid())?;
    Ok(AccountId::from(image_id))
}

fn deployed_image_id(
    read: &AccountRead,
    program_id: AccountId,
) -> Result<AccountId, OracleApiError> {
    let unavailable = || OracleApiError::new("program_header_unavailable");
    let (id, account) = decode_account(read).map_err(|_| unavailable())?;
    if id != program_id || account.program_owner != PROGRAM_LOADER_ACCOUNT_ID {
        return Err(unavailable());
    }
    let header = ProgramHeader::from_bytes(account.data.as_ref()).ok_or_else(unavailable)?;
    Ok(AccountId::from(header.image_id))
}
