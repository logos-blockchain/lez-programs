use stablecoin_ffi::{
    decode_position, decode_protocol_parameters, initialize_program_plan, position_info,
    program_info, DecodePositionRequest, DecodeProtocolParametersRequest,
    InitializeProgramPlanRequest, PositionInfoRequest, ProgramInfoRequest, StablecoinResult,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
    let _program_info: fn(ProgramInfoRequest) -> StablecoinResult = program_info;
    let _decode: fn(DecodeProtocolParametersRequest) -> StablecoinResult =
        decode_protocol_parameters;
    let _position_info: fn(PositionInfoRequest) -> StablecoinResult = position_info;
    let _decode_position: fn(DecodePositionRequest) -> StablecoinResult = decode_position;
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
}
