use serde_json::{json, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};

use super::{
    parse_stablecoin_program_id, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest, StablecoinApiError,
    StablecoinResult,
};
use crate::account::{account_id_hex, decode_account};

pub fn decode_protocol_parameters(request: DecodeProtocolParametersRequest) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let (account_id, account) = decode_account(&request.protocol_parameters)
        .map_err(|_| StablecoinApiError::new("account_read_failed"))?;

    if account_id != compute_protocol_parameters_pda(stablecoin_program_id) {
        return Err(StablecoinApiError::new("protocol_parameters_pda_mismatch"));
    }
    if account.program_owner != stablecoin_program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }

    let parameters = ProtocolParameters::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_protocol_parameters_data"))?;
    Ok(parameters_value(account_id, &parameters))
}

pub fn decode_stability_fee_accumulator(
    request: DecodeStabilityFeeAccumulatorRequest,
) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let (account_id, account) = decode_account(&request.stability_fee_accumulator)
        .map_err(|_| StablecoinApiError::new("account_read_failed"))?;

    if account_id != compute_stability_fee_accumulator_pda(stablecoin_program_id) {
        return Err(StablecoinApiError::new(
            "stability_fee_accumulator_pda_mismatch",
        ));
    }
    if account.program_owner != stablecoin_program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }

    let accumulator = StabilityFeeAccumulator::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_stability_fee_accumulator_data"))?;
    Ok(stability_fee_accumulator_value(account_id, &accumulator))
}

pub fn decode_redemption_price_state(
    request: DecodeRedemptionPriceStateRequest,
) -> StablecoinResult {
    let stablecoin_program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let (account_id, account) = decode_account(&request.redemption_price_state)
        .map_err(|_| StablecoinApiError::new("account_read_failed"))?;

    if account_id != compute_redemption_price_state_pda(stablecoin_program_id) {
        return Err(StablecoinApiError::new(
            "redemption_price_state_pda_mismatch",
        ));
    }
    if account.program_owner != stablecoin_program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }

    let state = RedemptionPriceState::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_redemption_price_state_data"))?;
    Ok(redemption_price_state_value(account_id, &state))
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

fn stability_fee_accumulator_value(
    account_id: lee_core::account::AccountId,
    accumulator: &StabilityFeeAccumulator,
) -> Value {
    json!({
        "accountId": account_id.to_string(),
        "accountIdHex": account_id_hex(account_id),
        "accumulatedRateAtLastAccrual":
            accumulator.accumulated_rate_at_last_accrual.to_string(),
        "lastAccruedAt": accumulator.last_accrued_at.to_string(),
    })
}

fn redemption_price_state_value(
    account_id: lee_core::account::AccountId,
    state: &RedemptionPriceState,
) -> Value {
    json!({
        "accountId": account_id.to_string(),
        "accountIdHex": account_id_hex(account_id),
        "redemptionPriceAtLastUpdate": state.redemption_price_at_last_update.to_string(),
        "redemptionRatePerMillisecond": state.redemption_rate_per_millisecond.to_string(),
        "controllerIntegralTerm": state.controller_integral_term.to_string(),
        "lastUpdatedAt": state.last_updated_at.to_string(),
    })
}
