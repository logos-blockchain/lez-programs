use amm_core::{compute_protocol_fee_pda, ConfigFacts, Instruction};
use serde_json::Value;
use token_core::{TokenDescriptor, TokenKind};

use super::{
    config::load_config,
    envelope::{plan_response, signer, slot},
    TransferOwnershipPlanRequest, WithdrawProtocolFeesPlanRequest,
};
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

    let instruction = borsh::to_vec(&Instruction::UpdateConfig { new_authority })
        .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL slot order for UpdateConfig: the config account (mut, updated in place, not a
    // signer) and the current admin authority (signs). `new_authority` is instruction data, not
    // an account. The authority slot is signature-only, so it names the program being called.
    let slots = [
        slot(config_id, amm_program),
        signer(config.authority, amm_program),
    ];

    Ok(plan_response(&request.amm_program_id, &slots, instruction))
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

    // The two holding reads v0.2.5 made are gone: `descriptor` rides on the chained
    // transfer, so the Token Program refuses the call unless *both* the protocol-fee
    // holding and the destination hold exactly this asset -- the whole of what those
    // reads used to establish. Protocol fees accrue in a fungible token.
    let instruction = borsh::to_vec(&Instruction::WithdrawProtocolFees {
        amount,
        facts: ConfigFacts {
            token_program_id: config.token_program_id,
            twap_oracle_program_id: config.twap_oracle_program_id,
            authority: config.authority,
            swap_fee_bps: config.swap_fee_bps,
            protocol_fee_bps: config.protocol_fee_bps,
        },
        descriptor: TokenDescriptor {
            definition_id: token_definition,
            kind: TokenKind::Fungible,
        },
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL slot order for WithdrawProtocolFees: config, the protocol-fee holding PDA (mut),
    // the destination holding (mut), and the admin authority (signs). `amount` is instruction data.
    let protocol_fee_holding = compute_protocol_fee_pda(amm_program, config_id, token_definition);
    let slots = [
        slot(config_id, amm_program),
        slot(protocol_fee_holding, config.token_program_id),
        slot(destination, config.token_program_id),
        signer(config.authority, amm_program),
    ];

    Ok(plan_response(&request.amm_program_id, &slots, instruction))
}
