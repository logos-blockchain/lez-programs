#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use account::{AccountRead, WalletAccount};
pub use api::{
    decode_protocol_parameters, initialize_program_plan, program_info,
    DecodeProtocolParametersRequest, InitializeProgramPlanRequest, ProgramInfoRequest,
    StablecoinApiError, StablecoinResponse, StablecoinResult,
};
