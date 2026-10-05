//! Transport-independent stablecoin client operations.

mod admin;
mod close;
mod debt;
mod decode;
mod freeze;
mod health;
mod plan;
mod position;
mod program;
mod projection;
mod quote;
mod request;
mod withdraw;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod journeys;

#[cfg(test)]
mod repay_tests;

#[cfg(test)]
mod health_tests;

#[cfg(test)]
mod withdraw_tests;

#[cfg(test)]
mod debt_tests;

#[cfg(test)]
mod close_tests;

#[cfg(test)]
mod admin_tests;

#[cfg(test)]
mod freeze_tests;

use std::{error::Error, fmt};

pub use admin::{
    set_admin_plan, set_controller_gains_plan, set_freeze_authority_plan,
    set_market_price_oracle_plan, set_minimum_collateralization_ratio_plan,
    set_timing_parameters_plan,
};
pub use close::close_position_plan;
pub use debt::generate_debt_plan;
pub use decode::{
    decode_protocol_parameters, decode_redemption_price_state, decode_stability_fee_accumulator,
};
pub use freeze::{freeze_plan, unfreeze_plan};
pub use health::position_health;
pub use plan::{
    accrue_stability_fee_plan, deposit_collateral_plan, initialize_program_plan,
    open_position_plan, position_addresses, refresh_globals_plan, repay_debt_plan,
    update_redemption_rate_plan,
};
pub use program::program_info;
pub use projection::current_global_state;
pub use quote::redemption_rate_update_quote;
pub use request::{
    AccrueStabilityFeePlanRequest, AdminPlanContext, ClosePositionPlanRequest,
    CurrentGlobalStateRequest, DecodeProtocolParametersRequest, DecodeRedemptionPriceStateRequest,
    DecodeStabilityFeeAccumulatorRequest, DepositCollateralPlanRequest, FreezeAuthorityPlanRequest,
    GenerateDebtPlanRequest, InitializeProgramPlanRequest, OpenPositionPlanRequest,
    PositionAddressesRequest, PositionHealthRequest, ProgramInfoRequest,
    RedemptionRateUpdateQuoteRequest, RefreshGlobalsPlanRequest, RepayDebtPlanRequest,
    SetAdminPlanRequest, SetControllerGainsPlanRequest, SetFreezeAuthorityPlanRequest,
    SetMarketPriceOraclePlanRequest, SetMinimumCollateralizationRatioPlanRequest,
    SetTimingParametersPlanRequest, UpdateRedemptionRatePlanRequest, WithdrawCollateralPlanRequest,
};
use serde_json::Value;
pub use withdraw::withdraw_collateral_plan;

use crate::account::parse_program_id;

pub type StablecoinResponse = Value;
pub type StablecoinResult = Result<StablecoinResponse, StablecoinApiError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StablecoinApiError {
    code: &'static str,
}

impl StablecoinApiError {
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for StablecoinApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl Error for StablecoinApiError {}

fn parse_stablecoin_program_id(
    value: &str,
) -> Result<lee_core::program::ProgramId, StablecoinApiError> {
    let program_id =
        parse_program_id(value).map_err(|_| StablecoinApiError::new("invalid_program_id"))?;
    if program_id == [0_u32; 8] {
        return Err(StablecoinApiError::new("invalid_program_id"));
    }
    Ok(program_id)
}
