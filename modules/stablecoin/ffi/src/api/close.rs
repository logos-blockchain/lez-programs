use stablecoin_core::{compute_position_vault_pda, Instruction};
use token_core::TokenHolding;

use super::{
    decode::validated_protocol_parameters,
    parse_stablecoin_program_id,
    plan::{parse_account_id, parse_decimal_u64, plan_response, required_account},
    position::validated_position,
    ClosePositionPlanRequest, StablecoinApiError, StablecoinResult,
};

/// Plan closure of a fully settled position. Frozen protocols may still close;
/// the instruction clears Position data without releasing either account.
pub fn close_position_plan(request: ClosePositionPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let nonce = parse_decimal_u64(&request.position_nonce)?;
    let (position_id, position) = validated_position(program_id, owner, nonce, &request.position)?;
    let (parameters_id, _) =
        validated_protocol_parameters(program_id, &request.protocol_parameters)?;

    if position.normalized_debt_amount != 0 {
        return Err(StablecoinApiError::new("position_has_debt"));
    }
    if position.collateral_amount != 0 {
        return Err(StablecoinApiError::new("position_has_collateral"));
    }

    let expected_vault_id = compute_position_vault_pda(program_id, position_id);
    if position.vault_account_id != expected_vault_id {
        return Err(StablecoinApiError::new("position_vault_mismatch"));
    }
    let (vault_id, vault) = required_account(&request.vault)?;
    if vault_id != expected_vault_id {
        return Err(StablecoinApiError::new("vault_pda_mismatch"));
    }
    let holding = TokenHolding::try_from(&vault.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_vault"))?;
    let TokenHolding::Fungible { balance, .. } = holding else {
        return Err(StablecoinApiError::new("invalid_position_vault"));
    };
    if balance != 0 {
        return Err(StablecoinApiError::new("vault_not_empty"));
    }

    plan_response(
        program_id,
        [owner, position_id, vault_id, parameters_id],
        [true, false, false, false],
        Instruction::ClosePosition,
    )
}
