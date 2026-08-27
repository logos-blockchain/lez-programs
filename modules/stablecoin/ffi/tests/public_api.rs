use stablecoin_ffi::{
    accrue_stability_fee_plan, current_global_state, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, initialize_program_plan,
    program_info, redemption_rate_update_quote, refresh_globals_plan, update_redemption_rate_plan,
    AccrueStabilityFeePlanRequest, CurrentGlobalStateRequest, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, ProgramInfoRequest, RedemptionRateUpdateQuoteRequest,
    RefreshGlobalsPlanRequest, StablecoinResult, UpdateRedemptionRatePlanRequest,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
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
}
