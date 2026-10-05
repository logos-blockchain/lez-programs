#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use account::{AccountRead, WalletAccount};
pub use api::{
    decode_position, decode_protocol_parameters, decode_stability_fee_accumulator,
    initialize_program_plan, position_info, program_info, DecodePositionRequest,
    DecodeProtocolParametersRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, PositionInfoRequest, ProgramInfoRequest, StablecoinApiError,
    StablecoinResponse, StablecoinResult,
};
