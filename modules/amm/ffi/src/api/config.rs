use amm_core::{compute_config_pda, AmmConfig};
use nssa_core::{account::Account, program::ProgramId};
use serde_json::{json, Value};

use super::ConfigIdRequest;
use crate::account::{account_id_hex, decode_account, parse_program_id, AccountRead};

pub(super) fn config_id(request: ConfigIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    Ok(json!({
        "status": "ok",
        "configId": account_id_hex(compute_config_pda(amm_program)),
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
