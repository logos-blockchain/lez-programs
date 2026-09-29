use borsh::from_slice;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::account::AccountId;
use serde_json::{json, Value};
use stablecoin_core::{
    compute_position_pda, compute_position_vault_pda, compute_protocol_parameters_pda,
    compute_redemption_price_state_pda, compute_stability_fee_accumulator_pda,
    compute_stablecoin_definition_pda, compute_stablecoin_master_holding_pda, Instruction,
    Position,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

use super::{
    decode::{
        validated_protocol_parameters, validated_redemption_price_state,
        validated_stability_fee_accumulator,
    },
    parse_stablecoin_program_id,
    projection::clock_timestamp,
    quote::{redemption_rate_update_quote, validated_market_price_oracle},
    AccrueStabilityFeePlanRequest, DepositCollateralPlanRequest, InitializeProgramPlanRequest,
    OpenPositionPlanRequest, PositionAddressesRequest, RedemptionRateUpdateQuoteRequest,
    RefreshGlobalsPlanRequest, StablecoinApiError, StablecoinResult,
    UpdateRedemptionRatePlanRequest,
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

pub fn open_position_plan(request: OpenPositionPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let holding_id = parse_account_id(&request.user_collateral_holding_id)?;
    let position_nonce = parse_decimal_u64(&request.position_nonce)?;
    let initial_collateral_amount = parse_decimal_u128(&request.initial_collateral_amount)?;

    let (_, parameters) = validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    if parameters.is_frozen {
        return Err(StablecoinApiError::new("protocol_frozen"));
    }

    let (collateral_definition_id, collateral_definition_account) =
        required_account(&request.collateral_definition)?;
    if collateral_definition_id != parameters.collateral_definition_id {
        return Err(StablecoinApiError::new("collateral_definition_mismatch"));
    }
    let collateral_definition = TokenDefinition::try_from(&collateral_definition_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_collateral_definition"))?;
    if !matches!(collateral_definition, TokenDefinition::Fungible { .. }) {
        return Err(StablecoinApiError::new("invalid_collateral_definition"));
    }

    let (read_holding_id, holding_account) = required_account(&request.user_collateral_holding)?;
    if read_holding_id != holding_id {
        return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
    }
    let holding = TokenHolding::try_from(&holding_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_user_collateral_holding"))?;
    if !matches!(
        holding,
        TokenHolding::Fungible { definition_id, .. } if definition_id == collateral_definition_id
    ) {
        return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
    }
    if holding_account.program_owner != collateral_definition_account.program_owner {
        return Err(StablecoinApiError::new("token_program_mismatch"));
    }

    let (clock_id, _) = required_account(&request.clock)?;
    clock_timestamp(&request.clock)?;

    let position_id = compute_position_pda(program_id, owner, position_nonce);
    let vault_id = compute_position_vault_pda(program_id, position_id);
    plan_response(
        program_id,
        [
            owner,
            position_id,
            vault_id,
            holding_id,
            collateral_definition_id,
            compute_protocol_parameters_pda(program_id),
            clock_id,
        ],
        [true, false, false, true, false, false, false],
        Instruction::OpenPosition {
            position_nonce,
            initial_collateral_amount,
        },
    )
}

/// Derive a position and its collateral vault from the stablecoin program,
/// owner, and caller-chosen nonce.
pub fn position_addresses(request: PositionAddressesRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let position_nonce = parse_decimal_u64(&request.position_nonce)?;
    let position_id = compute_position_pda(program_id, owner, position_nonce);
    let vault_id = compute_position_vault_pda(program_id, position_id);
    Ok(json!({
        "positionId": position_id.to_string(),
        "positionIdHex": account_id_hex(position_id),
        "vaultId": vault_id.to_string(),
        "vaultIdHex": account_id_hex(vault_id),
    }))
}

pub fn deposit_collateral_plan(request: DepositCollateralPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let position_nonce = parse_decimal_u64(&request.position_nonce)?;
    let amount = parse_u128(&request.amount)?;
    let holding_id = parse_account_id(&request.user_collateral_holding_id)?;
    let (parameters_id, parameters) =
        validated_protocol_parameters(program_id, &request.protocol_parameters)?;

    let expected_position_id = compute_position_pda(program_id, owner, position_nonce);
    let (position_id, position_account) = required_account(&request.position)?;
    if position_id != expected_position_id {
        return Err(StablecoinApiError::new("position_pda_mismatch"));
    }
    if position_account.program_owner != program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }
    let position = Position::try_from(&position_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_data"))?;
    if position.owner_account_id != owner {
        return Err(StablecoinApiError::new("position_owner_mismatch"));
    }
    if position.position_nonce != position_nonce {
        return Err(StablecoinApiError::new("position_nonce_mismatch"));
    }

    let expected_vault_id = compute_position_vault_pda(program_id, position_id);
    if position.vault_account_id != expected_vault_id {
        return Err(StablecoinApiError::new("position_vault_mismatch"));
    }
    let (vault_id, vault_account) = required_account(&request.vault)?;
    if vault_id != expected_vault_id {
        return Err(StablecoinApiError::new("vault_pda_mismatch"));
    }
    let vault_holding = TokenHolding::try_from(&vault_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_vault"))?;
    let vault_balance = match vault_holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } if definition_id == parameters.collateral_definition_id => balance,
        TokenHolding::Fungible { .. } => {
            return Err(StablecoinApiError::new("collateral_definition_mismatch"));
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            return Err(StablecoinApiError::new("invalid_position_vault"));
        }
    };

    let (read_holding_id, holding_account) = required_account(&request.user_collateral_holding)?;
    if read_holding_id != holding_id {
        return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
    }
    if holding_account.program_owner != vault_account.program_owner {
        return Err(StablecoinApiError::new("token_program_mismatch"));
    }
    let holding = TokenHolding::try_from(&holding_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_user_collateral_holding"))?;
    let source_balance = match holding {
        TokenHolding::Fungible {
            definition_id,
            balance,
        } if definition_id == parameters.collateral_definition_id => balance,
        TokenHolding::Fungible { .. } => {
            return Err(StablecoinApiError::new("collateral_definition_mismatch"));
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            return Err(StablecoinApiError::new("invalid_user_collateral_holding"));
        }
    };
    if source_balance < amount {
        return Err(StablecoinApiError::new("insufficient_collateral_balance"));
    }
    vault_balance
        .checked_add(amount)
        .ok_or_else(|| StablecoinApiError::new("collateral_amount_overflow"))?;

    plan_response(
        program_id,
        [owner, position_id, vault_id, holding_id, parameters_id],
        [true, false, false, true, false],
        Instruction::DepositCollateral { amount },
    )
}

pub fn accrue_stability_fee_plan(request: AccrueStabilityFeePlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let caller = parse_account_id(&request.caller_id)?;
    validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    validated_stability_fee_accumulator(program_id, &request.stability_fee_accumulator)?;
    clock_timestamp(&request.clock)?;

    plan_response(
        program_id,
        [
            caller,
            compute_protocol_parameters_pda(program_id),
            compute_stability_fee_accumulator_pda(program_id),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        [true, false, false, false],
        Instruction::AccrueStabilityFee,
    )
}

pub fn update_redemption_rate_plan(request: UpdateRedemptionRatePlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let caller = parse_account_id(&request.caller_id)?;
    let (_, parameters) = validated_protocol_parameters(program_id, &request.protocol_parameters)?;

    let quote = redemption_rate_update_quote(RedemptionRateUpdateQuoteRequest {
        stablecoin_program_id: request.stablecoin_program_id,
        protocol_parameters: request.protocol_parameters,
        redemption_price_state: request.redemption_price_state,
        market_price_oracle: request.market_price_oracle,
        clock: request.clock,
    })?;
    require_ready_quote(&quote)?;

    plan_response(
        program_id,
        [
            caller,
            compute_protocol_parameters_pda(program_id),
            compute_redemption_price_state_pda(program_id),
            parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        [true, false, false, false, false],
        Instruction::UpdateRedemptionRate,
    )
}

pub fn refresh_globals_plan(request: RefreshGlobalsPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let caller = parse_account_id(&request.caller_id)?;
    let (_, parameters) = validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    validated_stability_fee_accumulator(program_id, &request.stability_fee_accumulator)?;
    validated_redemption_price_state(program_id, &request.redemption_price_state)?;
    validated_market_price_oracle(
        &request.market_price_oracle,
        parameters.market_price_oracle_id,
    )?;
    clock_timestamp(&request.clock)?;

    plan_response(
        program_id,
        [
            caller,
            compute_protocol_parameters_pda(program_id),
            compute_stability_fee_accumulator_pda(program_id),
            compute_redemption_price_state_pda(program_id),
            parameters.market_price_oracle_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        [true, false, false, false, false, false],
        Instruction::RefreshGlobals,
    )
}

fn require_ready_quote(quote: &Value) -> Result<(), StablecoinApiError> {
    if quote.get("canSubmit").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }

    let blocker = quote
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str);
    let code = match blocker {
        Some("oracle_stale") => "oracle_stale",
        Some("oracle_price_zero") => "oracle_price_zero",
        Some("rate_update_too_soon") => "rate_update_too_soon",
        _ => "backend_error",
    };
    Err(StablecoinApiError::new(code))
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

fn parse_decimal_u128(value: &str) -> Result<u128, StablecoinApiError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StablecoinApiError::new("invalid_numeric_value"));
    }
    value
        .parse::<u128>()
        .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
}

fn parse_decimal_u64(value: &str) -> Result<u64, StablecoinApiError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(StablecoinApiError::new("invalid_numeric_value"));
    }
    value
        .parse::<u64>()
        .map_err(|_| StablecoinApiError::new("invalid_numeric_value"))
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

fn plan_response<const ACCOUNT_COUNT: usize>(
    program_id: lee_core::program::ProgramId,
    account_ids: [AccountId; ACCOUNT_COUNT],
    signing_requirements: [bool; ACCOUNT_COUNT],
    instruction: Instruction,
) -> StablecoinResult {
    let instruction = risc0_zkvm::serde::to_vec(&instruction)
        .map_err(|_| StablecoinApiError::new("backend_error"))?;
    Ok(json!({
        "programId": hex::encode(program_id_bytes(program_id)),
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements.into_iter().collect::<Vec<_>>(),
        "instruction": instruction,
    }))
}
