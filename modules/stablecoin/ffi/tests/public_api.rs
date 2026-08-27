use stablecoin_ffi::{
    decode_protocol_parameters, decode_stability_fee_accumulator, initialize_program_plan,
    program_info, DecodeProtocolParametersRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, ProgramInfoRequest, StablecoinResult,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
    let _program_info: fn(ProgramInfoRequest) -> StablecoinResult = program_info;
    let _decode: fn(DecodeProtocolParametersRequest) -> StablecoinResult =
        decode_protocol_parameters;
    let _decode_accumulator: fn(DecodeStabilityFeeAccumulatorRequest) -> StablecoinResult =
        decode_stability_fee_accumulator;
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
}
