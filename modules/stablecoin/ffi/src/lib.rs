#![deny(unsafe_op_in_unsafe_fn)]

mod account;
mod ffi;

pub mod api;

pub use account::{AccountRead, WalletAccount};
pub use api::{
    accrue_stability_fee_plan, current_global_state, decode_protocol_parameters,
    decode_redemption_price_state, decode_stability_fee_accumulator, deposit_collateral_plan,
    generate_debt_plan, initialize_program_plan, open_position_plan, position_addresses,
    position_health, program_info, redemption_rate_update_quote, refresh_globals_plan,
    repay_debt_plan, update_redemption_rate_plan, withdraw_collateral_plan,
    AccrueStabilityFeePlanRequest, CurrentGlobalStateRequest, DecodeProtocolParametersRequest,
    DecodeRedemptionPriceStateRequest, DecodeStabilityFeeAccumulatorRequest,
    DepositCollateralPlanRequest, GenerateDebtPlanRequest, InitializeProgramPlanRequest,
    OpenPositionPlanRequest, PositionAddressesRequest, PositionHealthRequest, ProgramInfoRequest,
    RedemptionRateUpdateQuoteRequest, RefreshGlobalsPlanRequest, RepayDebtPlanRequest,
    StablecoinApiError, StablecoinResponse, StablecoinResult, UpdateRedemptionRatePlanRequest,
    WithdrawCollateralPlanRequest,
};
