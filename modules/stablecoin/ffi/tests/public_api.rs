use stablecoin_ffi::{
    decode_position, decode_protocol_parameters, decode_stability_fee_accumulator,
    initialize_program_plan, position_info, program_info, DecodePositionRequest,
    DecodeProtocolParametersRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, PositionInfoRequest, ProgramInfoRequest, StablecoinResult,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
    let _program_info: fn(ProgramInfoRequest) -> StablecoinResult = program_info;
    let _decode: fn(DecodeProtocolParametersRequest) -> StablecoinResult =
        decode_protocol_parameters;
    let _decode_accumulator: fn(DecodeStabilityFeeAccumulatorRequest) -> StablecoinResult =
        decode_stability_fee_accumulator;
    let _position_info: fn(PositionInfoRequest) -> StablecoinResult = position_info;
    let _decode_position: fn(DecodePositionRequest) -> StablecoinResult = decode_position;
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
}
