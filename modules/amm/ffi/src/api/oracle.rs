use amm_core::Instruction;
use lee_core::account::AccountId;
use serde_json::{json, Value};
use twap_oracle_core::{compute_oracle_price_account_pda, compute_price_observations_pda};

use super::{
    pair::derive_pair, CreateOraclePriceAccountPlanRequest, CreatePriceObservationsPlanRequest,
};
use crate::account::{account_id_from_hex, account_id_hex, parse_program_id};

/// The tx-submission envelope shared by the two oracle-setup plans: the fixed IDL account ids as
/// hex, their signer flags (nothing signs — both are chained calls into the TWAP oracle seeded
/// from validated pool state), and the risc0-encoded instruction words.
fn plan_response(program_id: &str, account_ids: &[AccountId], instruction: Vec<u32>) -> Value {
    let signing_requirements = vec![false; account_ids.len()];
    json!({
        "programId": program_id,
        "accountIds": account_ids.iter().copied().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    })
}

/// Resolves the pair's config / pool / current-tick / clock, returning them plus the TWAP oracle
/// program id (for the window-seeded PDAs). Shared prelude for the two oracle-setup plans.
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
/// for `window_duration_ms`. A chained call into the configured oracle — the feed's initial tick
/// is read on-chain from the pool's current-tick account, so nothing is caller-priced.
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
    if window < u64::from(twap_oracle_core::OBSERVATIONS_CAPACITY) {
        return Err(String::from("invalid_window"));
    }
    let price_observations =
        compute_price_observations_pda(pair.twap_oracle_program, pair.pool, window);

    let instruction = risc0_zkvm::serde::to_vec(&Instruction::CreatePriceObservations {
        window_duration: window,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order: config, pool (the price source), current_tick (supplies the initial
    // tick), price_observations (init), clock. Nothing signs.
    let account_ids = [
        pair.config,
        pair.pool,
        pair.current_tick,
        price_observations,
        pair.clock,
    ];
    Ok(plan_response(
        &request.amm_program_id,
        &account_ids,
        instruction,
    ))
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
    let oracle_price_account =
        compute_oracle_price_account_pda(pair.twap_oracle_program, pair.pool, window);

    let instruction = risc0_zkvm::serde::to_vec(&Instruction::CreateOraclePriceAccount {
        window_duration: window,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order: config, pool (the price source), oracle_price_account (init), clock.
    // Nothing signs.
    let account_ids = [pair.config, pair.pool, oracle_price_account, pair.clock];
    Ok(plan_response(
        &request.amm_program_id,
        &account_ids,
        instruction,
    ))
}
