use stablecoin_core::Instruction;

use super::{
    decode::validated_protocol_parameters,
    parse_stablecoin_program_id,
    plan::{parse_account_id, plan_response},
    FreezeAuthorityPlanRequest, StablecoinApiError, StablecoinResult,
};

/// Plan an authorized freeze, including an already-frozen protocol.
pub fn freeze_plan(request: FreezeAuthorityPlanRequest) -> StablecoinResult {
    freeze_authority_plan(request, Instruction::Freeze)
}

/// Plan an authorized unfreeze, including an already-unfrozen protocol.
pub fn unfreeze_plan(request: FreezeAuthorityPlanRequest) -> StablecoinResult {
    freeze_authority_plan(request, Instruction::Unfreeze)
}

fn freeze_authority_plan(
    request: FreezeAuthorityPlanRequest,
    instruction: Instruction,
) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let authority = parse_account_id(&request.freeze_authority_id)?;
    let (parameters_id, parameters) =
        validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    if authority != parameters.freeze_authority_account_id {
        return Err(StablecoinApiError::new("freeze_authority_mismatch"));
    }
    plan_response(
        program_id,
        [authority, parameters_id],
        [true, false],
        instruction,
    )
}
