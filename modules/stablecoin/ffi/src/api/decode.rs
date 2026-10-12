use serde_json::{json, Value};
use stablecoin_core::{compute_protocol_parameters_pda, ProtocolParameters};

use super::{
    parse_stablecoin_program_id, DecodeProtocolParametersRequest, StablecoinApiError,
    StablecoinResult,
};
use crate::account::{account_id_hex, decode_shard, AccountReadError};

pub fn decode_protocol_parameters(request: DecodeProtocolParametersRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    // `stablecoin_program_mismatch` survives v0.2.5's `program_owner !=
    // stablecoin_program_id` with the same meaning: a shard is keyed by the
    // program that wrote it, so asking for this program's shard and finding none
    // is the same claim as finding the account owned by somebody else.
    let (account_id, shard) = decode_shard(&request.protocol_parameters, stablecoin_program_id)
        .map_err(|error| match error {
            AccountReadError::Failed | AccountReadError::Malformed => {
                StablecoinApiError::new("account_read_failed")
            }
            AccountReadError::NoShard => StablecoinApiError::new("stablecoin_program_mismatch"),
        })?;

    if account_id != compute_protocol_parameters_pda(stablecoin_program_id) {
        return Err(StablecoinApiError::new("protocol_parameters_pda_mismatch"));
    }

    let parameters = ProtocolParameters::try_from(&shard)
        .map_err(|_| StablecoinApiError::new("invalid_protocol_parameters_data"))?;
    Ok(parameters_value(account_id, &parameters))
}

fn parameters_value(
    account_id: lee_core::account::AccountId,
    parameters: &ProtocolParameters,
) -> Value {
    json!({
        "accountId": account_id.to_string(),
        "accountIdHex": account_id_hex(account_id),
        "adminId": parameters.admin_account_id.to_string(),
        "adminIdHex": account_id_hex(parameters.admin_account_id),
        "freezeAuthorityId": parameters.freeze_authority_account_id.to_string(),
        "freezeAuthorityIdHex": account_id_hex(parameters.freeze_authority_account_id),
        "stablecoinDefinitionId": parameters.stablecoin_definition_id.to_string(),
        "stablecoinDefinitionIdHex": account_id_hex(parameters.stablecoin_definition_id),
        "collateralDefinitionId": parameters.collateral_definition_id.to_string(),
        "collateralDefinitionIdHex": account_id_hex(parameters.collateral_definition_id),
        "marketPriceOracleId": parameters.market_price_oracle_id.to_string(),
        "marketPriceOracleIdHex": account_id_hex(parameters.market_price_oracle_id),
        // New in v0.3.0: the protocol binds not just *which* accounts it trusts
        // but whose shards they are, so a caller cannot route a burn or an
        // observation through a program of their own choosing.
        "tokenProgramId": parameters.token_program_id.to_string(),
        "tokenProgramIdHex": account_id_hex(parameters.token_program_id),
        "marketPriceOracleProgramId": parameters.market_price_oracle_program_id.to_string(),
        "marketPriceOracleProgramIdHex":
            account_id_hex(parameters.market_price_oracle_program_id),
        "stabilityFeePerMillisecond": parameters.stability_fee_per_millisecond.to_string(),
        "controllerProportionalGain": parameters.controller_proportional_gain.to_string(),
        "controllerIntegralGain": parameters.controller_integral_gain.to_string(),
        "minimumCollateralizationRatio": parameters.minimum_collateralization_ratio.to_string(),
        "minimumMillisecondsBetweenRateUpdates":
            parameters.minimum_milliseconds_between_rate_updates.to_string(),
        "maximumOraclePriceAgeMilliseconds":
            parameters.maximum_oracle_price_age_milliseconds.to_string(),
        "isFrozen": parameters.is_frozen,
    })
}
