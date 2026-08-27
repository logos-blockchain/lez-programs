#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use account::{AccountRead, WalletAccount};
pub use api::{
    current_global_state, decode_protocol_parameters, decode_redemption_price_state,
    decode_stability_fee_accumulator, initialize_program_plan, program_info,
    redemption_rate_update_quote, CurrentGlobalStateRequest, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest,
    InitializeProgramPlanRequest, ProgramInfoRequest, RedemptionRateUpdateQuoteRequest,
    StablecoinApiError, StablecoinResponse, StablecoinResult,
};
