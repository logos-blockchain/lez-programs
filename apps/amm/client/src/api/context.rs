use std::collections::{BTreeMap, BTreeSet};

use amm_core::{
    FEE_TIER_BPS_1, FEE_TIER_BPS_100, FEE_TIER_BPS_30, FEE_TIER_BPS_5, MINIMUM_LIQUIDITY,
};
use nssa_core::{account::AccountId, program::ProgramId};
use serde_json::{json, Value};
use token_core::TokenDefinition;

use super::{
    config::load_config,
    holding::{select_holding, wallet_holdings, SelectedHolding},
    quote_error::issue,
    ContextRequest, TokenIdsRequest,
};
use crate::account::{
    account_id_from_hex, account_id_hex, decode_account, parse_base58_id, parse_program_id,
    program_id_base58, AccountRead,
};

pub(super) fn token_ids(request: TokenIdsRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok(config) = load_config(amm_program, &request.config) else {
        return Ok(manifest_error("config_unavailable"));
    };

    let holdings = wallet_holdings(&request.wallet_accounts, config.token_program_id);
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
    token_ids.extend(holdings.into_iter().map(|holding| holding.definition_id));

    Ok(json!({
        "status": "ok",
        "tokenIds": token_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
    }))
}

fn manifest_error(code: &str) -> Value {
    json!({ "status": "error", "code": code, "tokenIds": [] })
}

pub(super) fn context(request: ContextRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok(config) = load_config(amm_program, &request.config) else {
        return Ok(context_error(&request, "config_unavailable"));
    };

    let holdings = wallet_holdings(&request.wallet_accounts, config.token_program_id);
    let source_map = token_sources(&request, &holdings);
    let mut rows = Vec::new();
    let mut warnings = Vec::new();

    for (token_id, sources) in source_map {
        let read = request
            .token_definitions
            .iter()
            .find(|read| account_id_from_hex(&read.id, "token definition id") == Ok(token_id));
        let (name, total_supply, metadata_id) =
            match fungible_definition(read, token_id, config.token_program_id) {
                Ok(definition) => definition,
                Err(error) => {
                    rows.push(unavailable_token_row(token_id, sources, error.code));
                    if error.warn {
                        warnings.push(issue(
                            error.code,
                            "Token definition could not be read.",
                            &[],
                            json!({ "tokenId": token_id.to_string() }),
                        ));
                    }
                    continue;
                }
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

fn token_sources(
    request: &ContextRequest,
    holdings: &[SelectedHolding],
) -> BTreeMap<AccountId, Vec<String>> {
    let mut sources: BTreeMap<AccountId, BTreeSet<String>> = BTreeMap::new();
    for id in &request.configured_token_ids {
        if let Ok(id) = account_id_from_hex(id, "configured token id") {
            sources
                .entry(id)
                .or_default()
                .insert(String::from("config"));
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
    for holding in holdings {
        sources
            .entry(holding.definition_id)
            .or_default()
            .insert(String::from("holding"));
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

pub(super) struct DefinitionError {
    pub(super) code: &'static str,
    pub(super) warn: bool,
}

pub(super) fn fungible_definition(
    read: Option<&AccountRead>,
    token_id: AccountId,
    token_program: ProgramId,
) -> Result<(String, u128, Option<AccountId>), DefinitionError> {
    let Some(read) = read else {
        return Err(DefinitionError {
            code: "token_definition_unreadable",
            warn: true,
        });
    };
    let Ok((id, account)) = decode_account(read) else {
        return Err(DefinitionError {
            code: "token_definition_unreadable",
            warn: true,
        });
    };
    if id != token_id {
        return Err(DefinitionError {
            code: "token_definition_unreadable",
            warn: false,
        });
    }
    if account.program_owner != token_program {
        return Err(DefinitionError {
            code: "token_program_mismatch",
            warn: false,
        });
    }
    match TokenDefinition::try_from(&account.data) {
        Ok(TokenDefinition::Fungible {
            name,
            total_supply,
            metadata_id,
            ..
        }) => Ok((name, total_supply, metadata_id)),
        _ => Err(DefinitionError {
            code: "token_not_fungible",
            warn: false,
        }),
    }
}
