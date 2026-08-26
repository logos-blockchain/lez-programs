use borsh::from_slice;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::account::AccountId;
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
    account_id_from_hex, account_id_hex, decode_account, program_id_bytes, AccountRead,
};

pub fn initialize_program_plan(request: InitializeProgramPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let admin = parse_account_id(&request.admin_id)?;
    let freeze_authority = parse_account_id(&request.freeze_authority_id)?;

    let (collateral_definition_id, collateral_definition) =
        required_account(&request.collateral_definition)?;
    let collateral = TokenDefinition::try_from(&collateral_definition.data)
        .map_err(|_| StablecoinApiError::new("invalid_collateral_definition"))?;
    if !matches!(collateral, TokenDefinition::Fungible { .. }) {
        return Err(StablecoinApiError::new("invalid_collateral_definition"));
    }

    let (market_price_oracle_id, market_price_oracle) =
        required_account(&request.market_price_oracle)?;
    let oracle = OraclePriceAccount::try_from(&market_price_oracle.data)
        .map_err(|_| StablecoinApiError::new("invalid_market_price_oracle"))?;

    let (clock_id, clock) = required_account(&request.clock)?;
    if clock_id != CLOCK_01_PROGRAM_ACCOUNT_ID
        || from_slice::<ClockAccountData>(clock.data.as_ref()).is_err()
    {
        return Err(StablecoinApiError::new("invalid_clock"));
    }

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
        [true, false, false, false, false, false, false, false, false],
        instruction,
    )
}

fn required_account(
    read: &AccountRead,
) -> Result<(AccountId, lee_core::account::Account), StablecoinApiError> {
    decode_account(read).map_err(|_| StablecoinApiError::new("account_read_failed"))
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

fn plan_response(
    program_id: lee_core::program::ProgramId,
    account_ids: [AccountId; 9],
    signing_requirements: [bool; 9],
    instruction: Instruction,
) -> StablecoinResult {
    let instruction = risc0_zkvm::serde::to_vec(&instruction)
        .map_err(|_| StablecoinApiError::new("backend_error"))?;
    Ok(json!({
        "programId": hex::encode(program_id_bytes(program_id)),
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
}
