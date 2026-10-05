use alloy_primitives::U512;
use serde_json::json;
use stablecoin_core::{
    collateralization::collateralization_values,
    compute_position_vault_pda,
    math::{try_project_rate_wide, FIXED_POINT_ONE},
};

use super::{
    decode::{
        validated_protocol_parameters, validated_redemption_price_state,
        validated_stability_fee_accumulator,
    },
    parse_stablecoin_program_id,
    plan::{parse_account_id, parse_decimal_u64},
    position::validated_position,
    projection::clock_timestamp,
    PositionHealthRequest, StablecoinApiError, StablecoinResult,
};
use crate::account::account_id_hex;

/// Quote stored position health using the program's fractional-debt comparison.
pub fn position_health(request: PositionHealthRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let nonce = parse_decimal_u64(&request.position_nonce)?;
    let (position_id, position) = validated_position(program_id, owner, nonce, &request.position)?;
    let vault_id = compute_position_vault_pda(program_id, position_id);
    if position.vault_account_id != vault_id {
        return Err(StablecoinApiError::new("position_vault_mismatch"));
    }
    let (_, parameters) = validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    let (_, accumulator) =
        validated_stability_fee_accumulator(program_id, &request.stability_fee_accumulator)?;
    let (_, redemption) =
        validated_redemption_price_state(program_id, &request.redemption_price_state)?;
    let now = clock_timestamp(&request.clock)?;
    let current_accumulator = try_project_rate_wide(
        accumulator.accumulated_rate_at_last_accrual,
        parameters.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    )
    .ok_or_else(|| StablecoinApiError::new("health_projection_overflow"))?;
    let current_redemption_price = try_project_rate_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    )
    .ok_or_else(|| StablecoinApiError::new("health_projection_overflow"))?;
    let nominal_debt = U512::from(position.normalized_debt_amount)
        .checked_mul(current_accumulator)
        .ok_or_else(|| StablecoinApiError::new("health_arithmetic_overflow"))?
        / U512::from(FIXED_POINT_ONE);
    let values = collateralization_values(
        position.collateral_amount,
        position.normalized_debt_amount,
        current_accumulator,
        current_redemption_price,
        parameters.minimum_collateralization_ratio,
    );
    Ok(json!({
        "ownerId": owner.to_string(), "ownerIdHex": account_id_hex(owner),
        "positionNonce": nonce.to_string(),
        "positionId": position_id.to_string(), "positionIdHex": account_id_hex(position_id),
        "vaultId": vault_id.to_string(), "vaultIdHex": account_id_hex(vault_id),
        "collateralAmount": position.collateral_amount.to_string(),
        "normalizedDebtAmount": position.normalized_debt_amount.to_string(),
        "openedAt": position.opened_at.to_string(),
        "currentAccumulatedRate": current_accumulator.to_string(),
        "currentRedemptionPrice": current_redemption_price.to_string(),
        "minimumCollateralizationRatio": parameters.minimum_collateralization_ratio.to_string(),
        "projectedAt": now.to_string(),
        "nominalDebt": nominal_debt.to_string(),
        "collateralValue": values.collateral_value.to_string(),
        "requiredCollateralValue": values.required_collateral_value.to_string(),
        "requirementSaturated": values.requirement_saturated,
        "isCollateralized": values.is_collateralized(),
    }))
}
