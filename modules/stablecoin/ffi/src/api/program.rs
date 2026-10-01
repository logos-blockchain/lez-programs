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
use crate::account::account_id_hex;

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

/// The account id a binary's `ProgramHeader` would have **if** deployed at the
/// image-id bijection address.
///
/// Since LEZ v0.2.5 a binary does not determine where its program lives:
/// `CreateHeader` writes the header into whatever undeployed account the deployer
/// names. This is therefore a guess, correct only under that convention. When a
/// caller supplies `stablecoinProgramId` as well, that is the authority; a
/// disagreement means the header was deployed somewhere other than its bijection
/// address and is reported as `program_id_mismatch`.
fn program_id_from_binary(value: &str) -> Result<AccountId, StablecoinApiError> {
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
