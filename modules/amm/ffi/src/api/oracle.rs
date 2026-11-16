use serde_json::Value;

use super::{
    pair::derive_pair, CreateOraclePriceAccountPlanRequest, CreatePriceObservationsPlanRequest,
};
use crate::account::{account_id_from_hex, parse_program_id};

/// Resolves the config, pool and Oracle deployment for setup messages.
fn resolve(
    request_amm_program_id: &str,
    token_a_id: &str,
    token_b_id: &str,
    config: &crate::account::AccountRead,
) -> Result<super::pair::PairIds, String> {
    let amm_program = parse_program_id(request_amm_program_id)?;
    let token_a = account_id_from_hex(token_a_id, "token A id")?;
    let token_b = account_id_from_hex(token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    derive_pair(amm_program, token_a, token_b, config)
        .map_err(|_| String::from("config_unavailable"))
}

/// Builds the `CreatePriceObservations` submission: creates the pool's TWAP observations account
/// for `window_duration_ms`. The AMM supplies prices from its pool state.
pub(super) fn create_price_observations_plan(
    request: CreatePriceObservationsPlanRequest,
) -> Result<Value, String> {
    let pair = resolve(
        &request.amm_program_id,
        &request.token_a_id,
        &request.token_b_id,
        &request.config,
    )?;
    let window = request.window_duration_ms;
    if window < amm_core::oracle::MIN_WINDOW_DURATION {
        return Err("invalid_window".into());
    }
    let message = amm_core::Message::CreatePriceObservations {
        window_duration: window,
    };

    super::plan::pool_plan(&request.amm_program_id, pair, message)
}

/// Builds the `CreateOraclePriceAccount` submission: creates the pool's TWAP oracle price account
/// for `window_duration_ms`. A chained call into the configured oracle — no caller pricing.
pub(super) fn create_oracle_price_account_plan(
    request: CreateOraclePriceAccountPlanRequest,
) -> Result<Value, String> {
    let pair = resolve(
        &request.amm_program_id,
        &request.token_a_id,
        &request.token_b_id,
        &request.config,
    )?;
    let window = request.window_duration_ms;

    let message = amm_core::Message::CreateOraclePriceAccount {
        window_duration: window,
    };

    super::plan::pool_plan(&request.amm_program_id, pair, message)
}
