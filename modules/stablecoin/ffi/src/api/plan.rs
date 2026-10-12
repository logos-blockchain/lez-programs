use borsh::from_slice;
use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::account::{AccountId, ShardData};
use serde_json::{json, Value};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, compute_stablecoin_definition_pda,
    compute_stablecoin_master_holding_pda, Instruction,
};
use token_core::TokenDefinition;
use twap_oracle_core::OraclePriceAccount;

use super::{
    parse_stablecoin_program_id, InitializeProgramPlanRequest, StablecoinApiError, StablecoinResult,
};
use crate::account::{
    account_id_from_hex, account_id_hex, decode_shard, AccountRead, AccountReadError,
};

pub fn initialize_program_plan(request: InitializeProgramPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let admin = parse_account_id(&request.admin_id)?;
    let freeze_authority = parse_account_id(&request.freeze_authority_id)?;
    let token_program_id = parse_account_id(&request.token_program_id)?;
    let oracle_program_id = parse_account_id(&request.market_price_oracle_program_id)?;

    let (collateral_definition_id, collateral_shard) =
        required_shard(&request.collateral_definition, token_program_id)?;
    let collateral = TokenDefinition::try_from(&collateral_shard)
        .map_err(|_| StablecoinApiError::new("invalid_collateral_definition"))?;
    if !matches!(collateral, TokenDefinition::Fungible { .. }) {
        return Err(StablecoinApiError::new("invalid_collateral_definition"));
    }

    let (market_price_oracle_id, oracle_shard) =
        required_shard(&request.market_price_oracle, oracle_program_id)?;
    let oracle = OraclePriceAccount::try_from(&oracle_shard)
        .map_err(|_| StablecoinApiError::new("invalid_market_price_oracle"))?;

    // The clock is read for its timestamp, not just validated: `now` is a declared
    // instruction field in v0.3.0, and the guest pins it against this very shard
    // with its `CheckClock` guard. So the client-side read is what makes the
    // declaration, and the guard is what makes it true.
    let (clock_id, clock_shard) = required_shard(&request.clock, clock_account_id())?;
    let Ok(clock) = from_slice::<ClockAccountData>(clock_shard.as_ref()) else {
        return Err(StablecoinApiError::new("invalid_clock"));
    };
    if clock_id != CLOCK_01_PROGRAM_ACCOUNT_ID {
        return Err(StablecoinApiError::new("invalid_clock"));
    }
    let now = clock.timestamp;

    let stablecoin_definition_id = compute_stablecoin_definition_pda(program_id);
    if oracle.base_asset != stablecoin_definition_id
        || oracle.quote_asset != collateral_definition_id
    {
        return Err(StablecoinApiError::new("oracle_asset_mismatch"));
    }

    let initial_stability_fee_per_millisecond =
        parse_u128(&request.initial_stability_fee_per_millisecond)?;
    let initial_controller_proportional_gain =
        parse_i128(&request.initial_controller_proportional_gain)?;
    let initial_controller_integral_gain = parse_i128(&request.initial_controller_integral_gain)?;
    let initial_minimum_collateralization_ratio =
        parse_u128(&request.initial_minimum_collateralization_ratio)?;
    let minimum_milliseconds_between_rate_updates =
        parse_u64(&request.minimum_milliseconds_between_rate_updates)?;
    let maximum_oracle_price_age_milliseconds =
        parse_u64(&request.maximum_oracle_price_age_milliseconds)?;
    let initial_redemption_price = parse_u128(&request.initial_redemption_price)?;
    if request.stablecoin_name.is_empty() {
        return Err(StablecoinApiError::new("invalid_stablecoin_name"));
    }

    let instruction = Instruction::InitializeProgram {
        freeze_authority_account_id: freeze_authority,
        initial_stability_fee_per_millisecond,
        initial_controller_proportional_gain,
        initial_controller_integral_gain,
        initial_minimum_collateralization_ratio,
        minimum_milliseconds_between_rate_updates,
        maximum_oracle_price_age_milliseconds,
        initial_redemption_price,
        stablecoin_name: request.stablecoin_name,
        now,
    };

    plan_response(
        program_id,
        [
            admin,
            compute_protocol_parameters_pda(program_id),
            compute_stability_fee_accumulator_pda(program_id),
            compute_redemption_price_state_pda(program_id),
            stablecoin_definition_id,
            compute_stablecoin_master_holding_pda(program_id),
            collateral_definition_id,
            market_price_oracle_id,
            clock_id,
        ],
        // Which program's shard each slot selects. Unlike a Token instruction,
        // this one spans four programs, and three of the entries are load-bearing
        // rather than incidental: `collateral_definition`'s and
        // `market_price_oracle`'s are the values `initialize_program` *records* as
        // the protocol's bindings, and the clock's is what makes the timestamp the
        // real one rather than bytes a caller authored at that address.
        //
        // The stablecoin definition and its master holding are the Token Program's
        // because that is who writes them -- through the chained
        // `NewFungibleDefinition` -- even though this program declares the slots.
        [
            program_id,
            program_id,
            program_id,
            program_id,
            token_program_id,
            token_program_id,
            token_program_id,
            oracle_program_id,
            clock_account_id(),
        ],
        [true, false, false, false, false, false, false, false, false],
        instruction,
    )
}

/// One program's shard of an account read, required to be present.
fn required_shard(
    read: &AccountRead,
    program_account_id: AccountId,
) -> Result<(AccountId, ShardData), StablecoinApiError> {
    decode_shard(read, program_account_id).map_err(|error| match error {
        AccountReadError::Failed | AccountReadError::Malformed => {
            StablecoinApiError::new("account_read_failed")
        }
        AccountReadError::NoShard => StablecoinApiError::new("program_shard_missing"),
    })
}

fn parse_account_id(value: &str) -> Result<AccountId, StablecoinApiError> {
    let account_id = account_id_from_hex(value, "account id")
        .map_err(|_| StablecoinApiError::new("invalid_account_id"))?;
    if account_id.value() == &[0_u8; 32] {
        return Err(StablecoinApiError::new("invalid_account_id"));
    }
    Ok(account_id)
}

fn decimal_text(value: &str) -> Result<&str, StablecoinApiError> {
    let trimmed = value.trim();
    let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    };
    if unquoted.is_empty() {
        return Err(StablecoinApiError::new("invalid_numeric_value"));
    }
    Ok(unquoted)
}

fn parse_u128(value: &Value) -> Result<u128, StablecoinApiError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(u128::from)
            .ok_or_else(|| StablecoinApiError::new("invalid_numeric_value")),
        Value::String(raw) => {
            let raw = decimal_text(raw)?;
            if !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(StablecoinApiError::new("invalid_numeric_value"));
            }
            raw.parse::<u128>()
                .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
        }
        _ => Err(StablecoinApiError::new("invalid_numeric_value")),
    }
}

fn parse_u64(value: &Value) -> Result<u64, StablecoinApiError> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| StablecoinApiError::new("invalid_numeric_value")),
        Value::String(raw) => {
            let raw = decimal_text(raw)?;
            if !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(StablecoinApiError::new("invalid_numeric_value"));
            }
            raw.parse::<u64>()
                .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
        }
        _ => Err(StablecoinApiError::new("invalid_numeric_value")),
    }
}

fn parse_i128(value: &Value) -> Result<i128, StablecoinApiError> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .ok_or_else(|| StablecoinApiError::new("invalid_numeric_value")),
        Value::String(raw) => {
            let raw = decimal_text(raw)?;
            let digits = raw.strip_prefix('-').unwrap_or(raw);
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(StablecoinApiError::new("invalid_numeric_value"));
            }
            raw.parse::<i128>()
                .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
        }
        _ => Err(StablecoinApiError::new("invalid_numeric_value")),
    }
}

/// Builds the plan envelope the module's C++ hands to the wallet.
///
/// `programAccountIds` is new in v0.3.0: a transaction names *shards*, not
/// accounts, so every slot carries the program whose shard it selects alongside
/// its address. It travels as a parallel array because `signingRequirements`
/// already does, and the three stay index-aligned.
fn plan_response(
    program_id: AccountId,
    account_ids: [AccountId; 9],
    program_account_ids: [AccountId; 9],
    signing_requirements: [bool; 9],
    instruction: Instruction,
) -> StablecoinResult {
    // LEZ v0.2.5 decodes instruction data as Borsh, not RISC Zero's serde codec. The
    // payload is therefore a byte array now, where it used to be an array of u32 words.
    let instruction =
        borsh::to_vec(&instruction).map_err(|_| StablecoinApiError::new("backend_error"))?;
    Ok(json!({
        "programId": account_id_hex(program_id),
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "programAccountIds": program_account_ids
            .into_iter()
            .map(account_id_hex)
            .collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
}
