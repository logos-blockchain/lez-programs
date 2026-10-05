use stablecoin_ffi::{
    accrue_stability_fee_plan, current_global_state, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, deposit_collateral_plan,
    initialize_program_plan, open_position_plan, position_addresses, program_info,
    redemption_rate_update_quote, refresh_globals_plan, repay_debt_plan,
    update_redemption_rate_plan, AccrueStabilityFeePlanRequest, CurrentGlobalStateRequest,
    DecodeProtocolParametersRequest, DecodeRedemptionPriceStateRequest,
    DecodeStabilityFeeAccumulatorRequest, DepositCollateralPlanRequest,
    InitializeProgramPlanRequest, OpenPositionPlanRequest, PositionAddressesRequest,
    ProgramInfoRequest, RedemptionRateUpdateQuoteRequest, RefreshGlobalsPlanRequest,
    RepayDebtPlanRequest, StablecoinResult, UpdateRedemptionRatePlanRequest,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
    let _borrow: fn(stablecoin_ffi::GenerateDebtPlanRequest) -> StablecoinResult =
        stablecoin_ffi::generate_debt_plan;
    let _withdraw: fn(stablecoin_ffi::WithdrawCollateralPlanRequest) -> StablecoinResult =
        stablecoin_ffi::withdraw_collateral_plan;
    let _health: fn(stablecoin_ffi::PositionHealthRequest) -> StablecoinResult =
        stablecoin_ffi::position_health;
    let _program_info: fn(ProgramInfoRequest) -> StablecoinResult = program_info;
    let _decode: fn(DecodeProtocolParametersRequest) -> StablecoinResult =
        decode_protocol_parameters;
    let _decode_accumulator: fn(DecodeStabilityFeeAccumulatorRequest) -> StablecoinResult =
        decode_stability_fee_accumulator;
    let _decode_redemption_state: fn(DecodeRedemptionPriceStateRequest) -> StablecoinResult =
        decode_redemption_price_state;
    let _current_global_state: fn(CurrentGlobalStateRequest) -> StablecoinResult =
        current_global_state;
    let _redemption_rate_quote: fn(RedemptionRateUpdateQuoteRequest) -> StablecoinResult =
        redemption_rate_update_quote;
    let _accrue: fn(AccrueStabilityFeePlanRequest) -> StablecoinResult = accrue_stability_fee_plan;
    let _update: fn(UpdateRedemptionRatePlanRequest) -> StablecoinResult =
        update_redemption_rate_plan;
    let _refresh: fn(RefreshGlobalsPlanRequest) -> StablecoinResult = refresh_globals_plan;
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
    let _open_position: fn(OpenPositionPlanRequest) -> StablecoinResult = open_position_plan;
    let _position_addresses: fn(PositionAddressesRequest) -> StablecoinResult = position_addresses;
    let _deposit: fn(DepositCollateralPlanRequest) -> StablecoinResult = deposit_collateral_plan;
    let _repay: fn(RepayDebtPlanRequest) -> StablecoinResult = repay_debt_plan;
}
