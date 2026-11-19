use amm_core::Message;
use serde_json::Value;

use super::{config::load_config, TransferOwnershipPlanRequest, WithdrawProtocolFeesPlanRequest};
use crate::account::{account_id_from_hex, parse_program_id};

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

    let message = Message::UpdateConfig {
        config: config_id,
        new_authority,
    };

    super::plan::admin_plan(&request.amm_program_id, config_id, &config, message)
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

    let message = Message::WithdrawProtocolFees {
        config: config_id,
        token: token_definition,
        destination,
        amount,
    };

    super::plan::admin_plan(&request.amm_program_id, config_id, &config, message)
}
