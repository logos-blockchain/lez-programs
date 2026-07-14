use alloy_primitives::U256;
use amm_core::{
    is_supported_fee_tier, isqrt_product, mul_div_floor, spot_price_q64_64, PoolDefinition,
    FEE_BPS_DENOMINATOR, MINIMUM_LIQUIDITY,
};
use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use serde_json::{json, Value};
use token_core::TokenDefinition;
use twap_oracle_core::CurrentTickAccount;

use super::{
    accounts::{active_account_plan, missing_account_plan},
    clock::decode_clock,
    commitment::{QuoteCommitment, RequestCommitment},
    context::fungible_definition,
    funding::{funding_commitments, funding_issues, hash_quote},
    holding::{decode_fungible_holding, select_holding, wallet_holdings},
    pair::{derive_pair, is_canonical_pair, PairIds},
    position::{
        AccountPlan, AccountPlanHoldings, EvaluatedQuote, NewPositionPlan, QuoteBranch,
        QuoteComputation,
    },
    quote_error::{fatal_quote, issue},
    QuoteRequest, SCHEMA,
};
use crate::account::{
    decode_account, parse_base58_id, parse_program_id, program_id_bytes, AccountRead,
};

const DEFAULT_SLIPPAGE_BPS: u32 = 50;
const MAX_SLIPPAGE_BPS: u32 = 5_000;
const HIGH_SLIPPAGE_BPS: u32 = 2_000;
pub(super) const Q64: u128 = 1_u128 << 64;

pub(super) fn quote(request: QuoteRequest) -> Result<Value, String> {
    Ok(compute_quote(&request)?.into_value(&request.request))
}

pub(super) fn compute_quote(input: &QuoteRequest) -> Result<QuoteComputation, String> {
    if input.request.schema != SCHEMA {
        return Ok(fatal_quote(
            "unsupported_schema",
            &["schema"],
            json!({ "received": input.request.schema }),
        ));
    }
    let amm_program = parse_program_id(&input.amm_program_id)?;
    let token_a = match parse_base58_id(&input.request.token_a_id, "token A id") {
        Ok(id) => id,
        Err(_) => return Ok(fatal_quote("invalid_token_id", &["tokenAId"], json!({}))),
    };
    let token_b = match parse_base58_id(&input.request.token_b_id, "token B id") {
        Ok(id) => id,
        Err(_) => return Ok(fatal_quote("invalid_token_id", &["tokenBId"], json!({}))),
    };
    if token_a == token_b {
        return Ok(fatal_quote(
            "same_token_pair",
            &["tokenAId", "tokenBId"],
            json!({}),
        ));
    }
    if !is_canonical_pair(token_a, token_b) {
        return Ok(fatal_quote(
            "non_canonical_pair",
            &["tokenAId", "tokenBId"],
            json!({}),
        ));
    }
    if !is_supported_fee_tier(u128::from(input.request.fee_bps)) {
        return Ok(fatal_quote(
            "invalid_fee_tier",
            &["feeBps"],
            json!({ "feeBps": input.request.fee_bps }),
        ));
    }

    let pair = match derive_pair(amm_program, token_a, token_b, &input.snapshot.config) {
        Ok(pair) => pair,
        Err(_) => return Ok(fatal_quote("config_unavailable", &[], json!({}))),
    };
    if let Some(error) = AccountPlan::validate_snapshot_ids(&pair, &input.snapshot) {
        return Ok(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": error }),
        ));
    }
    for (read, token_id, field) in [
        (&input.snapshot.token_a, token_a, "tokenAId"),
        (&input.snapshot.token_b, token_b, "tokenBId"),
    ] {
        if let Err(error) = fungible_definition(Some(read), token_id, pair.token_program) {
            return Ok(fatal_quote(
                error.code,
                &[field],
                json!({ "tokenId": token_id.to_string() }),
            ));
        }
    }

    let (_, pool_account) = match decode_account(&input.snapshot.pool) {
        Ok(value) => value,
        Err(_) => {
            return Ok(fatal_quote(
                "account_read_failed",
                &[],
                json!({ "role": "pool", "accountId": pair.pool.to_string() }),
            ))
        }
    };
    if pool_account == Account::default() {
        compute_missing_quote(input, amm_program, pair)
    } else {
        compute_active_quote(input, amm_program, pair, pool_account)
    }
}

fn compute_missing_quote(
    input: &QuoteRequest,
    amm_program: ProgramId,
    pair: PairIds,
) -> Result<QuoteComputation, String> {
    for (read, role) in [
        (&input.snapshot.vault_a, "vault_a"),
        (&input.snapshot.vault_b, "vault_b"),
        (&input.snapshot.lp_definition, "lp_definition"),
        (&input.snapshot.lp_lock_holding, "lp_lock_holding"),
        (&input.snapshot.current_tick, "current_tick"),
    ] {
        let Ok((_, account)) = decode_account(read) else {
            return Ok(fatal_quote(
                "account_read_failed",
                &[],
                json!({ "role": role }),
            ));
        };
        if account != Account::default() {
            return Ok(fatal_quote(
                "pool_unavailable",
                &[],
                json!({ "role": role }),
            ));
        }
    }
    if !valid_clock(&input.snapshot.clock, pair.clock) {
        return Ok(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": "clock" }),
        ));
    }

    let requested_price = match raw_value(input.request.initial_price_real_raw.as_deref()) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                "amount_must_be_positive",
                &["initialPriceRealRaw"],
                json!({}),
            ))
        }
        Err(code) => return Ok(fatal_quote(code, &["initialPriceRealRaw"], json!({}))),
    };
    let (minimum_a, minimum_b) = minimum_opening_pair(requested_price)?;
    let direct_amounts =
        input.request.amount_a_raw.is_some() || input.request.amount_b_raw.is_some();
    let (amount_a, amount_b) = if direct_amounts {
        let amount_a = match raw_value(input.request.amount_a_raw.as_deref()) {
            Ok(value) if value > 0 => value,
            Ok(_) => {
                return Ok(fatal_quote(
                    "amount_must_be_positive",
                    &["amountARaw"],
                    json!({}),
                ))
            }
            Err(code) => return Ok(fatal_quote(code, &["amountARaw"], json!({}))),
        };
        let amount_b = match raw_value(input.request.amount_b_raw.as_deref()) {
            Ok(value) if value > 0 => value,
            Ok(_) => {
                return Ok(fatal_quote(
                    "amount_must_be_positive",
                    &["amountBRaw"],
                    json!({}),
                ))
            }
            Err(code) => return Ok(fatal_quote(code, &["amountBRaw"], json!({}))),
        };
        if spot_price_q64_64(amount_a, amount_b) != requested_price {
            return Ok(fatal_quote(
                "deposit_ratio_mismatch",
                &["amountARaw", "amountBRaw"],
                json!({}),
            ));
        }
        (amount_a, amount_b)
    } else {
        (minimum_a, minimum_b)
    };
    let initial_lp = isqrt_product(amount_a, amount_b);
    if initial_lp <= MINIMUM_LIQUIDITY {
        return Ok(fatal_quote(
            "amount_too_low",
            &["amountARaw", "amountBRaw"],
            json!({ "minimumLiquidityRaw": MINIMUM_LIQUIDITY.to_string() }),
        ));
    }
    let expected_lp = initial_lp - MINIMUM_LIQUIDITY;

    let holdings = wallet_holdings(&input.snapshot.wallet_accounts, pair.token_program);
    let holding_a = select_holding(&holdings, pair.token_a);
    let holding_b = select_holding(&holdings, pair.token_b);
    let funding = funding_issues(
        input.snapshot.wallet_available,
        pair,
        &holding_a,
        amount_a,
        &holding_b,
        amount_b,
        ["amountARaw", "amountBRaw"],
    );
    let can_submit = funding.is_empty();
    let mut account_plan = missing_account_plan(
        input,
        pair,
        amm_program,
        AccountPlanHoldings {
            token_a: holding_a.as_ref(),
            token_b: holding_b.as_ref(),
            lp: None,
        },
    )?;
    let sources = account_plan.take_sources();
    let funding_commitment = funding_commitments(pair, &holding_a, amount_a, &holding_b, amount_b);
    let commitment = QuoteCommitment {
        schema: String::from(SCHEMA),
        network_id: input.network_id.clone(),
        network_fingerprint: input.network_fingerprint.clone(),
        amm_program_id: program_id_bytes(amm_program),
        token_a_id: pair.token_a.into_value(),
        token_b_id: pair.token_b.into_value(),
        fee_bps: input.request.fee_bps,
        pool_status: 0,
        request: RequestCommitment::Missing { amount_a, amount_b },
        max_a: amount_a,
        max_b: amount_b,
        actual_a: amount_a,
        actual_b: amount_b,
        expected_lp,
        lp_guard: MINIMUM_LIQUIDITY,
        requires_fresh_lp: true,
        sources,
        funding: funding_commitment,
        warnings: Vec::new(),
    };
    let quote_hash = hash_quote(&commitment)?;
    let preview = account_plan.preview();
    let value = json!({
        "schema": SCHEMA,
        "status": "ok",
        "canSubmit": can_submit,
        "code": if can_submit { "ready" } else { "funding_required" },
        "poolStatus": "missing_pool",
        "instruction": "NewDefinition",
        "quoteHash": quote_hash,
        "feeBps": input.request.fee_bps,
        "poolId": pair.pool.to_string(),
        "tokenAId": pair.token_a.to_string(),
        "tokenBId": pair.token_b.to_string(),
        "maxAmountARaw": amount_a.to_string(),
        "maxAmountBRaw": amount_b.to_string(),
        "actualAmountARaw": amount_a.to_string(),
        "actualAmountBRaw": amount_b.to_string(),
        "expectedLpRaw": expected_lp.to_string(),
        "lockedLpRaw": MINIMUM_LIQUIDITY.to_string(),
        "initialPriceRealRaw": spot_price_q64_64(amount_a, amount_b).to_string(),
        "minimumAmountARaw": minimum_a.to_string(),
        "minimumAmountBRaw": minimum_b.to_string(),
        "requiresFreshLp": true,
        "accountPreview": preview,
        "errors": funding,
        "warnings": [],
    });
    let plan = if can_submit {
        Some(NewPositionPlan::new(
            account_plan,
            QuoteBranch::Missing { amount_a, amount_b },
        )?)
    } else {
        None
    };
    Ok(QuoteComputation::Evaluated(EvaluatedQuote {
        value,
        quote_hash,
        plan,
    }))
}

fn compute_active_quote(
    input: &QuoteRequest,
    amm_program: ProgramId,
    pair: PairIds,
    pool_account: Account,
) -> Result<QuoteComputation, String> {
    if pool_account.program_owner != amm_program {
        return Ok(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "owner_mismatch" }),
        ));
    }
    let Ok(pool) = PoolDefinition::try_from(&pool_account.data) else {
        return Ok(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "invalid_pool_data" }),
        ));
    };
    let stored_reversed = if pool.definition_token_a_id == pair.token_a
        && pool.definition_token_b_id == pair.token_b
    {
        false
    } else if pool.definition_token_a_id == pair.token_b
        && pool.definition_token_b_id == pair.token_a
    {
        true
    } else {
        return Ok(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "pair_mismatch" }),
        ));
    };
    if pool.reserve_a == 0 || pool.reserve_b == 0 || pool.liquidity_pool_supply == 0 {
        return Ok(fatal_quote("pool_inactive", &[], json!({})));
    }
    if pool.fees != u128::from(input.request.fee_bps) {
        return Ok(fatal_quote(
            "fee_tier_mismatch",
            &["feeBps"],
            json!({ "poolFeeBps": pool.fees.to_string() }),
        ));
    }
    if !is_supported_fee_tier(pool.fees) {
        return Ok(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "unsupported_pool_fee" }),
        ));
    }

    let max_a = match raw_value(input.request.max_amount_a_raw.as_deref()) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                "amount_must_be_positive",
                &["maxAmountARaw"],
                json!({}),
            ))
        }
        Err(code) => return Ok(fatal_quote(code, &["maxAmountARaw"], json!({}))),
    };
    let max_b = match raw_value(input.request.max_amount_b_raw.as_deref()) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                "amount_must_be_positive",
                &["maxAmountBRaw"],
                json!({}),
            ))
        }
        Err(code) => return Ok(fatal_quote(code, &["maxAmountBRaw"], json!({}))),
    };
    let slippage_bps = input.request.slippage_bps.unwrap_or(DEFAULT_SLIPPAGE_BPS);
    if slippage_bps > MAX_SLIPPAGE_BPS {
        return Ok(fatal_quote(
            "invalid_slippage",
            &["slippageBps"],
            json!({ "maximum": MAX_SLIPPAGE_BPS }),
        ));
    }

    let (stored_max_a, stored_max_b) = if stored_reversed {
        (max_b, max_a)
    } else {
        (max_a, max_b)
    };
    let ideal_a = mul_div_floor(pool.reserve_a, stored_max_b, pool.reserve_b);
    let ideal_b = mul_div_floor(pool.reserve_b, stored_max_a, pool.reserve_a);
    let stored_actual_a = stored_max_a.min(ideal_a);
    let stored_actual_b = stored_max_b.min(ideal_b);
    if stored_actual_a == 0 || stored_actual_b == 0 {
        return Ok(fatal_quote(
            "amount_too_low",
            &["maxAmountARaw", "maxAmountBRaw"],
            json!({}),
        ));
    }
    let expected_lp =
        mul_div_floor(pool.liquidity_pool_supply, stored_actual_a, pool.reserve_a).min(
            mul_div_floor(pool.liquidity_pool_supply, stored_actual_b, pool.reserve_b),
        );
    if expected_lp == 0 {
        return Ok(fatal_quote(
            "amount_too_low",
            &["maxAmountARaw", "maxAmountBRaw"],
            json!({}),
        ));
    }
    let minimum_lp = mul_div_floor(
        expected_lp,
        FEE_BPS_DENOMINATOR - u128::from(slippage_bps),
        FEE_BPS_DENOMINATOR,
    );
    if minimum_lp == 0 {
        return Ok(fatal_quote("minimum_lp_zero", &["slippageBps"], json!({})));
    }
    let (actual_a, actual_b, reserve_a, reserve_b) = if stored_reversed {
        (
            stored_actual_b,
            stored_actual_a,
            pool.reserve_b,
            pool.reserve_a,
        )
    } else {
        (
            stored_actual_a,
            stored_actual_b,
            pool.reserve_a,
            pool.reserve_b,
        )
    };

    if let Some(error) = validate_active_accounts(input, pair, &pool, stored_reversed) {
        return Ok(error);
    }
    let holdings = wallet_holdings(&input.snapshot.wallet_accounts, pair.token_program);
    let holding_a = select_holding(&holdings, pair.token_a);
    let holding_b = select_holding(&holdings, pair.token_b);
    let lp_holding = select_holding(&holdings, pair.lp_definition);
    let requires_fresh_lp = lp_holding.is_none();
    let funding = funding_issues(
        input.snapshot.wallet_available,
        pair,
        &holding_a,
        actual_a,
        &holding_b,
        actual_b,
        ["maxAmountARaw", "maxAmountBRaw"],
    );
    let can_submit = funding.is_empty();
    let warnings = if slippage_bps >= HIGH_SLIPPAGE_BPS {
        vec![issue(
            "high_slippage",
            "High slippage tolerance.",
            &["slippageBps"],
            json!({ "slippageBps": slippage_bps }),
        )]
    } else {
        Vec::new()
    };
    let warning_codes = warnings
        .iter()
        .filter_map(|warning| warning["code"].as_str().map(String::from))
        .collect();
    let mut account_plan = active_account_plan(
        input,
        pair,
        amm_program,
        &pool,
        stored_reversed,
        AccountPlanHoldings {
            token_a: holding_a.as_ref(),
            token_b: holding_b.as_ref(),
            lp: lp_holding.as_ref(),
        },
    )?;
    let sources = account_plan.take_sources();
    let commitment = QuoteCommitment {
        schema: String::from(SCHEMA),
        network_id: input.network_id.clone(),
        network_fingerprint: input.network_fingerprint.clone(),
        amm_program_id: program_id_bytes(amm_program),
        token_a_id: pair.token_a.into_value(),
        token_b_id: pair.token_b.into_value(),
        fee_bps: input.request.fee_bps,
        pool_status: 1,
        request: RequestCommitment::Active {
            max_a,
            max_b,
            slippage_bps,
        },
        max_a,
        max_b,
        actual_a,
        actual_b,
        expected_lp,
        lp_guard: minimum_lp,
        requires_fresh_lp,
        sources,
        funding: funding_commitments(pair, &holding_a, actual_a, &holding_b, actual_b),
        warnings: warning_codes,
    };
    let quote_hash = hash_quote(&commitment)?;
    let preview = account_plan.preview();
    let value = json!({
        "schema": SCHEMA,
        "status": "ok",
        "canSubmit": can_submit,
        "code": if can_submit { "ready" } else { "funding_required" },
        "poolStatus": "active_pool",
        "instruction": "AddLiquidity",
        "quoteHash": quote_hash,
        "feeBps": input.request.fee_bps,
        "poolFeeBps": pool.fees.to_string(),
        "poolId": pair.pool.to_string(),
        "tokenAId": pair.token_a.to_string(),
        "tokenBId": pair.token_b.to_string(),
        "maxAmountARaw": max_a.to_string(),
        "maxAmountBRaw": max_b.to_string(),
        "actualAmountARaw": actual_a.to_string(),
        "actualAmountBRaw": actual_b.to_string(),
        "reserveARaw": reserve_a.to_string(),
        "reserveBRaw": reserve_b.to_string(),
        "liquiditySupplyRaw": pool.liquidity_pool_supply.to_string(),
        "expectedLpRaw": expected_lp.to_string(),
        "minimumLpRaw": minimum_lp.to_string(),
        "initialPriceRealRaw": spot_price_q64_64(reserve_a, reserve_b).to_string(),
        "requiresFreshLp": requires_fresh_lp,
        "accountPreview": preview,
        "errors": funding,
        "warnings": warnings,
    });
    let plan = if can_submit {
        Some(NewPositionPlan::new(
            account_plan,
            QuoteBranch::Active {
                max_a,
                max_b,
                minimum_lp,
                stored_reversed,
            },
        )?)
    } else {
        None
    };
    Ok(QuoteComputation::Evaluated(EvaluatedQuote {
        value,
        quote_hash,
        plan,
    }))
}

fn validate_active_accounts(
    input: &QuoteRequest,
    pair: PairIds,
    pool: &PoolDefinition,
    stored_reversed: bool,
) -> Option<QuoteComputation> {
    let expected_vault_a = if stored_reversed {
        pair.vault_b
    } else {
        pair.vault_a
    };
    let expected_vault_b = if stored_reversed {
        pair.vault_a
    } else {
        pair.vault_b
    };
    if pool.vault_a_id != expected_vault_a
        || pool.vault_b_id != expected_vault_b
        || pool.liquidity_pool_id != pair.lp_definition
    {
        return Some(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "pool_account_mismatch" }),
        ));
    }
    let (canonical_vault_a, canonical_vault_b) = (
        decode_holding(&input.snapshot.vault_a, pair.token_program, pair.token_a),
        decode_holding(&input.snapshot.vault_b, pair.token_program, pair.token_b),
    );
    let (Ok(vault_a_balance), Ok(vault_b_balance)) = (canonical_vault_a, canonical_vault_b) else {
        return Some(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": "vault" }),
        ));
    };
    let (reserve_a, reserve_b) = if stored_reversed {
        (pool.reserve_b, pool.reserve_a)
    } else {
        (pool.reserve_a, pool.reserve_b)
    };
    if vault_a_balance < reserve_a || vault_b_balance < reserve_b {
        return Some(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "vault_below_reserve" }),
        ));
    }
    let Ok((lp_id, lp_account)) = decode_account(&input.snapshot.lp_definition) else {
        return Some(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": "lp_definition" }),
        ));
    };
    if lp_id != pair.lp_definition
        || lp_account.program_owner != pair.token_program
        || !matches!(
            TokenDefinition::try_from(&lp_account.data),
            Ok(TokenDefinition::Fungible { .. })
        )
    {
        return Some(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "invalid_lp_definition" }),
        ));
    }
    let Ok((tick_id, tick_account)) = decode_account(&input.snapshot.current_tick) else {
        return Some(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": "current_tick" }),
        ));
    };
    if tick_id != pair.current_tick
        || tick_account.program_owner != pair.twap_program
        || CurrentTickAccount::try_from(&tick_account.data).is_err()
    {
        return Some(fatal_quote(
            "pool_unavailable",
            &[],
            json!({ "reason": "invalid_current_tick" }),
        ));
    }
    if !valid_clock(&input.snapshot.clock, pair.clock) {
        return Some(fatal_quote(
            "account_read_failed",
            &[],
            json!({ "role": "clock" }),
        ));
    }
    None
}

fn decode_holding(
    read: &AccountRead,
    token_program: ProgramId,
    definition_id: AccountId,
) -> Result<u128, String> {
    let holding = decode_fungible_holding(read, token_program)?;
    if holding.definition_id != definition_id {
        return Err(String::from("invalid fungible holding"));
    }
    Ok(holding.balance)
}

fn valid_clock(read: &AccountRead, expected_id: AccountId) -> bool {
    matches!(decode_clock(read), Ok((id, _)) if id == expected_id)
}

fn raw_value(value: Option<&str>) -> Result<u128, &'static str> {
    let Some(value) = value else {
        return Err("amount_required");
    };
    if value.is_empty() {
        return Err("amount_required");
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid_raw_amount");
    }
    value.parse().map_err(|_| "invalid_raw_amount")
}

pub(super) fn minimum_opening_pair(price: u128) -> Result<(u128, u128), String> {
    let minimum_initial_lp = U256::from(MINIMUM_LIQUIDITY + 1);
    let target_product = minimum_initial_lp
        .checked_mul(minimum_initial_lp)
        .ok_or_else(|| String::from("minimum liquidity product overflow"))?;
    if price >= Q64 {
        let amount_a = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_a| {
            let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
            U256::from(amount_a) * amount_b >= target_product
        });
        let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
        Ok((
            amount_a,
            u128::try_from(amount_b).map_err(|_| String::from("opening amount overflow"))?,
        ))
    } else {
        let amount_b = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_b| {
            let amount_a = div_ceil_u256(U256::from(amount_b) * U256::from(Q64), U256::from(price));
            amount_a * U256::from(amount_b) >= target_product
        });
        let amount_a = div_ceil_u256(U256::from(amount_b) * U256::from(Q64), U256::from(price));
        Ok((
            u128::try_from(amount_a).map_err(|_| String::from("opening amount overflow"))?,
            amount_b,
        ))
    }
}

fn binary_search_min(mut low: u128, mut high: u128, predicate: impl Fn(u128) -> bool) -> u128 {
    while low < high {
        let mid = low + (high - low) / 2;
        if predicate(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

pub(super) fn div_ceil_u256(numerator: U256, denominator: U256) -> U256 {
    numerator.div_ceil(denominator)
}
