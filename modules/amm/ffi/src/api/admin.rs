use amm_core::{compute_protocol_fee_pda, Instruction};
use serde_json::{json, Value};

use super::{config::load_config, TransferOwnershipPlanRequest, WithdrawProtocolFeesPlanRequest};
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
    let Ok((config_id, config)) = load_config(amm_program, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    let instruction = risc0_zkvm::serde::to_vec(&Instruction::UpdateConfig { new_authority })
        .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for UpdateConfig: the config account (mut, updated in place, not a
    // signer) and the current admin authority (signs). `new_authority` is instruction data, not
    // an account.
    let account_ids = [config_id, config.authority];
    let signing_requirements = [false, true];

    Ok(json!({
        "programId": request.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
}

/// Builds the `WithdrawProtocolFees` submission that moves `amount` of accrued protocol fees for
/// one token out to `destination_id`. The fee source is the instance's protocol-fee PDA for
/// `token_definition_id`, derived here as `compute_protocol_fee_pda(amm, config_id, token_def)`.
/// The config's stored `authority` is the sole signer (decoded from `config`, like
/// `transfer_ownership_plan`); the guest enforces that only that admin may withdraw.
pub(super) fn withdraw_protocol_fees_plan(
    request: WithdrawProtocolFeesPlanRequest,
) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_definition =
        account_id_from_hex(&request.token_definition_id, "token definition id")?;
    let destination = account_id_from_hex(&request.destination_id, "destination id")?;
    let amount = request
        .amount
        .parse::<u128>()
        .map_err(|error| format!("invalid amount: {error}"))?;
    let Ok((config_id, config)) = load_config(amm_program, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    let instruction = risc0_zkvm::serde::to_vec(&Instruction::WithdrawProtocolFees { amount })
        .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for WithdrawProtocolFees: config, the protocol-fee holding PDA (mut),
    // the destination holding (mut), and the admin authority (signs). `amount` is instruction data.
    let protocol_fee_holding = compute_protocol_fee_pda(amm_program, config_id, token_definition);
    let account_ids = [
        config_id,
        protocol_fee_holding,
        destination,
        config.authority,
    ];
    let signing_requirements = [false, false, false, true];

    Ok(json!({
        "programId": request.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
}
