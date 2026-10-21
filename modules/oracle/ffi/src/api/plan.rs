//! Typed transaction planners, one per TWAP oracle instruction. Each derives the
//! instruction's PDAs from the program id and price source, validates its
//! arguments, and returns `{plan, identities}`. Account state (existence,
//! ownership) is checked by the host against live reads before submission.

use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::{account::AccountId, program::ProgramId};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, Instruction, OBSERVATIONS_CAPACITY,
};

use super::{
    numeric::{parse_i32, parse_u128, parse_u64},
    parse_id, parse_oracle_program_id,
    twap::insert_id,
    OracleApiError, OracleResult, TransactionPlan,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCurrentTickAccountRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub initial_price: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePriceObservationsRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub initial_tick: String,
    pub window_duration_ms: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateOraclePriceAccountRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub base_asset_id: String,
    pub quote_asset_id: String,
    pub initial_price: String,
    pub window_duration_ms: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateCurrentTickRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub price: String,
}

/// Shared by `recordTick` and `publishPrice`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedMaintenanceRequest {
    pub oracle_program_id: String,
    pub price_source_id: String,
    pub window_duration_ms: String,
}

struct Feed {
    program: ProgramId,
    source: AccountId,
}

impl Feed {
    fn parse(program: &str, source: &str) -> Result<Self, OracleApiError> {
        Ok(Self {
            program: parse_oracle_program_id(program)?,
            source: parse_id(source, "invalid_account_id")?,
        })
    }

    fn current_tick(&self) -> AccountId {
        compute_current_tick_account_pda(self.program, self.source)
    }

    fn observations(&self, window: u64) -> AccountId {
        compute_price_observations_pda(self.program, self.source, window)
    }

    fn price(&self, window: u64) -> AccountId {
        compute_oracle_price_account_pda(self.program, self.source, window)
    }
}

/// The guest rejects windows below `OBSERVATIONS_CAPACITY`: the sampling
/// interval `window / OBSERVATIONS_CAPACITY` must be at least 1 ms.
fn parse_window(value: &str) -> Result<u64, OracleApiError> {
    let window = parse_u64(value)?;
    if window < u64::from(OBSERVATIONS_CAPACITY) {
        return Err(OracleApiError::new("invalid_window"));
    }
    Ok(window)
}

fn planned<const N: usize>(
    feed: &Feed,
    accounts: [AccountId; N],
    instruction: Instruction,
    identities: &[(&str, AccountId)],
) -> OracleResult {
    let plan = TransactionPlan::new(feed.program, accounts, instruction)?;
    let mut ids = Map::new();
    for (key, id) in identities {
        insert_id(&mut ids, key, *id);
    }
    insert_id(&mut ids, "priceSourceId", feed.source);
    insert_id(&mut ids, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);
    Ok(json!({
        "plan": serde_json::to_value(plan).map_err(|_| OracleApiError::new("backend_error"))?,
        "identities": Value::Object(ids),
    }))
}

pub fn create_current_tick_account(request: CreateCurrentTickAccountRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let initial_price = parse_u128(&request.initial_price)?;
    let tick = feed.current_tick();
    planned(
        &feed,
        [tick, feed.source, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::CreateCurrentTickAccount { initial_price },
        &[("currentTickAccountId", tick)],
    )
}

pub fn create_price_observations(request: CreatePriceObservationsRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let initial_tick = parse_i32(&request.initial_tick)?;
    let window_duration = parse_window(&request.window_duration_ms)?;
    let observations = feed.observations(window_duration);
    planned(
        &feed,
        [observations, feed.source, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::CreatePriceObservations {
            initial_tick,
            window_duration,
        },
        &[("priceObservationsId", observations)],
    )
}

pub fn create_oracle_price_account(request: CreateOraclePriceAccountRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let base_asset = parse_id(&request.base_asset_id, "invalid_account_id")?;
    let quote_asset = parse_id(&request.quote_asset_id, "invalid_account_id")?;
    let initial_price = parse_u128(&request.initial_price)?;
    // Zero is the "no valid price" sentinel; the guest never creates it.
    if initial_price == 0 {
        return Err(OracleApiError::new("invalid_price"));
    }
    let window_duration = parse_window(&request.window_duration_ms)?;
    let price = feed.price(window_duration);
    planned(
        &feed,
        [price, feed.source, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::CreateOraclePriceAccount {
            base_asset,
            quote_asset,
            initial_price,
            window_duration,
        },
        &[
            ("priceAccountId", price),
            ("baseAssetId", base_asset),
            ("quoteAssetId", quote_asset),
        ],
    )
}

pub fn update_current_tick(request: UpdateCurrentTickRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let price = parse_u128(&request.price)?;
    let tick = feed.current_tick();
    planned(
        &feed,
        [tick, feed.source, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::UpdateCurrentTick { price },
        &[("currentTickAccountId", tick)],
    )
}

pub fn record_tick(request: FeedMaintenanceRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let window_duration = parse_window(&request.window_duration_ms)?;
    let observations = feed.observations(window_duration);
    let tick = feed.current_tick();
    planned(
        &feed,
        [observations, tick, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::RecordTick {
            price_source_id: feed.source,
            window_duration,
        },
        &[
            ("priceObservationsId", observations),
            ("currentTickAccountId", tick),
        ],
    )
}

pub fn publish_price(request: FeedMaintenanceRequest) -> OracleResult {
    let feed = Feed::parse(&request.oracle_program_id, &request.price_source_id)?;
    let window_duration = parse_window(&request.window_duration_ms)?;
    let observations = feed.observations(window_duration);
    let price = feed.price(window_duration);
    let tick = feed.current_tick();
    planned(
        &feed,
        [observations, price, tick, CLOCK_01_PROGRAM_ACCOUNT_ID],
        Instruction::PublishPrice {
            price_source_id: feed.source,
            window_duration,
        },
        &[
            ("priceObservationsId", observations),
            ("priceAccountId", price),
            ("currentTickAccountId", tick),
        ],
    )
}
