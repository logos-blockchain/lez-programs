use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{account::AccountId, program::ProgramId};
use serde_json::{json, Value};
use twap_oracle_core::compute_current_tick_account_pda;

use super::{config::load_config, PairIdsRequest};
use crate::account::{account_id_hex, parse_base58_id, parse_program_id, AccountRead};

#[derive(Clone, Copy)]
pub(super) struct PairIds {
    pub(super) token_a: AccountId,
    pub(super) token_b: AccountId,
    pub(super) config: AccountId,
    pub(super) pool: AccountId,
    pub(super) vault_a: AccountId,
    pub(super) vault_b: AccountId,
    pub(super) lp_definition: AccountId,
    pub(super) lp_lock_holding: AccountId,
    pub(super) current_tick: AccountId,
    pub(super) clock: AccountId,
    pub(super) token_program: ProgramId,
    pub(super) twap_program: ProgramId,
}

pub(super) fn pair_ids(request: PairIdsRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let Ok(token_a) = parse_base58_id(&request.token_a_id, "token A id") else {
        return Ok(json!({ "status": "error", "code": "invalid_token_id" }));
    };
    let Ok(token_b) = parse_base58_id(&request.token_b_id, "token B id") else {
        return Ok(json!({ "status": "error", "code": "invalid_token_id" }));
    };
    if token_a == token_b {
        return Ok(json!({ "status": "error", "code": "same_token_pair" }));
    }
    if !is_canonical_pair(token_a, token_b) {
        return Ok(json!({ "status": "error", "code": "non_canonical_pair" }));
    }

    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Ok(json!({ "status": "error", "code": "config_unavailable" }));
    };
    Ok(pair_json(pair))
}

pub(super) fn is_canonical_pair(token_a: AccountId, token_b: AccountId) -> bool {
    token_a.value() > token_b.value()
}

pub(super) fn derive_pair(
    amm_program: ProgramId,
    token_a: AccountId,
    token_b: AccountId,
    config_read: &AccountRead,
) -> Result<PairIds, String> {
    let config_id = compute_config_pda(amm_program);
    let config = load_config(amm_program, config_read)?;
    let pool = compute_pool_pda(amm_program, token_a, token_b);
    Ok(PairIds {
        token_a,
        token_b,
        config: config_id,
        pool,
        vault_a: compute_vault_pda(amm_program, pool, token_a),
        vault_b: compute_vault_pda(amm_program, pool, token_b),
        lp_definition: compute_liquidity_token_pda(amm_program, pool),
        lp_lock_holding: compute_lp_lock_holding_pda(amm_program, pool),
        current_tick: compute_current_tick_account_pda(config.twap_oracle_program_id, pool),
        clock: CLOCK_01_PROGRAM_ACCOUNT_ID,
        token_program: config.token_program_id,
        twap_program: config.twap_oracle_program_id,
    })
}

fn pair_json(pair: PairIds) -> Value {
    json!({
        "status": "ok",
        "tokenAId": account_id_hex(pair.token_a),
        "tokenBId": account_id_hex(pair.token_b),
        "configId": account_id_hex(pair.config),
        "poolId": account_id_hex(pair.pool),
        "vaultAId": account_id_hex(pair.vault_a),
        "vaultBId": account_id_hex(pair.vault_b),
        "lpDefinitionId": account_id_hex(pair.lp_definition),
        "lpLockHoldingId": account_id_hex(pair.lp_lock_holding),
        "currentTickId": account_id_hex(pair.current_tick),
        "clockId": account_id_hex(pair.clock),
    })
}
