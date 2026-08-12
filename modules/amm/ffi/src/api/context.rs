use std::collections::BTreeSet;

use nssa_core::{account::AccountId, program::ProgramId};
use serde_json::{json, Value};
use token_core::TokenDefinition;

use super::{
    config::load_config,
    holding::{select_holding, wallet_holdings},
    ResolveTokensRequest,
};
use crate::account::{account_id_from_hex, decode_account, parse_program_id, AccountRead};

/// Resolves an explicit, app-provided set of token ids into selector rows. The app owns the
/// id set (its configured tokens plus any custom/pasted ids it remembers), so there is no
/// network envelope, no status, and no process-cached wallet state here: the module reads the
/// definitions + wallet fresh and passes them in.
///
/// `token_ids` are hex (the module normalizes base58→hex at the boundary); `token_definitions`
/// are the corresponding read accounts, keyed by hex id. Every returned row has the same shape —
/// `{ definitionId (base58), name, totalSupply, holdingId, balance }` — so the app never branches
/// per row; when the wallet doesn't hold the token, `holdingId` is `""` and `balance` is `"0"`.
/// A requested id whose definition is unreadable or non-fungible is omitted; the app treats a
/// requested id with no returned row as unresolved/unavailable.
pub(super) fn resolve_tokens(request: ResolveTokensRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok(config) = load_config(amm_program, &request.config) else {
        return Ok(json!({ "status": "error", "code": "config_unavailable", "tokens": [] }));
    };

    let holdings = wallet_holdings(&request.wallet_accounts, config.token_program_id);

    // De-duplicate the requested ids, dropping any malformed ones. Order is irrelevant —
    // the rows are sorted below (held first, then by id).
    let mut token_ids = BTreeSet::new();
    for id in &request.token_ids {
        if let Ok(id) = account_id_from_hex(id, "token id") {
            token_ids.insert(id);
        }
    }

    let mut rows = Vec::new();
    for token_id in token_ids {
        let read = request
            .token_definitions
            .iter()
            .find(|read| account_id_from_hex(&read.id, "token definition id") == Ok(token_id));
        // Only readable, fungible definitions become rows; anything else is omitted.
        let Some((name, total_supply)) =
            fungible_definition(read, token_id, config.token_program_id)
        else {
            continue;
        };

        // Uniform shape — every row carries holdingId/balance so the app never branches per row.
        // A token the wallet doesn't hold gets an empty id and "0" balance.
        let selected = select_holding(&holdings, token_id);
        rows.push(json!({
            "definitionId": token_id.to_string(),
            "name": name,
            "totalSupply": total_supply.to_string(),
            "holdingId": selected.as_ref().map(|holding| holding.id.to_string()).unwrap_or_default(),
            "balance": selected
                .as_ref()
                .map_or_else(|| String::from("0"), |holding| holding.balance.to_string()),
        }));
    }

    rows.sort_by(|left, right| {
        let held = |row: &Value| !row["holdingId"].as_str().unwrap_or_default().is_empty();
        held(right).cmp(&held(left)).then_with(|| {
            left["definitionId"]
                .as_str()
                .cmp(&right["definitionId"].as_str())
        })
    });

    Ok(json!({ "status": "ok", "tokens": rows }))
}

/// Decodes a token definition read as a fungible `(name, total_supply)`, or `None` when it is
/// unreadable, owned by a different program, its id mismatches, or it isn't fungible.
fn fungible_definition(
    read: Option<&AccountRead>,
    token_id: AccountId,
    token_program: ProgramId,
) -> Option<(String, u128)> {
    let (id, account) = decode_account(read?).ok()?;
    if id != token_id || account.program_owner != token_program {
        return None;
    }
    match TokenDefinition::try_from(&account.data) {
        Ok(TokenDefinition::Fungible {
            name, total_supply, ..
        }) => Some((name, total_supply)),
        _ => None,
    }
}
