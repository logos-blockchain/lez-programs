use alloy_primitives::{U256, U512};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use stablecoin_core::{
    collateralization::collateralization_values,
    compute_stablecoin_definition_pda,
    math::{
        compute_current_redemption_price_wide, FIXED_POINT_ONE,
        MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS,
    },
    Instruction,
};
use token_core::{TokenDefinition, TokenHolding};

use super::{
    decode::{
        validated_protocol_parameters, validated_redemption_price_state,
        validated_stability_fee_accumulator,
    },
    parse_stablecoin_program_id,
    plan::{parse_account_id, parse_decimal_u64, parse_u128, plan_response, required_account},
    position::validated_position,
    projection::clock_timestamp,
    quote::validated_market_price_oracle,
    GenerateDebtPlanRequest, StablecoinApiError, StablecoinResult,
};

/// Validate borrowing with narrow debt pricing and wide post-mint health.
pub fn generate_debt_plan(request: GenerateDebtPlanRequest) -> StablecoinResult {
    let program_id = parse_stablecoin_program_id(&request.stablecoin_program_id)?;
    let owner = parse_account_id(&request.owner_id)?;
    let nonce = parse_decimal_u64(&request.position_nonce)?;
    let amount = parse_u128(&request.amount)?;
    let destination_id = parse_account_id(&request.user_stablecoin_holding_id)?;
    let (position_id, position) = validated_position(program_id, owner, nonce, &request.position)?;
    let (parameters_id, parameters) =
        validated_protocol_parameters(program_id, &request.protocol_parameters)?;
    if parameters.is_frozen {
        return Err(StablecoinApiError::new("protocol_frozen"));
    }
    let (accumulator_id, accumulator) =
        validated_stability_fee_accumulator(program_id, &request.stability_fee_accumulator)?;
    let (redemption_id, redemption) =
        validated_redemption_price_state(program_id, &request.redemption_price_state)?;
    let now = clock_timestamp(&request.clock)?;
    let oracle = validated_market_price_oracle(
        &request.market_price_oracle,
        parameters.market_price_oracle_id,
    )?;
    let age = now
        .checked_sub(oracle.timestamp)
        .ok_or_else(|| StablecoinApiError::new("oracle_future"))?;
    if age > parameters.maximum_oracle_price_age_milliseconds {
        return Err(StablecoinApiError::new("oracle_stale"));
    }

    let (definition_id, definition_account) = required_account(&request.stablecoin_definition)?;
    if definition_id != parameters.stablecoin_definition_id
        || definition_id != compute_stablecoin_definition_pda(program_id)
    {
        return Err(StablecoinApiError::new("stablecoin_definition_mismatch"));
    }
    let definition = TokenDefinition::try_from(&definition_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_stablecoin_definition"))?;
    let total_supply = match definition {
        TokenDefinition::Fungible {
            total_supply,
            authority,
            ..
        } => {
            if authority != Some(definition_id) {
                return Err(StablecoinApiError::new("invalid_stablecoin_mint_authority"));
            }
            total_supply
        }
        TokenDefinition::NonFungible { .. } => {
            return Err(StablecoinApiError::new("invalid_stablecoin_definition"))
        }
    };
    if destination_id == owner || destination_id == definition_id {
        return Err(StablecoinApiError::new("invalid_user_stablecoin_holding"));
    }
    let (read_destination, destination_account) =
        required_account(&request.user_stablecoin_holding)?;
    if read_destination != destination_id {
        return Err(StablecoinApiError::new("invalid_user_stablecoin_holding"));
    }
    if destination_account.program_owner != definition_account.program_owner {
        return Err(StablecoinApiError::new("token_program_mismatch"));
    }
    let destination = TokenHolding::try_from(&destination_account.data)
        .map_err(|_| StablecoinApiError::new("invalid_user_stablecoin_holding"))?;
    let balance = match destination {
        TokenHolding::Fungible {
            definition_id: holding_definition,
            balance,
        } if holding_definition == definition_id => balance,
        TokenHolding::Fungible { .. } => {
            return Err(StablecoinApiError::new("stablecoin_definition_mismatch"))
        }
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            return Err(StablecoinApiError::new("invalid_user_stablecoin_holding"))
        }
    };
    total_supply
        .checked_add(amount)
        .ok_or_else(|| StablecoinApiError::new("stablecoin_supply_overflow"))?;
    balance
        .checked_add(amount)
        .ok_or_else(|| StablecoinApiError::new("stablecoin_balance_overflow"))?;

    let current_accumulator = checked_accumulator(
        accumulator.accumulated_rate_at_last_accrual,
        parameters.stability_fee_per_millisecond,
        accumulator.last_accrued_at,
        now,
    )
    .ok_or_else(|| StablecoinApiError::new("debt_pricing_arithmetic_error"))?;
    let debt_delta = checked_debt_delta(amount, current_accumulator)
        .ok_or_else(|| StablecoinApiError::new("debt_pricing_arithmetic_error"))?;
    let new_debt = position
        .normalized_debt_amount
        .checked_add(debt_delta)
        .ok_or_else(|| StablecoinApiError::new("normalized_debt_overflow"))?;
    let price = compute_current_redemption_price_wide(
        redemption.redemption_price_at_last_update,
        redemption.redemption_rate_per_millisecond,
        redemption.last_updated_at,
        now,
    );
    if price.is_zero() {
        return Err(StablecoinApiError::new("redemption_price_zero"));
    }
    if !collateralization_values(
        position.collateral_amount,
        new_debt,
        U512::from(current_accumulator),
        price,
        parameters.minimum_collateralization_ratio,
    )
    .is_collateralized()
    {
        return Err(StablecoinApiError::new("position_undercollateralized"));
    }
    plan_response(
        program_id,
        [
            owner,
            position_id,
            definition_id,
            destination_id,
            accumulator_id,
            redemption_id,
            parameters.market_price_oracle_id,
            parameters_id,
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        [true, false, false, false, false, false, false, false, false],
        Instruction::GenerateDebt { amount },
    )
}

fn checked_mul_div(a: u128, b: u128, divisor: u128) -> Option<u128> {
    // Two u128 factors fit U256; division and narrowing can still fail.
    (U256::from(a) * U256::from(b))
        .checked_div(U256::from(divisor))?
        .try_into()
        .ok()
}

/// Fallible equivalent of the native narrow accumulator projection. Every
/// compounding step narrows to u128, exactly as the guest does.
pub(super) fn checked_accumulator(anchor: u128, rate: u128, last: u64, now: u64) -> Option<u128> {
    let mut exponent = now
        .saturating_sub(last)
        .min(MAXIMUM_COMPOUNDING_WINDOW_MILLISECONDS);
    let mut factor = FIXED_POINT_ONE;
    if rate != FIXED_POINT_ONE {
        let mut base = rate;
        while exponent > 0 {
            if exponent & 1 == 1 {
                factor = checked_mul_div(factor, base, FIXED_POINT_ONE)?;
            }
            exponent >>= 1;
            if exponent > 0 {
                base = checked_mul_div(base, base, FIXED_POINT_ONE)?;
            }
        }
    }
    checked_mul_div(anchor, factor, FIXED_POINT_ONE)
}

pub(super) fn checked_debt_delta(amount: u128, accumulator: u128) -> Option<u128> {
    let product = U256::from(amount) * U256::from(FIXED_POINT_ONE);
    let divisor = U256::from(accumulator);
    let quotient = product.checked_div(divisor)?;
    // The fixed-point product has at most 218 bits, so rounding up fits U256.
    let rounded = if (product % divisor).is_zero() {
        quotient
    } else {
        quotient + U256::ONE
    };
    rounded.try_into().ok()
}
