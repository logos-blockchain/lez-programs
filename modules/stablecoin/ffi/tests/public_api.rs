use stablecoin_ffi::{
    decode_protocol_parameters, initialize_program_plan, program_info,
    DecodeProtocolParametersRequest, InitializeProgramPlanRequest, ProgramInfoRequest,
    StablecoinResult,
};

#[test]
fn crate_root_reexports_stablecoin_surface() {
    let _program_info: fn(ProgramInfoRequest) -> StablecoinResult = program_info;
    let _decode: fn(DecodeProtocolParametersRequest) -> StablecoinResult =
        decode_protocol_parameters;
    let _initialize: fn(InitializeProgramPlanRequest) -> StablecoinResult = initialize_program_plan;
}
