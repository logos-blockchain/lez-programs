//! TWAP discovery and trusted instruction-plan construction. Account existence
//! and operation-specific validation belong to the later transaction planners.

use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use lee_core::account::AccountId;
use serde::Serialize;
use serde_json::{json, Map, Value};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda, Instruction, OBSERVATIONS_CAPACITY,
};

use super::{numeric::parse_u64, parse_id, FeedAddressesRequest, OracleApiError, OracleResult};
use crate::account::account_id_hex;

pub fn feed_addresses(request: FeedAddressesRequest) -> OracleResult {
    let program = parse_id(&request.oracle_program_id, "invalid_program_id")?;
    let source = parse_id(&request.price_source_id, "invalid_account_id")?;
    let window = parse_u64(&request.window_duration_ms)?;
    if window < u64::from(OBSERVATIONS_CAPACITY) {
        return Err(OracleApiError::new("invalid_window"));
    }
    let mut value = Map::new();
    insert_id(
        &mut value,
        "currentTickAccountId",
        compute_current_tick_account_pda(program, source),
    );
    insert_id(
        &mut value,
        "priceObservationsId",
        compute_price_observations_pda(program, source, window),
    );
    insert_id(
        &mut value,
        "priceAccountId",
        compute_oracle_price_account_pda(program, source, window),
    );
    insert_id(&mut value, "clockId", CLOCK_01_PROGRAM_ACCOUNT_ID);
    Ok(Value::Object(value))
}

pub(crate) fn insert_id(value: &mut Map<String, Value>, key: &str, id: AccountId) {
    value.insert(key.to_owned(), json!(id.to_string()));
    value.insert(format!("{key}Hex"), json!(account_id_hex(id)));
}

/// Internal wire plan: identities are canonical hex and instruction data is
/// Borsh bytes. This is not a public Logos transaction method or an RPC input.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPlan {
    program_id: String,
    account_ids: Vec<String>,
    signing_requirements: Vec<bool>,
    instruction: Vec<u8>,
}

impl TransactionPlan {
    /// The caller supplies validated accounts in guest order. Signer flags come
    /// from the typed instruction, so source writes always sign at index 1 and
    /// maintenance never asks for an instruction signature.
    pub fn new<const N: usize>(
        program_id: AccountId,
        account_ids: [AccountId; N],
        instruction: Instruction,
    ) -> Result<Self, OracleApiError> {
        let (count, source_write) = match &instruction {
            Instruction::CreatePriceObservations { .. }
            | Instruction::CreateOraclePriceAccount { .. }
            | Instruction::CreateCurrentTickAccount { .. }
            | Instruction::UpdateCurrentTick { .. } => (3, true),
            Instruction::RecordTick { .. } => (3, false),
            Instruction::PublishPrice { .. } => (4, false),
        };
        if N != count {
            return Err(OracleApiError::new("bad_request"));
        }
        let instruction =
            borsh::to_vec(&instruction).map_err(|_| OracleApiError::new("backend_error"))?;
        Ok(Self {
            program_id: account_id_hex(program_id),
            account_ids: account_ids.into_iter().map(account_id_hex).collect(),
            signing_requirements: (0..N).map(|index| source_write && index == 1).collect(),
            instruction,
        })
    }
}
