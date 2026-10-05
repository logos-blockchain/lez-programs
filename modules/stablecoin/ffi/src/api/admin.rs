use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{math::FIXED_POINT_ONE, Instruction, ProtocolParameters};
use twap_oracle_core::OraclePriceAccount;

use super::{
    decode::validated_protocol_parameters,
    parse_stablecoin_program_id,
    plan::{
        parse_account_id, parse_account_id_value, parse_i128, parse_u128, parse_u64, plan_response,
        required_account,
    },
    AdminPlanContext, SetAdminPlanRequest, SetControllerGainsPlanRequest,
    SetFreezeAuthorityPlanRequest, SetMarketPriceOraclePlanRequest,
    SetMinimumCollateralizationRatioPlanRequest, SetTimingParametersPlanRequest,
    StablecoinApiError, StablecoinResult,
};

struct ValidatedAdmin {
    program_id: ProgramId,
    admin: AccountId,
    parameters_id: AccountId,
    parameters: ProtocolParameters,
}

impl ValidatedAdmin {
    fn read(context: &AdminPlanContext) -> Result<Self, StablecoinApiError> {
        let program_id = parse_stablecoin_program_id(&context.stablecoin_program_id)?;
        let admin = parse_account_id(&context.admin_id)?;
        let (parameters_id, parameters) =
            validated_protocol_parameters(program_id, &context.protocol_parameters)?;
        if admin != parameters.admin_account_id {
            return Err(StablecoinApiError::new("admin_mismatch"));
        }
        Ok(Self {
            program_id,
            admin,
            parameters_id,
            parameters,
        })
    }

    fn plan(self, instruction: Instruction) -> StablecoinResult {
        plan_response(
            self.program_id,
            [self.admin, self.parameters_id],
            [true, false],
            instruction,
        )
    }
}

/// Plan a ratio update by the currently stored admin, including while frozen.
pub fn set_minimum_collateralization_ratio_plan(
    request: SetMinimumCollateralizationRatioPlanRequest,
) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_ratio = parse_u128(&request.new_ratio)?;
    if !(FIXED_POINT_ONE * 110 / 100..=FIXED_POINT_ONE * 10).contains(&new_ratio) {
        return Err(StablecoinApiError::new(
            "collateralization_ratio_out_of_band",
        ));
    }
    context.plan(Instruction::SetMinimumCollateralizationRatio { new_ratio })
}

/// Plan both signed gains in one instruction without resetting redemption state.
pub fn set_controller_gains_plan(request: SetControllerGainsPlanRequest) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_proportional_gain = parse_i128(&request.new_proportional_gain)?;
    let new_integral_gain = parse_i128(&request.new_integral_gain)?;
    if new_proportional_gain.unsigned_abs() > FIXED_POINT_ONE * 1_000
        || new_integral_gain.unsigned_abs() > FIXED_POINT_ONE
    {
        return Err(StablecoinApiError::new("controller_gains_out_of_band"));
    }
    context.plan(Instruction::SetControllerGains {
        new_proportional_gain,
        new_integral_gain,
    })
}

/// Plan both timing fields in one instruction, with inclusive native limits.
pub fn set_timing_parameters_plan(request: SetTimingParametersPlanRequest) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_minimum_milliseconds_between_rate_updates =
        parse_u64(&request.new_minimum_milliseconds_between_rate_updates)?;
    let new_maximum_oracle_price_age_milliseconds =
        parse_u64(&request.new_maximum_oracle_price_age_milliseconds)?;
    if !(1..=86_400_000).contains(&new_minimum_milliseconds_between_rate_updates)
        || !(1..=86_400_000).contains(&new_maximum_oracle_price_age_milliseconds)
    {
        return Err(StablecoinApiError::new("timing_parameters_out_of_band"));
    }
    context.plan(Instruction::SetTimingParameters {
        new_minimum_milliseconds_between_rate_updates,
        new_maximum_oracle_price_age_milliseconds,
    })
}

/// Plan immediate one-step admin rotation. The new handle is not a signer.
pub fn set_admin_plan(request: SetAdminPlanRequest) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_admin_account_id = parse_account_id_value(&request.new_admin_id)?;
    context.plan(Instruction::SetAdmin {
        new_admin_account_id,
    })
}

/// Plan freeze-authority rotation authorized by the admin, not the old authority.
pub fn set_freeze_authority_plan(request: SetFreezeAuthorityPlanRequest) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_freeze_authority_account_id = parse_account_id_value(&request.new_freeze_authority_id)?;
    context.plan(Instruction::SetFreezeAuthority {
        new_freeze_authority_account_id,
    })
}

/// Plan oracle replacement by the admin. Producer identity, price, observation
/// age and the frozen flag are intentionally not gates for this native setter.
pub fn set_market_price_oracle_plan(request: SetMarketPriceOraclePlanRequest) -> StablecoinResult {
    let context = ValidatedAdmin::read(&request.context)?;
    let new_oracle_id = parse_account_id(&request.new_oracle_id)?;
    let (read_id, account) = required_account(&request.new_oracle)?;
    if read_id != new_oracle_id {
        return Err(StablecoinApiError::new("market_price_oracle_mismatch"));
    }
    let oracle = OraclePriceAccount::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_market_price_oracle"))?;
    if oracle.base_asset != context.parameters.stablecoin_definition_id
        || oracle.quote_asset != context.parameters.collateral_definition_id
    {
        return Err(StablecoinApiError::new("oracle_asset_mismatch"));
    }
    // A public oracle can also be a wallet-controlled admin handle. Passing it
    // twice would violate the runtime's distinct-account requirement.
    if new_oracle_id == context.admin {
        return Err(StablecoinApiError::new("invalid_market_price_oracle"));
    }
    plan_response(
        context.program_id,
        [context.admin, context.parameters_id, new_oracle_id],
        [true, false, false],
        Instruction::SetMarketPriceOracle,
    )
}
