use serde::Deserialize;
use serde_json::Value;

use crate::AccountRead;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgramInfoRequest {
    #[serde(default)]
    pub stablecoin_program_id: Option<String>,
    #[serde(default)]
    pub elf: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeProtocolParametersRequest {
    pub stablecoin_program_id: String,
    pub protocol_parameters: AccountRead,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeProgramPlanRequest {
    pub stablecoin_program_id: String,
    pub admin_id: String,
    pub freeze_authority_id: String,
    /// The Token Program the collateral lives under. Needed twice over: it says
    /// which shard of `collateralDefinition` to decode, and it is the value
    /// `initialize_program` records as `ProtocolParameters.token_program_id` and
    /// checks every later token slot against.
    pub token_program_id: String,
    /// The program that publishes `marketPriceOracle`. Same two roles: the shard
    /// to decode, and the producer the protocol binds.
    pub market_price_oracle_program_id: String,
    pub collateral_definition: AccountRead,
    pub market_price_oracle: AccountRead,
    pub clock: AccountRead,
    pub initial_stability_fee_per_millisecond: Value,
    pub initial_controller_proportional_gain: Value,
    pub initial_controller_integral_gain: Value,
    pub initial_minimum_collateralization_ratio: Value,
    pub minimum_milliseconds_between_rate_updates: Value,
    pub maximum_oracle_price_age_milliseconds: Value,
    pub initial_redemption_price: Value,
    pub stablecoin_name: String,
}
