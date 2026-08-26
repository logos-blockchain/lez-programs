use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{account::AccountId, program::ProgramId};
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
use crate::account::{account_id_hex, program_id_bytes};

pub fn program_info(request: ProgramInfoRequest) -> StablecoinResult {
    let configured = request
        .stablecoin_program_id
        .as_deref()
        .map(parse_stablecoin_program_id)
        .transpose()?;
    let derived = request
        .elf
        .as_deref()
        .map(program_id_from_binary)
        .transpose()?;

    let program_id = match (configured, derived) {
        (Some(configured), Some(derived)) if configured != derived => {
            return Err(StablecoinApiError::new("program_id_mismatch"));
        }
        (Some(program_id), _) | (_, Some(program_id)) => program_id,
        (None, None) => return Err(StablecoinApiError::new("config_missing")),
    };

    Ok(program_info_value(program_id))
}

fn program_id_from_binary(value: &str) -> Result<ProgramId, StablecoinApiError> {
    let bytes =
        hex::decode(value).map_err(|_| StablecoinApiError::new("invalid_program_binary"))?;
    let binary = ProgramBinary::decode(&bytes)
        .map_err(|_| StablecoinApiError::new("invalid_program_binary"))?;
    binary
        .compute_image_id()
        .map(Into::into)
        .map_err(|_| StablecoinApiError::new("invalid_program_binary"))
}

fn program_info_value(program_id: ProgramId) -> Value {
    let mut result = Map::new();
    let program_account_id = AccountId::new(program_id_bytes(program_id));
    insert_id(&mut result, "programId", "programIdHex", program_account_id);
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
