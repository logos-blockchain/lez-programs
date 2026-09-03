use stablecoin_ffi::{
    decode_protocol_parameters, decode_redemption_price_state, decode_stability_fee_accumulator,
    initialize_program_plan, program_info, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, ProgramInfoRequest, StablecoinResult,
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
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
}
