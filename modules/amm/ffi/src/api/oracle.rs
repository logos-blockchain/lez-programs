use serde_json::Value;

use super::{
    pair::derive_pair, CreateOraclePriceAccountPlanRequest, CreatePriceObservationsPlanRequest,
};
use crate::account::{account_id_from_hex, parse_program_id};

fn checked_window(window: u64) -> Result<u64, String> {
    if window < u64::from(twap_oracle_core::OBSERVATIONS_CAPACITY) {
        return Err("invalid_window".into());
    }
    Ok(window)
}

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

/// Builds a request to open the observations and price actors for a window.
pub(super) fn create_price_observations_plan(
    request: CreatePriceObservationsPlanRequest,
) -> Result<Value, String> {
    let pair = resolve(
        &request.amm_program_id,
        &request.token_a_id,
        &request.token_b_id,
        &request.config,
    )?;
    let message = amm_core::Message::CreatePriceObservations {
        window_duration: checked_window(request.window_duration_ms)?,
    };

    super::plan::pool_plan(&request.amm_program_id, pair, message)
}

/// Alternate AMM entrypoint for opening the same pair of window actors.
pub(super) fn create_oracle_price_account_plan(
    request: CreateOraclePriceAccountPlanRequest,
) -> Result<Value, String> {
    let pair = resolve(
        &request.amm_program_id,
        &request.token_a_id,
        &request.token_b_id,
        &request.config,
    )?;
    let message = amm_core::Message::CreateOraclePriceAccount {
        window_duration: checked_window(request.window_duration_ms)?,
    };

    super::plan::pool_plan(&request.amm_program_id, pair, message)
}
