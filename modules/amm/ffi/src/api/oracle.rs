use amm_core::Instruction;
use clock_core::clock_account_id;
use lee_core::account::AccountId;
use serde_json::Value;
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, CurrentTickAccount,
};

use super::{
    clock::now_from_clock,
    envelope::{identity_of, plan_response, pool_from_hex, slot},
    pair::derive_pair,
    CreateOraclePriceAccountPlanRequest, CreatePriceObservationsPlanRequest,
};
use crate::account::{account_id_from_hex, parse_program_id, program_shard, AccountRead};

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
/// is still the oracle's own authoritative tick and not the caller's: it is read here and the
/// guard on the oracle's current-tick shard is what pins it.
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

    let pool = pool_from_hex(&request.pool_data)?;
    let initial_tick = current_tick(
        &request.current_tick,
        pair.twap_oracle_program,
        compute_current_tick_account_pda(pair.twap_oracle_program, pair.pool),
    )?;
    let now = now_from_clock(&request.clock)?;

    let instruction = borsh::to_vec(&Instruction::CreatePriceObservations {
        window_duration: window,
        facts: pair.facts,
        identity: identity_of(&pool),
        initial_tick,
        now,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL slot order: config, pool (the price source), current_tick (supplies the initial
    // tick), price_observations (init), clock. Nothing signs. The two oracle accounts select the
    // configured TWAP program's shards -- it is the program that writes them, through the chain.
    let slots = [
        slot(pair.config, pair.amm_program),
        slot(pair.pool, pair.amm_program),
        slot(pair.current_tick, pair.twap_oracle_program),
        slot(price_observations, pair.twap_oracle_program),
        slot(pair.clock, clock_account_id()),
    ];
    Ok(plan_response(&request.amm_program_id, &slots, instruction))
}

/// The oracle's authoritative tick for a pool, declared as `initial_tick`.
fn current_tick(
    read: &AccountRead,
    twap_oracle_program: AccountId,
    expected_id: AccountId,
) -> Result<i32, String> {
    let (id, shard) = program_shard(read, twap_oracle_program, "current tick account")?;
    if id != expected_id {
        return Err(String::from("current tick account is not the pool's PDA"));
    }
    CurrentTickAccount::try_from(&shard)
        .map(|tick| tick.tick)
        .map_err(|_| String::from("current tick account is undecodable"))
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

    // The reserves the initial price is derived from, pinned *exactly* by the pool's
    // guard. A price account is seeded once and read by consumers afterwards, so a
    // stale-by-one-swap seed is not acceptable; the cost is that this retries when
    // the pool moved underneath it.
    let pool = pool_from_hex(&request.pool_data)?;
    let now = now_from_clock(&request.clock)?;

    let instruction = borsh::to_vec(&Instruction::CreateOraclePriceAccount {
        window_duration: window,
        facts: pair.facts,
        identity: identity_of(&pool),
        reserve_a: pool.reserve_a,
        reserve_b: pool.reserve_b,
        now,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL slot order: config, pool (the price source), oracle_price_account (init), clock.
    // Nothing signs.
    let slots = [
        slot(pair.config, pair.amm_program),
        slot(pair.pool, pair.amm_program),
        slot(oracle_price_account, pair.twap_oracle_program),
        slot(pair.clock, clock_account_id()),
    ];
    Ok(plan_response(&request.amm_program_id, &slots, instruction))
}
