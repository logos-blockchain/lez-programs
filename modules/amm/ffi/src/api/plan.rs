use amm_core::{compute_protocol_fee_pda, AmmConfig, Message};
use lee_core::account::AccountId;
use serde_json::{json, Value};

use super::pair::PairIds;
use crate::account::{account_id_hex, parse_program_id};

/// Ordered actor mentions. The root is always first; repeated actors merge authorization.
struct Plan {
    actors: Vec<(AccountId, AccountId, bool)>,
}

impl Plan {
    fn add(&mut self, account: AccountId, program: AccountId, authorized: bool) {
        if let Some(actor) = self
            .actors
            .iter_mut()
            .find(|a| a.0 == account && a.1 == program)
        {
            actor.2 |= authorized;
        } else {
            self.actors.push((account, program, authorized));
        }
    }

    fn response(self, program: &str, message: Vec<u8>) -> Value {
        json!({
            "programId": program,
            "rootMention": 0,
            "accountIds": self.actors.iter().map(|a| account_id_hex(a.0)).collect::<Vec<_>>(),
            "actorProgramIds": self.actors.iter().map(|a| account_id_hex(a.1)).collect::<Vec<_>>(),
            "signingRequirements": self.actors.iter().map(|a| a.2).collect::<Vec<_>>(),
            "message": message,
        })
    }
}

pub(super) fn pool_plan(program: &str, pair: PairIds, message: Message) -> Result<Value, String> {
    let amm = parse_program_id(program)?;
    let bytes = borsh::to_vec(&message).map_err(|e| e.to_string())?;
    let mut plan = Plan { actors: Vec::new() };
    let root = if matches!(message, Message::NewDefinition { .. }) {
        pair.config
    } else {
        pair.pool
    };
    plan.add(root, amm, false);
    plan.add(pair.config, amm, false);
    plan.add(pair.pool, amm, false);
    plan.add(pair.pool, pair.twap_oracle_program, false);
    plan.add(pair.vault_a, pair.token_program, false);
    plan.add(pair.vault_b, pair.token_program, false);
    match message {
        Message::NewDefinition {
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            ..
        } => {
            plan.add(pair.lp_definition, pair.token_program, false);
            plan.add(pair.lp_lock_holding, pair.token_program, false);
            plan.add(user_holding_a, pair.token_program, true);
            plan.add(user_holding_b, pair.token_program, true);
            plan.add(user_holding_lp, pair.token_program, true);
        }
        Message::AddLiquidity {
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            ..
        } => {
            plan.add(pair.lp_definition, pair.token_program, false);
            plan.add(user_holding_a, pair.token_program, true);
            plan.add(user_holding_b, pair.token_program, true);
            plan.add(user_holding_lp, pair.token_program, false);
        }
        Message::RemoveLiquidity {
            user_holding_a,
            user_holding_b,
            user_holding_lp,
            ..
        } => {
            plan.add(pair.lp_definition, pair.token_program, false);
            plan.add(user_holding_a, pair.token_program, false);
            plan.add(user_holding_b, pair.token_program, false);
            plan.add(user_holding_lp, pair.token_program, true);
        }
        Message::SwapExactInput {
            user_input_holding,
            user_output_holding,
            ..
        }
        | Message::SwapExactOutput {
            user_input_holding,
            user_output_holding,
            ..
        } => {
            plan.add(user_input_holding, pair.token_program, true);
            plan.add(user_output_holding, pair.token_program, false);
            for token in [pair.token_a, pair.token_b] {
                plan.add(
                    compute_protocol_fee_pda(amm, pair.config, token),
                    pair.token_program,
                    false,
                );
            }
        }
        Message::SyncReserves
        | Message::CreatePriceObservations { .. }
        | Message::CreateOraclePriceAccount { .. } => {}
        _ => return Err("unsupported pool message".into()),
    }
    plan.add(pair.token_a, pair.token_program, false);
    plan.add(pair.token_b, pair.token_program, false);
    Ok(plan.response(program, bytes))
}

pub(super) fn admin_plan(
    program: &str,
    config_id: AccountId,
    config: &AmmConfig,
    message: Message,
) -> Result<Value, String> {
    let amm = parse_program_id(program)?;
    let bytes = borsh::to_vec(&message).map_err(|e| e.to_string())?;
    let mut plan = Plan { actors: Vec::new() };
    plan.add(config.authority, amm, true);
    plan.add(config_id, amm, false);
    if let Message::WithdrawProtocolFees {
        token, destination, ..
    } = message
    {
        plan.add(
            compute_protocol_fee_pda(amm, config_id, token),
            config.token_program_id,
            false,
        );
        plan.add(destination, config.token_program_id, false);
    }
    Ok(plan.response(program, bytes))
}
