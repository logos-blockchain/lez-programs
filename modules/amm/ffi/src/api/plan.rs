use std::collections::HashSet;

use amm_core::{compute_protocol_fee_pda, AmmConfig, Message};
use lee_core::account::{AccountId, Actor};
use serde_json::{json, Value};
use twap_oracle_core::{compute_oracle_price_account_pda, compute_price_observations_pda};

use super::pair::PairIds;
use crate::account::{account_id_hex, parse_program_id};

/// Actors are selected individually, but the wallet requires one authorization per account.
#[derive(Default)]
struct Plan {
    actors: Vec<Actor>,
    signers: HashSet<AccountId>,
}

impl Plan {
    fn add(&mut self, account: AccountId, program: AccountId, authorized: bool) {
        if authorized {
            self.signers.insert(account);
        }
        let actor = Actor::new(account, program);
        if !self.actors.contains(&actor) {
            self.actors.push(actor);
        }
    }

    fn response(self, program: &str, message: Vec<u8>) -> Value {
        json!({
            "programId": program,
            "rootMention": 0,
            "accountIds": self.actors.iter().map(|a| account_id_hex(a.account_id)).collect::<Vec<_>>(),
            "actorProgramIds": self.actors.iter().map(|a| account_id_hex(a.program_account_id)).collect::<Vec<_>>(),
            "signingRequirements": self.actors.iter().map(|a| self.signers.contains(&a.account_id)).collect::<Vec<_>>(),
            "message": message,
        })
    }
}

pub(super) fn pool_plan(program: &str, pair: PairIds, message: Message) -> Result<Value, String> {
    let amm = parse_program_id(program)?;
    let bytes = borsh::to_vec(&message).map_err(|e| e.to_string())?;
    let mut plan = Plan::default();
    let creates_pool = matches!(message, Message::NewDefinition { .. });
    let root = if creates_pool { pair.config } else { pair.pool };
    plan.add(root, amm, false);
    plan.add(pair.config, amm, false);
    plan.add(pair.pool, amm, false);
    plan.add(pair.current_tick, pair.twap_oracle_program, false);
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
        Message::CreatePriceObservations { window_duration }
        | Message::CreateOraclePriceAccount { window_duration } => {
            let oracle = pair.twap_oracle_program;
            let tick = pair.current_tick;
            plan.add(
                compute_price_observations_pda(oracle, tick, window_duration),
                oracle,
                false,
            );
            plan.add(
                compute_oracle_price_account_pda(oracle, tick, window_duration),
                oracle,
                false,
            );
        }
        Message::SyncReserves => {}
        _ => return Err("unsupported pool message".into()),
    }
    plan.add(pair.token_a, pair.token_program, false);
    plan.add(pair.token_b, pair.token_program, false);
    if !creates_pool {
        let clock = clock_core::clock_actor();
        plan.add(clock.account_id, clock.program_account_id, false);
    }
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
    let mut plan = Plan::default();
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
