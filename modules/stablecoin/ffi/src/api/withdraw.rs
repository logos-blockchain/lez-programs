use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use stablecoin_core::{
    collateralization::collateralization_values,
    compute_position_vault_pda,
    math::{compute_current_accumulated_rate_wide, compute_current_redemption_price_wide},
    Instruction,
};
use token_core::TokenHolding;

use super::{
    decode::{
        validated_protocol_parameters, validated_redemption_price_state,
        validated_stability_fee_accumulator,
    },
    parse_stablecoin_program_id,
    plan::{parse_account_id, parse_decimal_u64, parse_u128, plan_response, required_account},
    position::validated_position,
    projection::clock_timestamp,
    StablecoinApiError, StablecoinResult, WithdrawCollateralPlanRequest,
};

/// Validate and plan withdrawal using the native program's post-withdrawal health gate.
pub fn withdraw_collateral_plan(request: WithdrawCollateralPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let nonce = parse_decimal_u64(&request.position_nonce)?;
    let amount = parse_u128(&request.amount)?;
    let destination_id = parse_account_id(&request.user_collateral_holding_id)?;
    let (parameters_id, parameters) =
        validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    if parameters.is_frozen {
        return Err(StablecoinApiError::new("protocol_frozen"));
    }
    let (accumulator_id, accumulator) =
        validated_stability_fee_accumulator(program_id, &request.stability_fee_accumulator)?;
    let (redemption_id, redemption) =
        validated_redemption_price_state(program_id, &request.redemption_price_state)?;
    let now = clock_timestamp(&request.clock)?;
    let (position_id, position) = validated_position(program_id, owner, nonce, &request.position)?;
    let expected_vault = compute_position_vault_pda(program_id, position_id);
    if position.vault_account_id != expected_vault {
        return Err(StablecoinApiError::new("position_vault_mismatch"));
    }
    let (vault_id, vault_account) = required_account(&request.vault)?;
    if vault_id != expected_vault {
        return Err(StablecoinApiError::new("vault_pda_mismatch"));
    }
    // The runtime rejects duplicate public account IDs. These are the two
    // destination aliases compatible with the destination's token type.
    if destination_id == vault_id || destination_id == owner {
        return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
    }
    let vault_holding = TokenHolding::try_from(&vault_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_vault"))?;
    let vault_balance = match vault_holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } if definition_id == parameters.collateral_definition_id => balance,
        TokenHolding::Fungible { .. } => {
            return Err(StablecoinApiError::new("collateral_definition_mismatch"))
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            return Err(StablecoinApiError::new("invalid_position_vault"))
        }
    };
    let (read_destination_id, destination_account) =
        required_account(&request.user_collateral_holding)?;
    if read_destination_id != destination_id {
        return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
    }
    if destination_account.program_owner != vault_account.program_owner {
        return Err(StablecoinApiError::new("token_program_mismatch"));
    }
    let destination = TokenHolding::try_from(&destination_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_user_collateral_holding"))?;
    let destination_balance = match destination {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } if definition_id == parameters.collateral_definition_id => balance,
        TokenHolding::Fungible { .. } => {
            return Err(StablecoinApiError::new("collateral_definition_mismatch"))
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            return Err(StablecoinApiError::new("invalid_user_collateral_holding"))
        }
    };
    let remaining = position
        .collateral_amount
        .checked_sub(amount)
        .ok_or_else(|| StablecoinApiError::new("withdraw_amount_exceeds_collateral"))?;
    if vault_balance < amount {
        return Err(StablecoinApiError::new("insufficient_vault_balance"));
    }
    destination_balance
        .checked_add(amount)
        .ok_or_else(|| StablecoinApiError::new("collateral_amount_overflow"))?;
    if amount != 0 && position.normalized_debt_amount != 0 {
        let price = compute_current_redemption_price_wide(
            redemption.redemption_price_at_last_update,
            redemption.redemption_rate_per_millisecond,
            redemption.last_updated_at,
            now,
        );
        if price.is_zero() {
            return Err(StablecoinApiError::new("redemption_price_zero"));
        }
        let accumulator = compute_current_accumulated_rate_wide(
            accumulator.accumulated_rate_at_last_accrual,
            parameters.stability_fee_per_millisecond,
            accumulator.last_accrued_at,
            now,
        );
        if !collateralization_values(
            remaining,
            position.normalized_debt_amount,
            accumulator,
            price,
            parameters.minimum_collateralization_ratio,
        )
        .is_collateralized()
        {
            return Err(StablecoinApiError::new("position_undercollateralized"));
        }
    }
    plan_response(
        program_id,
        [
            owner,
            position_id,
            vault_id,
            destination_id,
            accumulator_id,
            redemption_id,
            parameters_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        [true, false, false, false, false, false, false, false],
        Instruction::WithdrawCollateral { amount },
    )
}
