use amm_core::{compute_config_pda, AmmConfig};
use lee_core::{
    account::{Account, AccountId},
    program::ProgramId,
};
use serde_json::{json, Value};

use super::{ConfigAccountRequest, ConfigIdRequest};
use crate::account::{
    account_id_hex, decode_account, parse_base58_id, parse_hex_32, parse_program_id,
    program_id_base58, AccountRead,
};

pub(super) fn config_id(request: ConfigIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let owner = parse_base58_id(&request.owner, "owner")?;
    // An omitted / empty nonce selects the owner's default (all-zero) instance.
    let nonce = if request.nonce.is_empty() {
        [0_u8; 32]
    } else {
        parse_hex_32(&request.nonce, "nonce")?
    };
    Ok(json!({
        "status": "ok",
        "configId": account_id_hex(compute_config_pda(amm_program, owner, nonce)),
    }))
}

/// Decodes the AMM config account: authority, the token/twap program ids the AMM chains into, the
/// instance-wide `swapFeeBps` (the swap fee charged on every swap in this namespace — fees are not
/// per-pool), and `protocolFeeBps` (the fraction of that swap fee diverted to the protocol; `0`
/// disables it). Ids are base58 (app-facing). `config_unavailable` when the config PDA isn't
/// on-chain yet / undecodable; `configId` / `ammProgramId` are still derivable from
/// `amm_program_id` via `config_id` for address derivation.
pub(super) fn config_account(request: ConfigAccountRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok((config_id, config)) = load_config(amm_program, &request.config) else {
        return Ok(json!({ "status": "error", "error": "config_unavailable" }));
    };
    Ok(json!({
        "status": "ok",
        "error": "",
        "configId": config_id.to_string(),
        "ammProgramId": program_id_base58(amm_program),
        "authority": config.authority.to_string(),
        "tokenProgramId": program_id_base58(config.token_program_id),
        "twapOracleProgramId": program_id_base58(config.twap_oracle_program_id),
        "swapFeeBps": u32::try_from(config.swap_fee_bps).unwrap_or(u32::MAX),
        "protocolFeeBps": u32::try_from(config.protocol_fee_bps).unwrap_or(u32::MAX),
    }))
}

/// Decodes and validates a passed AMM config account, returning its id (the namespace root
/// callers derive pools under) alongside the decoded config. Since the config PDA is now
/// namespaced by `(owner, nonce)`, the id can no longer be recomputed here without those
/// inputs — the caller-supplied account's id IS the namespace root. Program ownership and a
/// non-default, parseable account are still enforced.
pub(super) fn load_config(
    amm_program: ProgramId,
    read: &AccountRead,
) -> Result<(AccountId, AmmConfig), String> {
    let (id, account) = decode_account(read)?;
    if account.program_owner != amm_program || account == Account::default() {
        return Err(String::from("AMM config is unavailable"));
    }
    let config =
        AmmConfig::try_from(&account.data).map_err(|_| String::from("AMM config is invalid"))?;
    Ok((id, config))
}
