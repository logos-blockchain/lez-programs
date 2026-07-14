use nssa_core::account::Account;
use serde_json::{json, Value};

use super::{
    clock::decode_clock,
    position::{NewPositionPlan, QuoteBranch, QuoteComputation},
    quote::compute_quote,
    PlanRequest, QuoteRequest, SCHEMA,
};
use crate::account::{account_id_hex, decode_account, AccountRead};

const DEADLINE_WINDOW_MS: u64 = 1_200_000;

pub(super) fn plan(input: PlanRequest) -> Result<Value, String> {
    let quote_input = QuoteRequest {
        network_id: input.network_id,
        network_fingerprint: input.network_fingerprint,
        amm_program_id: input.amm_program_id.clone(),
        request: input.request,
        snapshot: input.snapshot,
    };
    let quote = compute_quote(&quote_input)?;
    if quote.quote_hash() != Some(input.quote_hash.as_str()) {
        return Ok(json!({
            "schema": SCHEMA,
            "status": "error",
            "code": "quote_changed",
            "recoverable": true,
            "quote": quote.into_value(&quote_input.request),
        }));
    }
    let evaluated = match quote {
        QuoteComputation::Evaluated(evaluated) => evaluated,
        QuoteComputation::Failed(failure) => {
            return Ok(json!({
                "schema": SCHEMA,
                "status": "error",
                "code": "quote_not_submittable",
                "recoverable": true,
                "quote": failure.into_value(&quote_input.request),
            }))
        }
    };
    let Some(plan) = evaluated.plan else {
        return Ok(json!({
            "schema": SCHEMA,
            "status": "error",
            "code": "quote_not_submittable",
            "recoverable": true,
            "quote": evaluated.value,
        }));
    };
    let fresh_lp = if plan.requires_fresh_lp() {
        let Some(read) = input.fresh_lp.as_ref() else {
            return Ok(json!({
                "schema": SCHEMA,
                "status": "needs_fresh_lp",
                "code": "fresh_lp_required",
            }));
        };
        let Ok((id, account)) = decode_account(read) else {
            return Ok(plan_error("wallet_submission_failed"));
        };
        if account != Account::default() || plan.accounts.contains(id) {
            return Ok(plan_error("wallet_submission_failed"));
        }
        Some(id)
    } else {
        None
    };
    let deadline = input
        .now_ms
        .checked_add(DEADLINE_WINDOW_MS)
        .ok_or_else(|| String::from("transaction deadline overflow"))?;
    let clock_timestamp = clock_timestamp(&quote_input.snapshot.clock)?;
    if clock_timestamp >= deadline {
        return Ok(plan_error("transaction_deadline_expired"));
    }
    let NewPositionPlan { accounts, branch } = plan;
    let (account_ids, signing_requirements) = accounts.wallet_args(fresh_lp)?;
    let instruction = match branch {
        QuoteBranch::Missing { amount_a, amount_b } => {
            let instruction = amm_core::Instruction::NewDefinition {
                token_a_amount: amount_a,
                token_b_amount: amount_b,
                fees: u128::from(quote_input.request.fee_bps),
                deadline,
            };
            risc0_zkvm::serde::to_vec(&instruction)
                .map_err(|error| format!("instruction serialization failed: {error}"))?
        }
        QuoteBranch::Active {
            max_a,
            max_b,
            minimum_lp,
            stored_reversed,
        } => {
            let (stored_max_a, stored_max_b) = if stored_reversed {
                (max_b, max_a)
            } else {
                (max_a, max_b)
            };
            let instruction = amm_core::Instruction::AddLiquidity {
                min_amount_liquidity: minimum_lp,
                max_amount_to_add_token_a: stored_max_a,
                max_amount_to_add_token_b: stored_max_b,
                deadline,
            };
            risc0_zkvm::serde::to_vec(&instruction)
                .map_err(|error| format!("instruction serialization failed: {error}"))?
        }
    };

    Ok(json!({
        "schema": SCHEMA,
        "status": "ready",
        "programId": input.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
        "deadlineMs": deadline.to_string(),
    }))
}

fn plan_error(code: &str) -> Value {
    json!({
        "schema": SCHEMA,
        "status": "error",
        "code": code,
        "recoverable": true,
    })
}

fn clock_timestamp(read: &AccountRead) -> Result<u64, String> {
    decode_clock(read).map(|(_, clock)| clock.timestamp)
}
