use std::collections::{BTreeMap, BTreeSet};

use alloy_primitives::U256;
use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, is_supported_fee_tier, isqrt_product, mul_div_floor, spot_price_q64_64,
    AmmConfig, PoolDefinition, FEE_BPS_DENOMINATOR, FEE_TIER_BPS_1, FEE_TIER_BPS_100,
    FEE_TIER_BPS_30, FEE_TIER_BPS_5, MINIMUM_LIQUIDITY,
};
use borsh::BorshSerialize;
use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
    Commitment,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::{compute_current_tick_account_pda, CurrentTickAccount};

use crate::{
    account::{
        account_id_from_hex, account_id_hex, decode_account, parse_base58_id, parse_program_id,
        program_id_base58, program_id_bytes,
    },
    model::{
        AccountRead, ConfigIdRequest, ContextRequest, PairIdsRequest, PairSnapshot, PlanRequest,
        PositionRequest, QuoteRequest, TokenIdsRequest, SCHEMA,
    },
};

const DEADLINE_WINDOW_MS: u64 = 1_200_000;
const DEFAULT_SLIPPAGE_BPS: u32 = 50;
const MAX_SLIPPAGE_BPS: u32 = 5_000;
const HIGH_SLIPPAGE_BPS: u32 = 2_000;
const MIN_DEPOSIT_SCALE_BPS: u32 = 10_000;
const Q64: u128 = 1_u128 << 64;

#[derive(Clone)]
struct SelectedHolding {
    id: AccountId,
    balance: u128,
    account: Account,
}

#[derive(Clone)]
enum QuoteBranch {
    Missing {
        amount_a: u128,
        amount_b: u128,
    },
    Active {
        max_a: u128,
        max_b: u128,
        minimum_lp: u128,
        stored_reversed: bool,
    },
}

struct QuoteComputation {
    value: Value,
    quote_hash: Option<String>,
    can_submit: bool,
    requires_fresh_lp: bool,
    pair: Option<PairIds>,
    holding_a: Option<SelectedHolding>,
    holding_b: Option<SelectedHolding>,
    lp_holding: Option<SelectedHolding>,
    branch: Option<QuoteBranch>,
}

#[derive(Clone, Copy)]
struct PairIds {
    token_a: AccountId,
    token_b: AccountId,
    config: AccountId,
    pool: AccountId,
    vault_a: AccountId,
    vault_b: AccountId,
    lp_definition: AccountId,
    lp_lock_holding: AccountId,
    current_tick: AccountId,
    clock: AccountId,
    token_program: ProgramId,
    twap_program: ProgramId,
}

#[derive(BorshSerialize)]
enum RequestCommitment {
    Missing {
        initial_price: u128,
        deposit_scale_bps: u32,
    },
    Active {
        max_a: u128,
        max_b: u128,
        slippage_bps: u32,
    },
}

#[derive(BorshSerialize)]
struct SourceCommitment {
    role: String,
    commitment: [u8; 32],
}

#[derive(BorshSerialize)]
struct FundingCommitment {
    token_id: [u8; 32],
    holding_id: Option<[u8; 32]>,
    available: u128,
    requested: u128,
}

#[derive(BorshSerialize)]
struct QuoteCommitment {
    schema: String,
    network_id: String,
    network_fingerprint: String,
    amm_program_id: [u8; 32],
    token_a_id: [u8; 32],
    token_b_id: [u8; 32],
    fee_bps: u32,
    pool_status: u8,
    request: RequestCommitment,
    max_a: u128,
    max_b: u128,
    actual_a: u128,
    actual_b: u128,
    expected_lp: u128,
    lp_guard: u128,
    requires_fresh_lp: bool,
    sources: Vec<SourceCommitment>,
    funding: Vec<FundingCommitment>,
    warnings: Vec<String>,
}

pub fn config_id(request: ConfigIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    Ok(json!({
        "status": "ok",
        "configId": account_id_hex(compute_config_pda(amm_program)),
    }))
}

pub fn token_ids(request: TokenIdsRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let config_id = compute_config_pda(amm_program);
    let Ok((read_id, config_account)) = decode_account(&request.config) else {
        return Ok(manifest_error("config_unavailable"));
    };
    if read_id != config_id || config_account.program_owner != amm_program {
        return Ok(manifest_error("config_unavailable"));
    }
    let Ok(config) = AmmConfig::try_from(&config_account.data) else {
        return Ok(manifest_error("config_unavailable"));
    };

    let mut token_ids = BTreeSet::new();
    for id in &request.configured_token_ids {
        if let Ok(id) = account_id_from_hex(id, "configured token id") {
            token_ids.insert(id);
        }
    }
    for id in request
        .recent_token_ids
        .iter()
        .chain(&request.resolved_token_ids)
    {
        if let Ok(id) = parse_base58_id(id, "token id") {
            token_ids.insert(id);
        }
    }
    for read in &request.wallet_accounts {
        let Ok((_, account)) = decode_account(read) else {
            continue;
        };
        if account.program_owner != config.token_program_id {
            continue;
        }
        if let Ok(TokenHolding::Fungible { definition_id, .. }) =
            TokenHolding::try_from(&account.data)
        {
            token_ids.insert(definition_id);
        }
    }

    Ok(json!({
        "status": "ok",
        "tokenIds": token_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
    }))
}

fn manifest_error(code: &str) -> Value {
    json!({ "status": "error", "code": code, "tokenIds": [] })
}

pub fn pair_ids(request: PairIdsRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_a = parse_base58_id(&request.token_a_id, "token A id")?;
    let token_b = parse_base58_id(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Ok(json!({ "status": "error", "code": "same_token_pair" }));
    }
    if !is_canonical_pair(token_a, token_b) {
        return Ok(json!({ "status": "error", "code": "non_canonical_pair" }));
    }

    let pair = derive_pair(amm_program, token_a, token_b, &request.config)?;
    Ok(pair_json(pair))
}

fn is_canonical_pair(token_a: AccountId, token_b: AccountId) -> bool {
    token_a.value() > token_b.value()
}

fn derive_pair(
    amm_program: ProgramId,
    token_a: AccountId,
    token_b: AccountId,
    config_read: &AccountRead,
) -> Result<PairIds, String> {
    let config_id = compute_config_pda(amm_program);
    let (read_id, config_account) = decode_account(config_read)?;
    if read_id != config_id
        || config_account.program_owner != amm_program
        || config_account == Account::default()
    {
        return Err(String::from("AMM config is unavailable"));
    }
    let config = AmmConfig::try_from(&config_account.data)
        .map_err(|_| String::from("AMM config is invalid"))?;
    let pool = compute_pool_pda(amm_program, token_a, token_b);
    Ok(PairIds {
        token_a,
        token_b,
        config: config_id,
        pool,
        vault_a: compute_vault_pda(amm_program, pool, token_a),
        vault_b: compute_vault_pda(amm_program, pool, token_b),
        lp_definition: compute_liquidity_token_pda(amm_program, pool),
        lp_lock_holding: compute_lp_lock_holding_pda(amm_program, pool),
        current_tick: compute_current_tick_account_pda(config.twap_oracle_program_id, pool),
        clock: CLOCK_01_PROGRAM_ACCOUNT_ID,
        token_program: config.token_program_id,
        twap_program: config.twap_oracle_program_id,
    })
}

fn pair_json(pair: PairIds) -> Value {
    json!({
        "status": "ok",
        "tokenAId": account_id_hex(pair.token_a),
        "tokenBId": account_id_hex(pair.token_b),
        "configId": account_id_hex(pair.config),
        "poolId": account_id_hex(pair.pool),
        "vaultAId": account_id_hex(pair.vault_a),
        "vaultBId": account_id_hex(pair.vault_b),
        "lpDefinitionId": account_id_hex(pair.lp_definition),
        "lpLockHoldingId": account_id_hex(pair.lp_lock_holding),
        "currentTickId": account_id_hex(pair.current_tick),
        "clockId": account_id_hex(pair.clock),
    })
}

pub fn context(request: ContextRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let config_id = compute_config_pda(amm_program);
    let config = match decode_account(&request.config) {
        Ok((id, account))
            if id == config_id
                && account.program_owner == amm_program
                && account != Account::default() =>
        {
            AmmConfig::try_from(&account.data).ok()
        }
        _ => None,
    };
    let Some(config) = config else {
        return Ok(context_error(&request, "config_unavailable"));
    };

    let source_map = token_sources(&request);
    let holdings = wallet_holdings(&request.wallet_accounts, config.token_program_id);
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    for (token_id, sources) in source_map {
        let read = request
            .token_definitions
            .iter()
            .find(|read| account_id_from_hex(&read.id, "token definition id") == Ok(token_id));
        let Some(read) = read else {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_definition_unreadable",
            ));
            warnings.push(issue(
                "token_definition_unreadable",
                "Token definition could not be read.",
                &[],
                json!({ "tokenId": token_id.to_string() }),
            ));
            continue;
        };
        let Ok((read_id, account)) = decode_account(read) else {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_definition_unreadable",
            ));
            warnings.push(issue(
                "token_definition_unreadable",
                "Token definition could not be read.",
                &[],
                json!({ "tokenId": token_id.to_string() }),
            ));
            continue;
        };
        if read_id != token_id {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_definition_unreadable",
            ));
            continue;
        }
        if account.program_owner != config.token_program_id {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_program_mismatch",
            ));
            continue;
        }

        let Ok(definition) = TokenDefinition::try_from(&account.data) else {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_not_fungible",
            ));
            continue;
        };
        let TokenDefinition::Fungible {
            name,
            total_supply,
            metadata_id,
            ..
        } = definition
        else {
            rows.push(unavailable_token_row(
                token_id,
                sources,
                "token_not_fungible",
            ));
            continue;
        };

        let selected = select_holding(&holdings, token_id);
        let mut row = json!({
            "definitionId": token_id.to_string(),
            "name": name,
            "metadataId": metadata_id.map(|id| id.to_string()),
            "totalSupplyRaw": total_supply.to_string(),
            "ownerProgramId": program_id_base58(config.token_program_id),
            "public": true,
            "fungible": true,
            "selectable": true,
            "status": "available",
            "code": "available",
            "sources": sources,
        });
        if let Some(selected) = selected {
            row["holdingId"] = json!(selected.id.to_string());
            row["balanceRaw"] = json!(selected.balance.to_string());
        }
        rows.push(row);
    }

    rows.sort_by(|left, right| {
        let left_holding = left.get("holdingId").is_some();
        let right_holding = right.get("holdingId").is_some();
        right_holding.cmp(&left_holding).then_with(|| {
            left["definitionId"]
                .as_str()
                .cmp(&right["definitionId"].as_str())
        })
    });

    Ok(json!({
        "schema": SCHEMA,
        "status": if request.wallet_available { "ready" } else { "no_wallet" },
        "networkId": request.network_id,
        "networkFingerprint": request.network_fingerprint,
        "walletAvailable": request.wallet_available,
        "minimumLiquidityRaw": MINIMUM_LIQUIDITY.to_string(),
        "programIds": {
            "amm": program_id_base58(amm_program),
            "token": program_id_base58(config.token_program_id),
            "twapOracle": program_id_base58(config.twap_oracle_program_id),
        },
        "tokens": rows,
        "feeTiers": fee_tiers(),
        "warnings": warnings,
    }))
}

fn context_error(request: &ContextRequest, code: &str) -> Value {
    json!({
        "schema": SCHEMA,
        "status": "error",
        "code": code,
        "networkId": request.network_id,
        "networkFingerprint": request.network_fingerprint,
        "walletAvailable": request.wallet_available,
        "tokens": [],
        "feeTiers": fee_tiers(),
        "warnings": [],
    })
}

fn fee_tiers() -> Value {
    json!([
        { "feeBps": FEE_TIER_BPS_1, "label": "0.01%", "enabled": true },
        { "feeBps": FEE_TIER_BPS_5, "label": "0.05%", "enabled": true },
        { "feeBps": FEE_TIER_BPS_30, "label": "0.30%", "enabled": true },
        { "feeBps": FEE_TIER_BPS_100, "label": "1.00%", "enabled": true },
    ])
}

fn token_sources(request: &ContextRequest) -> BTreeMap<AccountId, Vec<String>> {
    let mut sources: BTreeMap<AccountId, BTreeSet<String>> = BTreeMap::new();
    for id in &request.configured_token_ids {
        if let Ok(id) = account_id_from_hex(id, "configured token id") {
            sources
                .entry(id)
                .or_default()
                .insert(String::from("configured"));
        }
    }
    for (ids, source) in [
        (&request.recent_token_ids, "recent"),
        (&request.resolved_token_ids, "resolved"),
    ] {
        for id in ids {
            if let Ok(id) = parse_base58_id(id, "token id") {
                sources.entry(id).or_default().insert(String::from(source));
            }
        }
    }
    for read in &request.wallet_accounts {
        let Ok((_, account)) = decode_account(read) else {
            continue;
        };
        if let Ok(TokenHolding::Fungible { definition_id, .. }) =
            TokenHolding::try_from(&account.data)
        {
            sources
                .entry(definition_id)
                .or_default()
                .insert(String::from("holding"));
        }
    }
    sources
        .into_iter()
        .map(|(id, values)| (id, values.into_iter().collect()))
        .collect()
}

fn unavailable_token_row(token_id: AccountId, sources: Vec<String>, code: &str) -> Value {
    json!({
        "definitionId": token_id.to_string(),
        "name": "",
        "metadataId": Value::Null,
        "totalSupplyRaw": "0",
        "selectable": false,
        "status": code,
        "code": code,
        "sources": sources,
    })
}

fn wallet_holdings(reads: &[AccountRead], token_program: ProgramId) -> Vec<SelectedHolding> {
    reads
        .iter()
        .filter_map(|read| {
            let (id, account) = decode_account(read).ok()?;
            if account.program_owner != token_program {
                return None;
            }
            let TokenHolding::Fungible { balance, .. } =
                TokenHolding::try_from(&account.data).ok()?
            else {
                return None;
            };
            Some(SelectedHolding {
                id,
                balance,
                account,
            })
        })
        .collect()
}

fn select_holding(
    holdings: &[SelectedHolding],
    definition_id: AccountId,
) -> Option<SelectedHolding> {
    holdings
        .iter()
        .filter(|holding| {
            matches!(
                TokenHolding::try_from(&holding.account.data),
                Ok(TokenHolding::Fungible { definition_id: id, .. }) if id == definition_id
            )
        })
        .max_by(|left, right| {
            left.balance
                .cmp(&right.balance)
                .then_with(|| right.id.cmp(&left.id))
        })
        .cloned()
}

pub fn quote(request: QuoteRequest) -> Result<Value, String> {
    Ok(compute_quote(&request)?.value)
}

fn compute_quote(input: &QuoteRequest) -> Result<QuoteComputation, String> {
    if input.request.schema != SCHEMA {
        return Ok(fatal_quote(
            &input.request,
            "unsupported_schema",
            &["schema"],
            json!({ "received": input.request.schema }),
        ));
    }
    let amm_program = parse_program_id(&input.amm_program_id)?;
    let token_a = match parse_base58_id(&input.request.token_a_id, "token A id") {
        Ok(id) => id,
        Err(_) => {
            return Ok(fatal_quote(
                &input.request,
                "invalid_token_id",
                &["tokenAId"],
                json!({}),
            ))
        }
    };
    let token_b = match parse_base58_id(&input.request.token_b_id, "token B id") {
        Ok(id) => id,
        Err(_) => {
            return Ok(fatal_quote(
                &input.request,
                "invalid_token_id",
                &["tokenBId"],
                json!({}),
            ))
        }
    };
    if token_a == token_b {
        return Ok(fatal_quote(
            &input.request,
            "same_token_pair",
            &["tokenAId", "tokenBId"],
            json!({}),
        ));
    }
    if !is_canonical_pair(token_a, token_b) {
        return Ok(fatal_quote(
            &input.request,
            "non_canonical_pair",
            &["tokenAId", "tokenBId"],
            json!({}),
        ));
    }
    if !is_supported_fee_tier(u128::from(input.request.fee_bps)) {
        return Ok(fatal_quote(
            &input.request,
            "invalid_fee_tier",
            &["feeBps"],
            json!({ "feeBps": input.request.fee_bps }),
        ));
    }

    let pair = match derive_pair(amm_program, token_a, token_b, &input.snapshot.config) {
        Ok(pair) => pair,
        Err(_) => {
            return Ok(fatal_quote(
                &input.request,
                "config_unavailable",
                &[],
                json!({}),
            ))
        }
    };
    if let Some(error) = validate_snapshot_ids(&pair, &input.snapshot) {
        return Ok(fatal_quote(
            &input.request,
            "account_read_failed",
            &[],
            json!({ "role": error }),
        ));
    }
    if let Some(error) = validate_token_definition(
        &input.snapshot.token_a,
        token_a,
        pair.token_program,
        "tokenAId",
    ) {
        return Ok(error_quote(&input.request, error));
    }
    if let Some(error) = validate_token_definition(
        &input.snapshot.token_b,
        token_b,
        pair.token_program,
        "tokenBId",
    ) {
        return Ok(error_quote(&input.request, error));
    }

    let (_, pool_account) = match decode_account(&input.snapshot.pool) {
        Ok(value) => value,
        Err(_) => {
            return Ok(fatal_quote(
                &input.request,
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

fn validate_snapshot_ids(pair: &PairIds, snapshot: &PairSnapshot) -> Option<&'static str> {
    let expected = [
        (&snapshot.config, pair.config, "config"),
        (&snapshot.token_a, pair.token_a, "token_a"),
        (&snapshot.token_b, pair.token_b, "token_b"),
        (&snapshot.pool, pair.pool, "pool"),
        (&snapshot.vault_a, pair.vault_a, "vault_a"),
        (&snapshot.vault_b, pair.vault_b, "vault_b"),
        (&snapshot.lp_definition, pair.lp_definition, "lp_definition"),
        (
            &snapshot.lp_lock_holding,
            pair.lp_lock_holding,
            "lp_lock_holding",
        ),
        (&snapshot.current_tick, pair.current_tick, "current_tick"),
        (&snapshot.clock, pair.clock, "clock"),
    ];
    expected.into_iter().find_map(|(read, id, role)| {
        match account_id_from_hex(&read.id, "account id") {
            Ok(actual) if actual == id => None,
            _ => Some(role),
        }
    })
}

struct DomainError {
    code: &'static str,
    field: &'static str,
    details: Value,
}

fn validate_token_definition(
    read: &AccountRead,
    token_id: AccountId,
    token_program: ProgramId,
    field: &'static str,
) -> Option<DomainError> {
    let Ok((id, account)) = decode_account(read) else {
        return Some(DomainError {
            code: "token_definition_unreadable",
            field,
            details: json!({ "tokenId": token_id.to_string() }),
        });
    };
    if id != token_id {
        return Some(DomainError {
            code: "token_definition_unreadable",
            field,
            details: json!({ "tokenId": token_id.to_string() }),
        });
    }
    if account.program_owner != token_program {
        return Some(DomainError {
            code: "token_program_mismatch",
            field,
            details: json!({ "tokenId": token_id.to_string() }),
        });
    }
    if !matches!(
        TokenDefinition::try_from(&account.data),
        Ok(TokenDefinition::Fungible { .. })
    ) {
        return Some(DomainError {
            code: "token_not_fungible",
            field,
            details: json!({ "tokenId": token_id.to_string() }),
        });
    }
    None
}

fn error_quote(request: &PositionRequest, error: DomainError) -> QuoteComputation {
    fatal_quote(request, error.code, &[error.field], error.details)
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
                &input.request,
                "account_read_failed",
                &[],
                json!({ "role": role }),
            ));
        };
        if account != Account::default() {
            return Ok(fatal_quote(
                &input.request,
                "pool_unavailable",
                &[],
                json!({ "role": role }),
            ));
        }
    }
    if !valid_clock(&input.snapshot.clock, pair.clock) {
        return Ok(fatal_quote(
            &input.request,
            "account_read_failed",
            &[],
            json!({ "role": "clock" }),
        ));
    }

    let initial_price = match raw_value(
        input.request.initial_price_real_raw.as_deref(),
        "initialPriceRealRaw",
    ) {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                &input.request,
                "amount_must_be_positive",
                &["initialPriceRealRaw"],
                json!({}),
            ))
        }
        Err(code) => {
            return Ok(fatal_quote(
                &input.request,
                code,
                &["initialPriceRealRaw"],
                json!({}),
            ))
        }
    };
    let Some(scale_bps) = input.request.deposit_scale_bps else {
        return Ok(fatal_quote(
            &input.request,
            "invalid_deposit_scale",
            &["depositScaleBps"],
            json!({}),
        ));
    };
    if scale_bps < MIN_DEPOSIT_SCALE_BPS {
        return Ok(fatal_quote(
            &input.request,
            "invalid_deposit_scale",
            &["depositScaleBps"],
            json!({ "minimum": MIN_DEPOSIT_SCALE_BPS }),
        ));
    }

    let (base_a, base_b) = minimum_opening_pair(initial_price)?;
    let Some(amount_a) = scale_amount(base_a, scale_bps) else {
        return Ok(fatal_quote(
            &input.request,
            "amount_overflow",
            &["depositScaleBps"],
            json!({}),
        ));
    };
    let Some(amount_b) = scale_amount(base_b, scale_bps) else {
        return Ok(fatal_quote(
            &input.request,
            "amount_overflow",
            &["depositScaleBps"],
            json!({}),
        ));
    };
    let initial_lp = isqrt_product(amount_a, amount_b);
    if initial_lp <= MINIMUM_LIQUIDITY {
        return Ok(fatal_quote(
            &input.request,
            "amount_too_low",
            &["depositScaleBps"],
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
        "depositScaleBps",
    );
    let max_funded_scale = max_funded_scale(
        input.snapshot.wallet_available,
        base_a,
        base_b,
        holding_a.as_ref(),
        holding_b.as_ref(),
    );
    let can_submit = funding.is_empty() && input.snapshot.wallet_available;
    let sources = quote_sources_missing(input, &holding_a, &holding_b)?;
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
        request: RequestCommitment::Missing {
            initial_price,
            deposit_scale_bps: scale_bps,
        },
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
    let preview = missing_preview(pair, amm_program, holding_a.as_ref(), holding_b.as_ref());
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
        "initialPriceRealRaw": initial_price.to_string(),
        "depositScaleBps": scale_bps,
        "maxFundedScaleBps": max_funded_scale,
        "requiresFreshLp": true,
        "accountPreview": preview,
        "errors": funding,
        "warnings": [],
    });
    Ok(QuoteComputation {
        value,
        quote_hash: Some(quote_hash),
        can_submit,
        requires_fresh_lp: true,
        pair: Some(pair),
        holding_a,
        holding_b,
        lp_holding: None,
        branch: Some(QuoteBranch::Missing { amount_a, amount_b }),
    })
}

fn compute_active_quote(
    input: &QuoteRequest,
    amm_program: ProgramId,
    pair: PairIds,
    pool_account: Account,
) -> Result<QuoteComputation, String> {
    if pool_account.program_owner != amm_program {
        return Ok(fatal_quote(
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "owner_mismatch" }),
        ));
    }
    let Ok(pool) = PoolDefinition::try_from(&pool_account.data) else {
        return Ok(fatal_quote(
            &input.request,
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
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "pair_mismatch" }),
        ));
    };
    if pool.reserve_a == 0 || pool.reserve_b == 0 || pool.liquidity_pool_supply == 0 {
        return Ok(fatal_quote(&input.request, "pool_inactive", &[], json!({})));
    }
    if pool.fees != u128::from(input.request.fee_bps) {
        return Ok(fatal_quote(
            &input.request,
            "fee_tier_mismatch",
            &["feeBps"],
            json!({ "poolFeeBps": pool.fees.to_string() }),
        ));
    }
    if !is_supported_fee_tier(pool.fees) {
        return Ok(fatal_quote(
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "unsupported_pool_fee" }),
        ));
    }

    let max_a = match raw_value(input.request.max_amount_a_raw.as_deref(), "maxAmountARaw") {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                &input.request,
                "amount_must_be_positive",
                &["maxAmountARaw"],
                json!({}),
            ))
        }
        Err(code) => {
            return Ok(fatal_quote(
                &input.request,
                code,
                &["maxAmountARaw"],
                json!({}),
            ))
        }
    };
    let max_b = match raw_value(input.request.max_amount_b_raw.as_deref(), "maxAmountBRaw") {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            return Ok(fatal_quote(
                &input.request,
                "amount_must_be_positive",
                &["maxAmountBRaw"],
                json!({}),
            ))
        }
        Err(code) => {
            return Ok(fatal_quote(
                &input.request,
                code,
                &["maxAmountBRaw"],
                json!({}),
            ))
        }
    };
    let slippage_bps = input.request.slippage_bps.unwrap_or(DEFAULT_SLIPPAGE_BPS);
    if slippage_bps > MAX_SLIPPAGE_BPS {
        return Ok(fatal_quote(
            &input.request,
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
            &input.request,
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
            &input.request,
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
        return Ok(fatal_quote(
            &input.request,
            "minimum_lp_zero",
            &["slippageBps"],
            json!({}),
        ));
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
        max_a,
        &holding_b,
        max_b,
        "",
    );
    let can_submit = funding.is_empty() && input.snapshot.wallet_available;
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
    let sources = quote_sources_active(input, &holding_a, &holding_b, &lp_holding)?;
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
        funding: funding_commitments(pair, &holding_a, max_a, &holding_b, max_b),
        warnings: warning_codes,
    };
    let quote_hash = hash_quote(&commitment)?;
    let preview = active_preview(
        pair,
        amm_program,
        &pool,
        stored_reversed,
        holding_a.as_ref(),
        holding_b.as_ref(),
        lp_holding.as_ref(),
    );
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
    Ok(QuoteComputation {
        value,
        quote_hash: Some(quote_hash),
        can_submit,
        requires_fresh_lp,
        pair: Some(pair),
        holding_a,
        holding_b,
        lp_holding,
        branch: Some(QuoteBranch::Active {
            max_a,
            max_b,
            minimum_lp,
            stored_reversed,
        }),
    })
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
            &input.request,
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
            &input.request,
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
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "vault_below_reserve" }),
        ));
    }
    let Ok((lp_id, lp_account)) = decode_account(&input.snapshot.lp_definition) else {
        return Some(fatal_quote(
            &input.request,
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
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "invalid_lp_definition" }),
        ));
    }
    let Ok((tick_id, tick_account)) = decode_account(&input.snapshot.current_tick) else {
        return Some(fatal_quote(
            &input.request,
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
            &input.request,
            "pool_unavailable",
            &[],
            json!({ "reason": "invalid_current_tick" }),
        ));
    }
    if !valid_clock(&input.snapshot.clock, pair.clock) {
        return Some(fatal_quote(
            &input.request,
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
    let (_, account) = decode_account(read)?;
    if account.program_owner != token_program {
        return Err(String::from("holding owner mismatch"));
    }
    match TokenHolding::try_from(&account.data) {
        Ok(TokenHolding::Fungible {
            definition_id: id,
            balance,
        }) if id == definition_id => Ok(balance),
        _ => Err(String::from("invalid fungible holding")),
    }
}

fn valid_clock(read: &AccountRead, expected_id: AccountId) -> bool {
    let Ok((id, account)) = decode_account(read) else {
        return false;
    };
    id == expected_id && borsh::from_slice::<ClockAccountData>(account.data.as_ref()).is_ok()
}

fn raw_value(value: Option<&str>, _field: &str) -> Result<u128, &'static str> {
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

fn minimum_opening_pair(price: u128) -> Result<(u128, u128), String> {
    let target_product = U256::from(MINIMUM_LIQUIDITY)
        .checked_mul(U256::from(MINIMUM_LIQUIDITY))
        .ok_or_else(|| String::from("minimum liquidity product overflow"))?;
    if price >= Q64 {
        let amount_a = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_a| {
            let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
            U256::from(amount_a) * amount_b > target_product
        });
        let amount_b = div_ceil_u256(U256::from(amount_a) * U256::from(price), U256::from(Q64));
        Ok((
            amount_a,
            u128::try_from(amount_b).map_err(|_| String::from("opening amount overflow"))?,
        ))
    } else {
        let amount_b = binary_search_min(1, MINIMUM_LIQUIDITY + 1, |amount_b| {
            let amount_a = div_ceil_u256(U256::from(amount_b) * U256::from(Q64), U256::from(price));
            amount_a * U256::from(amount_b) > target_product
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

fn div_ceil_u256(numerator: U256, denominator: U256) -> U256 {
    numerator.div_ceil(denominator)
}

fn scale_amount(base: u128, scale_bps: u32) -> Option<u128> {
    let scaled = div_ceil_u256(
        U256::from(base) * U256::from(scale_bps),
        U256::from(MIN_DEPOSIT_SCALE_BPS),
    );
    u128::try_from(scaled).ok()
}

fn max_funded_scale(
    wallet_available: bool,
    base_a: u128,
    base_b: u128,
    holding_a: Option<&SelectedHolding>,
    holding_b: Option<&SelectedHolding>,
) -> Value {
    if !wallet_available {
        return Value::Null;
    }
    let (Some(holding_a), Some(holding_b)) = (holding_a, holding_b) else {
        return json!(0);
    };
    let scale_a =
        U256::from(holding_a.balance) * U256::from(MIN_DEPOSIT_SCALE_BPS) / U256::from(base_a);
    let scale_b =
        U256::from(holding_b.balance) * U256::from(MIN_DEPOSIT_SCALE_BPS) / U256::from(base_b);
    let funded = scale_a.min(scale_b);
    if funded < U256::from(MIN_DEPOSIT_SCALE_BPS) {
        return json!(0);
    }
    let capped = funded.min(U256::from(u32::MAX));
    json!(u32::try_from(capped).unwrap_or(u32::MAX))
}

fn funding_issues(
    wallet_available: bool,
    pair: PairIds,
    holding_a: &Option<SelectedHolding>,
    requested_a: u128,
    holding_b: &Option<SelectedHolding>,
    requested_b: u128,
    derived_field: &str,
) -> Vec<Value> {
    if !wallet_available {
        return vec![issue(
            "no_wallet",
            "Connect a wallet to submit.",
            &[],
            json!({}),
        )];
    }
    let mut errors = Vec::new();
    for (token_id, holding, requested, field) in [
        (
            pair.token_a,
            holding_a,
            requested_a,
            if derived_field.is_empty() {
                "maxAmountARaw"
            } else {
                derived_field
            },
        ),
        (
            pair.token_b,
            holding_b,
            requested_b,
            if derived_field.is_empty() {
                "maxAmountBRaw"
            } else {
                derived_field
            },
        ),
    ] {
        let available = holding.as_ref().map_or(0, |value| value.balance);
        if available < requested {
            errors.push(issue(
                "amount_exceeds_balance",
                "Amount exceeds the selected wallet holding balance.",
                &[field],
                json!({
                    "requestedRaw": requested.to_string(),
                    "availableRaw": available.to_string(),
                    "holdingFound": holding.is_some(),
                    "tokenId": token_id.to_string(),
                }),
            ));
        }
    }
    errors
}

fn funding_commitments(
    pair: PairIds,
    holding_a: &Option<SelectedHolding>,
    requested_a: u128,
    holding_b: &Option<SelectedHolding>,
    requested_b: u128,
) -> Vec<FundingCommitment> {
    [
        (pair.token_a, holding_a, requested_a),
        (pair.token_b, holding_b, requested_b),
    ]
    .into_iter()
    .map(|(token_id, holding, requested)| FundingCommitment {
        token_id: token_id.into_value(),
        holding_id: holding.as_ref().map(|value| value.id.into_value()),
        available: holding.as_ref().map_or(0, |value| value.balance),
        requested,
    })
    .collect()
}

fn quote_sources_missing(
    input: &QuoteRequest,
    holding_a: &Option<SelectedHolding>,
    holding_b: &Option<SelectedHolding>,
) -> Result<Vec<SourceCommitment>, String> {
    let mut sources = sources_from_reads(&[
        ("config", &input.snapshot.config),
        ("token_a", &input.snapshot.token_a),
        ("token_b", &input.snapshot.token_b),
        ("pool", &input.snapshot.pool),
        ("vault_a", &input.snapshot.vault_a),
        ("vault_b", &input.snapshot.vault_b),
        ("lp_definition", &input.snapshot.lp_definition),
        ("lp_lock_holding", &input.snapshot.lp_lock_holding),
        ("current_tick", &input.snapshot.current_tick),
    ])?;
    append_holding_source(&mut sources, "holding_a", holding_a);
    append_holding_source(&mut sources, "holding_b", holding_b);
    Ok(sources)
}

fn quote_sources_active(
    input: &QuoteRequest,
    holding_a: &Option<SelectedHolding>,
    holding_b: &Option<SelectedHolding>,
    lp_holding: &Option<SelectedHolding>,
) -> Result<Vec<SourceCommitment>, String> {
    let mut sources = sources_from_reads(&[
        ("config", &input.snapshot.config),
        ("token_a", &input.snapshot.token_a),
        ("token_b", &input.snapshot.token_b),
        ("pool", &input.snapshot.pool),
        ("vault_a", &input.snapshot.vault_a),
        ("vault_b", &input.snapshot.vault_b),
        ("lp_definition", &input.snapshot.lp_definition),
        ("current_tick", &input.snapshot.current_tick),
    ])?;
    append_holding_source(&mut sources, "holding_a", holding_a);
    append_holding_source(&mut sources, "holding_b", holding_b);
    append_holding_source(&mut sources, "holding_lp", lp_holding);
    Ok(sources)
}

fn sources_from_reads(reads: &[(&str, &AccountRead)]) -> Result<Vec<SourceCommitment>, String> {
    reads
        .iter()
        .map(|(role, read)| {
            let (id, account) = decode_account(read)?;
            Ok(SourceCommitment {
                role: String::from(*role),
                commitment: Commitment::new(&id, &account).to_byte_array(),
            })
        })
        .collect()
}

fn append_holding_source(
    sources: &mut Vec<SourceCommitment>,
    role: &str,
    holding: &Option<SelectedHolding>,
) {
    if let Some(holding) = holding {
        sources.push(SourceCommitment {
            role: String::from(role),
            commitment: Commitment::new(&holding.id, &holding.account).to_byte_array(),
        });
    }
}

fn hash_quote(commitment: &QuoteCommitment) -> Result<String, String> {
    let bytes = borsh::to_vec(commitment)
        .map_err(|error| format!("quote commitment serialization failed: {error}"))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn issue(code: &str, message: &str, fields: &[&str], details: Value) -> Value {
    json!({
        "code": code,
        "message": message,
        "details": details,
        "recoverable": true,
        "blockingFields": fields,
    })
}

fn fatal_quote(
    request: &PositionRequest,
    code: &str,
    fields: &[&str],
    details: Value,
) -> QuoteComputation {
    QuoteComputation {
        value: json!({
            "schema": SCHEMA,
            "status": "error",
            "canSubmit": false,
            "code": code,
            "poolStatus": "unavailable_pool",
            "tokenAId": request.token_a_id,
            "tokenBId": request.token_b_id,
            "accountPreview": [],
            "errors": [issue(code, "Position quote is unavailable.", fields, details)],
            "warnings": [],
        }),
        quote_hash: None,
        can_submit: false,
        requires_fresh_lp: false,
        pair: None,
        holding_a: None,
        holding_b: None,
        lp_holding: None,
        branch: None,
    }
}

fn preview_row(
    order: usize,
    role: &str,
    account_id: Option<AccountId>,
    expected_program: Option<ProgramId>,
    action: &str,
    signer: bool,
    init: bool,
) -> Value {
    json!({
        "order": order,
        "role": role,
        "accountId": account_id.map(|id| id.to_string()),
        "expectedProgramId": expected_program.map(program_id_base58),
        "action": action,
        "writable": action != "read",
        "signer": signer,
        "init": init,
    })
}

fn missing_preview(
    pair: PairIds,
    amm_program: ProgramId,
    holding_a: Option<&SelectedHolding>,
    holding_b: Option<&SelectedHolding>,
) -> Vec<Value> {
    vec![
        preview_row(
            0,
            "config",
            Some(pair.config),
            Some(amm_program),
            "read",
            false,
            false,
        ),
        preview_row(
            1,
            "pool",
            Some(pair.pool),
            Some(amm_program),
            "create",
            false,
            true,
        ),
        preview_row(
            2,
            "vault_a",
            Some(pair.vault_a),
            Some(pair.token_program),
            "create",
            false,
            true,
        ),
        preview_row(
            3,
            "vault_b",
            Some(pair.vault_b),
            Some(pair.token_program),
            "create",
            false,
            true,
        ),
        preview_row(
            4,
            "lp_definition",
            Some(pair.lp_definition),
            Some(pair.token_program),
            "create",
            false,
            true,
        ),
        preview_row(
            5,
            "lp_lock_holding",
            Some(pair.lp_lock_holding),
            Some(pair.token_program),
            "create",
            false,
            true,
        ),
        preview_row(
            6,
            "user_holding_a",
            holding_a.map(|value| value.id),
            Some(pair.token_program),
            "update",
            true,
            false,
        ),
        preview_row(
            7,
            "user_holding_b",
            holding_b.map(|value| value.id),
            Some(pair.token_program),
            "update",
            true,
            false,
        ),
        preview_row(
            8,
            "user_holding_lp",
            None,
            Some(pair.token_program),
            "create",
            true,
            true,
        ),
        preview_row(
            9,
            "current_tick",
            Some(pair.current_tick),
            Some(pair.twap_program),
            "create",
            false,
            true,
        ),
        preview_row(10, "clock", Some(pair.clock), None, "read", false, false),
    ]
}

fn active_preview(
    pair: PairIds,
    amm_program: ProgramId,
    pool: &PoolDefinition,
    stored_reversed: bool,
    holding_a: Option<&SelectedHolding>,
    holding_b: Option<&SelectedHolding>,
    lp_holding: Option<&SelectedHolding>,
) -> Vec<Value> {
    let (stored_holding_a, stored_holding_b) = if stored_reversed {
        (holding_b, holding_a)
    } else {
        (holding_a, holding_b)
    };
    vec![
        preview_row(
            0,
            "config",
            Some(pair.config),
            Some(amm_program),
            "read",
            false,
            false,
        ),
        preview_row(
            1,
            "pool",
            Some(pair.pool),
            Some(amm_program),
            "update",
            false,
            false,
        ),
        preview_row(
            2,
            "vault_a",
            Some(pool.vault_a_id),
            Some(pair.token_program),
            "update",
            false,
            false,
        ),
        preview_row(
            3,
            "vault_b",
            Some(pool.vault_b_id),
            Some(pair.token_program),
            "update",
            false,
            false,
        ),
        preview_row(
            4,
            "lp_definition",
            Some(pair.lp_definition),
            Some(pair.token_program),
            "update",
            false,
            false,
        ),
        preview_row(
            5,
            "user_holding_a",
            stored_holding_a.map(|value| value.id),
            Some(pair.token_program),
            "update",
            true,
            false,
        ),
        preview_row(
            6,
            "user_holding_b",
            stored_holding_b.map(|value| value.id),
            Some(pair.token_program),
            "update",
            true,
            false,
        ),
        preview_row(
            7,
            "user_holding_lp",
            lp_holding.map(|value| value.id),
            Some(pair.token_program),
            if lp_holding.is_some() {
                "update"
            } else {
                "create"
            },
            lp_holding.is_none(),
            lp_holding.is_none(),
        ),
        preview_row(
            8,
            "current_tick",
            Some(pair.current_tick),
            Some(pair.twap_program),
            "update",
            false,
            false,
        ),
        preview_row(9, "clock", Some(pair.clock), None, "read", false, false),
    ]
}

pub fn plan(input: PlanRequest) -> Result<Value, String> {
    let quote_input = QuoteRequest {
        network_id: input.network_id,
        network_fingerprint: input.network_fingerprint,
        amm_program_id: input.amm_program_id.clone(),
        request: input.request,
        snapshot: input.snapshot,
    };
    let quote = compute_quote(&quote_input)?;
    if quote.quote_hash.as_deref() != Some(input.quote_hash.as_str()) {
        return Ok(json!({
            "schema": SCHEMA,
            "status": "error",
            "code": "quote_changed",
            "recoverable": true,
            "quote": quote.value,
        }));
    }
    if !quote.can_submit {
        return Ok(json!({
            "schema": SCHEMA,
            "status": "error",
            "code": "quote_not_submittable",
            "recoverable": true,
            "quote": quote.value,
        }));
    }
    let pair = quote
        .pair
        .ok_or_else(|| String::from("submittable quote has no pair"))?;
    let holding_a = quote
        .holding_a
        .ok_or_else(|| String::from("submittable quote has no token A holding"))?;
    let holding_b = quote
        .holding_b
        .ok_or_else(|| String::from("submittable quote has no token B holding"))?;
    let fresh_lp = if quote.requires_fresh_lp {
        let Some(read) = input.fresh_lp.as_ref() else {
            return Ok(json!({
                "schema": SCHEMA,
                "status": "needs_fresh_lp",
                "code": "fresh_lp_required",
            }));
        };
        let Ok((id, account)) = decode_account(read) else {
            return Ok(plan_error("wallet_submission_failed"));
        };
        if account != Account::default() {
            return Ok(plan_error("wallet_submission_failed"));
        }
        Some(id)
    } else {
        None
    };
    let lp_id = quote
        .lp_holding
        .as_ref()
        .map(|holding| holding.id)
        .or(fresh_lp)
        .ok_or_else(|| String::from("transaction plan has no LP holding"))?;
    let deadline = input
        .now_ms
        .checked_add(DEADLINE_WINDOW_MS)
        .ok_or_else(|| String::from("transaction deadline overflow"))?;
    let clock_timestamp = clock_timestamp(&quote_input.snapshot.clock)?;
    if clock_timestamp >= deadline {
        return Ok(plan_error("transaction_deadline_expired"));
    }
    let amm_program = parse_program_id(&input.amm_program_id)?;

    let (account_ids, signing_requirements, instruction) = match quote
        .branch
        .ok_or_else(|| String::from("submittable quote has no branch"))?
    {
        QuoteBranch::Missing { amount_a, amount_b } => {
            let instruction = amm_core::Instruction::NewDefinition {
                token_a_amount: amount_a,
                token_b_amount: amount_b,
                fees: u128::from(quote_input.request.fee_bps),
                deadline,
            };
            (
                vec![
                    pair.config,
                    pair.pool,
                    pair.vault_a,
                    pair.vault_b,
                    pair.lp_definition,
                    pair.lp_lock_holding,
                    holding_a.id,
                    holding_b.id,
                    lp_id,
                    pair.current_tick,
                    pair.clock,
                ],
                vec![
                    false, false, false, false, false, false, true, true, true, false, false,
                ],
                risc0_zkvm::serde::to_vec(&instruction)
                    .map_err(|error| format!("instruction serialization failed: {error}"))?,
            )
        }
        QuoteBranch::Active {
            max_a,
            max_b,
            minimum_lp,
            stored_reversed,
        } => {
            let (_, pool_account) = decode_account(&quote_input.snapshot.pool)?;
            let pool = PoolDefinition::try_from(&pool_account.data)
                .map_err(|_| String::from("active pool data changed"))?;
            let (stored_max_a, stored_max_b, stored_holding_a, stored_holding_b) =
                if stored_reversed {
                    (max_b, max_a, holding_b.id, holding_a.id)
                } else {
                    (max_a, max_b, holding_a.id, holding_b.id)
                };
            let instruction = amm_core::Instruction::AddLiquidity {
                min_amount_liquidity: minimum_lp,
                max_amount_to_add_token_a: stored_max_a,
                max_amount_to_add_token_b: stored_max_b,
                deadline,
            };
            (
                vec![
                    pair.config,
                    pair.pool,
                    pool.vault_a_id,
                    pool.vault_b_id,
                    pair.lp_definition,
                    stored_holding_a,
                    stored_holding_b,
                    lp_id,
                    pair.current_tick,
                    pair.clock,
                ],
                vec![
                    false,
                    false,
                    false,
                    false,
                    false,
                    true,
                    true,
                    fresh_lp.is_some(),
                    false,
                    false,
                ],
                risc0_zkvm::serde::to_vec(&instruction)
                    .map_err(|error| format!("instruction serialization failed: {error}"))?,
            )
        }
    };

    Ok(json!({
        "schema": SCHEMA,
        "status": "ready",
        "programId": input.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
        "deadlineMs": deadline.to_string(),
        "ammProgramId": program_id_base58(amm_program),
    }))
}

fn plan_error(code: &str) -> Value {
    json!({
        "schema": SCHEMA,
        "status": "error",
        "code": code,
        "recoverable": true,
    })
}

fn clock_timestamp(read: &AccountRead) -> Result<u64, String> {
    let (_, account) = decode_account(read)?;
    borsh::from_slice::<ClockAccountData>(account.data.as_ref())
        .map(|clock| clock.timestamp)
        .map_err(|error| format!("invalid clock account: {error}"))
}

#[cfg(test)]
mod tests {
    use nssa_core::account::{Data, Nonce};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::account::account_read;

    const AMM_PROGRAM: ProgramId = [11; 8];
    const TOKEN_PROGRAM: ProgramId = [22; 8];
    const TWAP_PROGRAM: ProgramId = [33; 8];

    fn account(owner: ProgramId, data: Data) -> Account {
        Account {
            program_owner: owner,
            balance: 0,
            data,
            nonce: Nonce(0),
        }
    }

    fn default_read(id: AccountId) -> AccountRead {
        account_read(id, &Account::default())
    }

    fn config_account() -> Account {
        account(
            AMM_PROGRAM,
            Data::from(&AmmConfig {
                token_program_id: TOKEN_PROGRAM,
                twap_oracle_program_id: TWAP_PROGRAM,
                authority: AccountId::new([7; 32]),
            }),
        )
    }

    fn token_definition(name: &str, supply: u128) -> Account {
        account(
            TOKEN_PROGRAM,
            Data::from(&TokenDefinition::Fungible {
                name: String::from(name),
                total_supply: supply,
                metadata_id: None,
                authority: None,
            }),
        )
    }

    fn token_holding(definition_id: AccountId, balance: u128) -> Account {
        account(
            TOKEN_PROGRAM,
            Data::from(&TokenHolding::Fungible {
                definition_id,
                balance,
            }),
        )
    }

    fn clock_account() -> Account {
        account(
            [44; 8],
            Data::try_from(
                ClockAccountData {
                    block_id: 10,
                    timestamp: 1_000,
                }
                .to_bytes(),
            )
            .unwrap(),
        )
    }

    fn ids() -> PairIds {
        let token_a = AccountId::new([2; 32]);
        let token_b = AccountId::new([1; 32]);
        let config = compute_config_pda(AMM_PROGRAM);
        let pool = compute_pool_pda(AMM_PROGRAM, token_a, token_b);
        PairIds {
            token_a,
            token_b,
            config,
            pool,
            vault_a: compute_vault_pda(AMM_PROGRAM, pool, token_a),
            vault_b: compute_vault_pda(AMM_PROGRAM, pool, token_b),
            lp_definition: compute_liquidity_token_pda(AMM_PROGRAM, pool),
            lp_lock_holding: compute_lp_lock_holding_pda(AMM_PROGRAM, pool),
            current_tick: compute_current_tick_account_pda(TWAP_PROGRAM, pool),
            clock: CLOCK_01_PROGRAM_ACCOUNT_ID,
            token_program: TOKEN_PROGRAM,
            twap_program: TWAP_PROGRAM,
        }
    }

    fn base_snapshot(pair: PairIds) -> PairSnapshot {
        let holding_a_id = AccountId::new([61; 32]);
        let holding_b_id = AccountId::new([62; 32]);
        PairSnapshot {
            config: account_read(pair.config, &config_account()),
            token_a: account_read(pair.token_a, &token_definition("A", 1_000_000)),
            token_b: account_read(pair.token_b, &token_definition("B", 2_000_000)),
            pool: default_read(pair.pool),
            vault_a: default_read(pair.vault_a),
            vault_b: default_read(pair.vault_b),
            lp_definition: default_read(pair.lp_definition),
            lp_lock_holding: default_read(pair.lp_lock_holding),
            current_tick: default_read(pair.current_tick),
            clock: account_read(pair.clock, &clock_account()),
            wallet_available: true,
            wallet_accounts: vec![
                account_read(holding_a_id, &token_holding(pair.token_a, 1_000_000)),
                account_read(holding_b_id, &token_holding(pair.token_b, 1_000_000)),
            ],
        }
    }

    fn request(pair: PairIds) -> PositionRequest {
        assert!(is_canonical_pair(pair.token_a, pair.token_b));
        PositionRequest {
            schema: String::from(SCHEMA),
            token_a_id: pair.token_a.to_string(),
            token_b_id: pair.token_b.to_string(),
            fee_bps: 30,
            max_amount_a_raw: None,
            max_amount_b_raw: None,
            slippage_bps: None,
            initial_price_real_raw: Some(Q64.to_string()),
            deposit_scale_bps: Some(MIN_DEPOSIT_SCALE_BPS),
        }
    }

    #[test]
    fn minimum_pair_exceeds_protocol_lock() {
        for price in [1, Q64 / 10, Q64, Q64 * 2, u128::MAX] {
            let (amount_a, amount_b) = minimum_opening_pair(price).unwrap();
            assert!(amount_a > 0);
            assert!(amount_b > 0);
            assert!(isqrt_product(amount_a, amount_b) > MINIMUM_LIQUIDITY);
        }
    }

    #[test]
    fn minimum_pair_is_minimal_on_price_base_side() {
        let (amount_a, amount_b) = minimum_opening_pair(Q64 * 2).unwrap();
        assert!(isqrt_product(amount_a, amount_b) > MINIMUM_LIQUIDITY);
        let previous_b = div_ceil_u256(
            U256::from(amount_a - 1) * U256::from(Q64 * 2),
            U256::from(Q64),
        );
        assert!(
            U256::from(amount_a - 1) * previous_b
                <= U256::from(MINIMUM_LIQUIDITY * MINIMUM_LIQUIDITY)
        );
    }

    #[test]
    fn highest_balance_holding_wins_then_lowest_id() {
        let definition = AccountId::new([9; 32]);
        let holding = |id: u8, balance| SelectedHolding {
            id: AccountId::new([id; 32]),
            balance,
            account: account(
                TOKEN_PROGRAM,
                Data::from(&TokenHolding::Fungible {
                    definition_id: definition,
                    balance,
                }),
            ),
        };
        let selected = select_holding(
            &[holding(4, 10), holding(2, 20), holding(1, 20)],
            definition,
        )
        .unwrap();
        assert_eq!(selected.id, AccountId::new([1; 32]));
    }

    #[test]
    fn pair_manifest_uses_canonical_ids_and_current_program_types() {
        let token_a = AccountId::new([2; 32]);
        let token_b = AccountId::new([1; 32]);
        let config_id = compute_config_pda(AMM_PROGRAM);
        let config = account(
            AMM_PROGRAM,
            Data::from(&AmmConfig {
                token_program_id: TOKEN_PROGRAM,
                twap_oracle_program_id: TWAP_PROGRAM,
                authority: AccountId::new([7; 32]),
            }),
        );
        let result = pair_ids(PairIdsRequest {
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            config: account_read(config_id, &config),
            token_a_id: token_a.to_string(),
            token_b_id: token_b.to_string(),
        })
        .unwrap();
        assert_eq!(result["status"], "ok");
        assert_eq!(result["tokenAId"], account_id_hex(token_a));
        assert_eq!(result["tokenBId"], account_id_hex(token_b));
        assert_eq!(
            result["poolId"],
            account_id_hex(compute_pool_pda(AMM_PROGRAM, token_a, token_b))
        );
    }

    #[test]
    fn context_selects_tokens_without_holdings() {
        let token_id = AccountId::new([3; 32]);
        let config_id = compute_config_pda(AMM_PROGRAM);
        let config = account(
            AMM_PROGRAM,
            Data::from(&AmmConfig {
                token_program_id: TOKEN_PROGRAM,
                twap_oracle_program_id: TWAP_PROGRAM,
                authority: AccountId::new([7; 32]),
            }),
        );
        let definition = account(
            TOKEN_PROGRAM,
            Data::from(&TokenDefinition::Fungible {
                name: String::from("Token"),
                total_supply: 1_000_000,
                metadata_id: None,
                authority: None,
            }),
        );
        let value = context(ContextRequest {
            network_id: String::from("testnet"),
            network_fingerprint: String::from("block10:abc"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            wallet_available: true,
            config: account_read(config_id, &config),
            wallet_accounts: Vec::new(),
            token_definitions: vec![account_read(token_id, &definition)],
            configured_token_ids: vec![account_id_hex(token_id)],
            recent_token_ids: Vec::new(),
            resolved_token_ids: Vec::new(),
        })
        .unwrap();
        assert_eq!(value["tokens"][0]["selectable"], true);
        assert!(value["tokens"][0].get("holdingId").is_none());
    }

    #[test]
    fn missing_pool_snapshot_defaults_remain_real_accounts() {
        let id = AccountId::new([5; 32]);
        let read = default_read(id);
        let (decoded_id, decoded) = decode_account(&read).unwrap();
        assert_eq!(decoded_id, id);
        assert_eq!(decoded, Account::default());
    }

    #[test]
    fn missing_pool_quote_and_plan_use_current_account_order() {
        let pair = ids();
        let snapshot = base_snapshot(pair);
        let request = request(pair);
        let quote_value = quote(QuoteRequest {
            network_id: String::from("devnet"),
            network_fingerprint: String::from("channel:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request: request.clone(),
            snapshot: snapshot.clone(),
        })
        .unwrap();
        assert_eq!(quote_value["status"], "ok");
        assert_eq!(quote_value["poolStatus"], "missing_pool");
        assert_eq!(quote_value["canSubmit"], true);
        assert_eq!(quote_value["accountPreview"].as_array().unwrap().len(), 11);
        let quote_hash = quote_value["quoteHash"].as_str().unwrap().to_owned();

        let fresh_lp = AccountId::new([63; 32]);
        let plan_value = plan(PlanRequest {
            network_id: String::from("devnet"),
            network_fingerprint: String::from("channel:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request,
            snapshot,
            quote_hash,
            now_ms: 2_000,
            fresh_lp: Some(default_read(fresh_lp)),
        })
        .unwrap();
        assert_eq!(plan_value["status"], "ready");
        assert_eq!(plan_value["accountIds"].as_array().unwrap().len(), 11);
        assert_eq!(plan_value["accountIds"][8], account_id_hex(fresh_lp));
        assert_eq!(plan_value["signingRequirements"][6], true);
        assert_eq!(plan_value["signingRequirements"][7], true);
        assert_eq!(plan_value["signingRequirements"][8], true);
    }

    #[test]
    fn advancing_clock_does_not_stale_quote() {
        let pair = ids();
        let request = request(pair);
        let snapshot = base_snapshot(pair);
        let quote_value = quote(QuoteRequest {
            network_id: String::from("testnet"),
            network_fingerprint: String::from("block10:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request: request.clone(),
            snapshot: snapshot.clone(),
        })
        .unwrap();

        let mut advanced = snapshot;
        advanced.clock = account_read(
            pair.clock,
            &account(
                [44; 8],
                Data::try_from(
                    ClockAccountData {
                        block_id: 11,
                        timestamp: 1_500,
                    }
                    .to_bytes(),
                )
                .unwrap(),
            ),
        );
        let plan_value = plan(PlanRequest {
            network_id: String::from("testnet"),
            network_fingerprint: String::from("block10:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request,
            snapshot: advanced,
            quote_hash: quote_value["quoteHash"].as_str().unwrap().to_owned(),
            now_ms: 2_000,
            fresh_lp: Some(default_read(AccountId::new([63; 32]))),
        })
        .unwrap();

        assert_eq!(plan_value["status"], "ready");
    }

    #[test]
    fn active_pool_quote_uses_ratio_and_existing_lp_holding() {
        let pair = ids();
        let mut snapshot = base_snapshot(pair);
        let pool = PoolDefinition {
            definition_token_a_id: pair.token_a,
            definition_token_b_id: pair.token_b,
            vault_a_id: pair.vault_a,
            vault_b_id: pair.vault_b,
            liquidity_pool_id: pair.lp_definition,
            liquidity_pool_supply: 10_000,
            reserve_a: 10_000,
            reserve_b: 20_000,
            fees: 30,
        };
        snapshot.pool = account_read(pair.pool, &account(AMM_PROGRAM, Data::from(&pool)));
        snapshot.vault_a = account_read(pair.vault_a, &token_holding(pair.token_a, pool.reserve_a));
        snapshot.vault_b = account_read(pair.vault_b, &token_holding(pair.token_b, pool.reserve_b));
        snapshot.lp_definition = account_read(
            pair.lp_definition,
            &account(
                TOKEN_PROGRAM,
                Data::from(&TokenDefinition::Fungible {
                    name: String::from("LP"),
                    total_supply: pool.liquidity_pool_supply,
                    metadata_id: None,
                    authority: Some(pair.lp_definition),
                }),
            ),
        );
        snapshot.current_tick = account_read(
            pair.current_tick,
            &account(
                TWAP_PROGRAM,
                Data::from(&CurrentTickAccount {
                    tick: 0,
                    last_updated: 1_000,
                }),
            ),
        );
        let lp_holding = AccountId::new([64; 32]);
        snapshot.wallet_accounts.push(account_read(
            lp_holding,
            &token_holding(pair.lp_definition, 500),
        ));
        let mut request = request(pair);
        request.initial_price_real_raw = None;
        request.deposit_scale_bps = None;
        request.max_amount_a_raw = Some(String::from("1000"));
        request.max_amount_b_raw = Some(String::from("3000"));
        request.slippage_bps = Some(50);

        let quote_value = quote(QuoteRequest {
            network_id: String::from("testnet"),
            network_fingerprint: String::from("block10:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request: request.clone(),
            snapshot: snapshot.clone(),
        })
        .unwrap();
        assert_eq!(quote_value["poolStatus"], "active_pool");
        assert_eq!(quote_value["actualAmountARaw"], "1000");
        assert_eq!(quote_value["actualAmountBRaw"], "2000");
        assert_eq!(quote_value["expectedLpRaw"], "1000");
        assert_eq!(quote_value["minimumLpRaw"], "995");
        assert_eq!(quote_value["requiresFreshLp"], false);

        let plan_value = plan(PlanRequest {
            network_id: String::from("testnet"),
            network_fingerprint: String::from("block10:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request,
            snapshot,
            quote_hash: quote_value["quoteHash"].as_str().unwrap().to_owned(),
            now_ms: 2_000,
            fresh_lp: None,
        })
        .unwrap();
        assert_eq!(plan_value["status"], "ready");
        assert_eq!(plan_value["accountIds"].as_array().unwrap().len(), 10);
        assert_eq!(plan_value["accountIds"][7], account_id_hex(lp_holding));
        assert_eq!(plan_value["signingRequirements"][7], false);
    }

    #[test]
    fn stale_hash_returns_recomputed_quote_without_plan() {
        let pair = ids();
        let value = plan(PlanRequest {
            network_id: String::from("devnet"),
            network_fingerprint: String::from("channel:test"),
            amm_program_id: hex::encode(program_id_bytes(AMM_PROGRAM)),
            request: request(pair),
            snapshot: base_snapshot(pair),
            quote_hash: String::from("sha256:deadbeef"),
            now_ms: 2_000,
            fresh_lp: None,
        })
        .unwrap();
        assert_eq!(value["status"], "error");
        assert_eq!(value["code"], "quote_changed");
        assert_eq!(value["quote"]["status"], "ok");
    }
}
