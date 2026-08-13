use amm_core::{compute_config_pda, Instruction};
use serde_json::{json, Value};

use super::{config::load_config, TransferOwnershipPlanRequest};
use crate::account::{account_id_from_hex, account_id_hex, parse_program_id};

/// Builds the `UpdateConfig` submission that transfers the AMM's admin authority. The current
/// admin — the config's stored `authority`, decoded from `config` — is the sole signer;
/// `new_authority_id` (hex) becomes the new admin. The guest enforces that only the current admin
/// (signed) may call this and that the immutable program ids can't change.
pub(super) fn transfer_ownership_plan(
    request: TransferOwnershipPlanRequest,
) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let new_authority = account_id_from_hex(&request.new_authority_id, "new authority id")?;
    let Ok(config) = load_config(amm_program, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    let instruction = risc0_zkvm::serde::to_vec(&Instruction::UpdateConfig { new_authority })
        .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for UpdateConfig: the config account (mut, updated in place, not a
    // signer) and the current admin authority (signs). `new_authority` is instruction data, not
    // an account.
    let account_ids = [compute_config_pda(amm_program), config.authority];
    let signing_requirements = [false, true];

    Ok(json!({
        "programId": request.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
}
