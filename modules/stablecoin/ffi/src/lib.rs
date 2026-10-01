#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use account::{AccountRead, WalletAccount};
pub use api::{
    accrue_stability_fee_plan, current_global_state, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, deposit_collateral_plan,
    initialize_program_plan, open_position_plan, position_addresses, program_info,
    redemption_rate_update_quote, refresh_globals_plan, update_redemption_rate_plan,
    AccrueStabilityFeePlanRequest, CurrentGlobalStateRequest, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest,
    DepositCollateralPlanRequest, InitializeProgramPlanRequest, OpenPositionPlanRequest,
    PositionAddressesRequest, ProgramInfoRequest, RedemptionRateUpdateQuoteRequest,
    RefreshGlobalsPlanRequest, StablecoinApiError, StablecoinResponse, StablecoinResult,
    UpdateRedemptionRatePlanRequest,
};
