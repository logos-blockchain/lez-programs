use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{
    account::AccountId,
    program::{ProgramHeader, ProgramId, PROGRAM_LOADER_ACCOUNT_ID},
};
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Map, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda,
};

use super::{
    parse_stablecoin_program_id, ProgramInfoRequest, StablecoinApiError, StablecoinResult,
};
use crate::account::{account_id_hex, decode_account, AccountRead};

/// Resolves the stablecoin program and its singleton PDAs.
///
/// Since LEZ v0.2.5 a program lives at the account its `program_loader` header was
/// deployed to, chosen at deploy time, so only `stablecoinProgramId` names it. An
/// optional `elf` is checked against the bytecode deployed there: its ImageID must
/// equal the `image_id` of the header in `programHeader`. The ImageID is reported as
/// `imageIdHex` (empty without a binary) and never used as an address.
pub fn program_info(request: ProgramInfoRequest) -> StablecoinResult {
    let program_id = request
        .stablecoin_program_id
        .as_deref()
        .map(parse_stablecoin_program_id)
        .transpose()?
        .ok_or_else(|| StablecoinApiError::new("config_missing"))?;

    let image_id = match request.elf.as_deref() {
        None => None,
        Some(elf) => {
            let image_id = image_id_from_binary(elf)?;
            let header = request
                .program_header
                .as_ref()
                .ok_or_else(|| StablecoinApiError::new("program_header_unavailable"))?;
            if deployed_image_id(header, program_id)? != image_id {
                return Err(StablecoinApiError::new("program_binary_mismatch"));
            }
            Some(image_id)
        }
    };

    let mut value = program_info_value(program_id);
    value["imageIdHex"] = json!(image_id.map(account_id_hex).unwrap_or_default());
    Ok(value)
}

/// A binary's ImageID, as the account-id bytes a `ProgramHeader` stores it in.
fn image_id_from_binary(value: &str) -> Result<AccountId, StablecoinApiError> {
    let bytes =
        hex::decode(value).map_err(|_| StablecoinApiError::new("invalid_program_binary"))?;
    let binary = ProgramBinary::decode(&bytes)
        .map_err(|_| StablecoinApiError::new("invalid_program_binary"))?;
    let image_id: ProgramId = binary
        .compute_image_id()
        .map(Into::into)
        .map_err(|_| StablecoinApiError::new("invalid_program_binary"))?;
    Ok(AccountId::from(image_id))
}

/// The ImageID of the bytecode deployed at `program_id`, from its header account.
fn deployed_image_id(
    header: &AccountRead,
    program_id: AccountId,
) -> Result<AccountId, StablecoinApiError> {
    let unavailable = || StablecoinApiError::new("program_header_unavailable");
    let (account_id, account) = decode_account(header).map_err(|_| unavailable())?;
    if account_id != program_id || account.program_owner != PROGRAM_LOADER_ACCOUNT_ID {
        return Err(unavailable());
    }
    let header = ProgramHeader::from_bytes(account.data.as_ref()).ok_or_else(unavailable)?;
    Ok(AccountId::from(header.image_id))
}

fn program_info_value(program_id: AccountId) -> Value {
    let mut result = Map::new();
    insert_id(&mut result, "programId", "programIdHex", program_id);
    insert_id(
        &mut result,
        "protocolParametersId",
        "protocolParametersIdHex",
        compute_protocol_parameters_pda(program_id),
    );
    insert_id(
        &mut result,
        "stabilityFeeAccumulatorId",
        "stabilityFeeAccumulatorIdHex",
        compute_stability_fee_accumulator_pda(program_id),
    );
    insert_id(
        &mut result,
        "redemptionPriceStateId",
        "redemptionPriceStateIdHex",
        compute_redemption_price_state_pda(program_id),
    );
    insert_id(
        &mut result,
        "stablecoinDefinitionId",
        "stablecoinDefinitionIdHex",
        compute_stablecoin_definition_pda(program_id),
    );
    insert_id(
        &mut result,
        "stablecoinMasterHoldingId",
        "stablecoinMasterHoldingIdHex",
        compute_stablecoin_master_holding_pda(program_id),
    );
    insert_id(
        &mut result,
        "clockId",
        "clockIdHex",
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    );
    Value::Object(result)
}

fn insert_id(
    result: &mut Map<String, Value>,
    base58_key: &str,
    hex_key: &str,
    account_id: AccountId,
) {
    result.insert(base58_key.to_owned(), json!(account_id.to_string()));
    result.insert(hex_key.to_owned(), json!(account_id_hex(account_id)));
}
