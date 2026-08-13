use amm_core::{compute_config_pda, AmmConfig};
use nssa_core::{account::Account, program::ProgramId};
use serde_json::{json, Value};

use super::{ConfigAccountRequest, ConfigIdRequest};
use crate::account::{
    account_id_hex, decode_account, parse_program_id, program_id_base58, AccountRead,
};

pub(super) fn config_id(request: ConfigIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    Ok(json!({
        "status": "ok",
        "configId": account_id_hex(compute_config_pda(amm_program)),
    }))
}

/// Decodes the singleton config account: authority + the token/twap program ids the AMM chains
/// into. Ids are base58 (app-facing). `config_unavailable` when the config PDA isn't on-chain
/// yet / undecodable; `configId` / `ammProgramId` are still derivable from `amm_program_id` via
/// `config_id` for address derivation.
pub(super) fn config_account(request: ConfigAccountRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok(config) = load_config(amm_program, &request.config) else {
        return Ok(json!({ "status": "error", "error": "config_unavailable" }));
    };
    Ok(json!({
        "status": "ok",
        "error": "",
        "configId": compute_config_pda(amm_program).to_string(),
        "ammProgramId": program_id_base58(amm_program),
        "authority": config.authority.to_string(),
        "tokenProgramId": program_id_base58(config.token_program_id),
        "twapOracleProgramId": program_id_base58(config.twap_oracle_program_id),
    }))
}

pub(super) fn load_config(amm_program: ProgramId, read: &AccountRead) -> Result<AmmConfig, String> {
    let (id, account) = decode_account(read)?;
    if id != compute_config_pda(amm_program)
        || account.program_owner != amm_program
        || account == Account::default()
    {
        return Err(String::from("AMM config is unavailable"));
    }
    AmmConfig::try_from(&account.data).map_err(|_| String::from("AMM config is invalid"))
}
