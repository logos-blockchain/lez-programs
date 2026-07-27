//! Swap operations — pool discovery/decode, swap-transaction planning, and
//! program-id derivation. Same transport-independent pattern as the
//! new-position ops: pure functions returning JSON `Value`, PDAs reused from
//! `pair::derive_pair` so the swap path never re-derives seeds.

use amm_core::PoolDefinition;
use nssa_core::account::AccountId;
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};

use super::{
    pair::{derive_pair, is_canonical_pair},
    ProgramIdRequest, ResolvePoolRequest, SwapPairRequest, SwapPlanRequest,
};
use crate::account::{
    account_id_from_hex, account_id_hex, decode_account, parse_program_id, program_id_bytes,
};

/// Orders `(token_in, token_out)` into the pool's canonical `(token_a, token_b)`
/// so derived vault PDAs line up with the pool's stored `vault_a`/`vault_b`.
fn canonical_pair(token_in: AccountId, token_out: AccountId) -> (AccountId, AccountId) {
    if is_canonical_pair(token_in, token_out) {
        (token_in, token_out)
    } else {
        (token_out, token_in)
    }
}

fn parse_u128(value: &str, label: &str) -> Result<u128, String> {
    value
        .parse::<u128>()
        .map_err(|error| format!("invalid {label}: {error}"))
}

fn parse_u64(value: &str, label: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|error| format!("invalid {label}: {error}"))
}

/// Derives the canonical account ids for a swap pair — reuses `derive_pair`
/// after ordering `(token_in, token_out)` into `(token_a, token_b)`, so a
/// caller can read the pool account before decoding it. Like `pair_ids`, but
/// accepts the tokens in either order.
pub(super) fn swap_pair(request: SwapPairRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Ok(json!({ "status": "error", "code": "same_token_pair" }));
    }
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Ok(json!({ "status": "error", "code": "config_unavailable" }));
    };
    Ok(json!({
        "status": "ok",
        "configId": account_id_hex(pair.config),
        "poolId": account_id_hex(pair.pool),
        "vaultAId": account_id_hex(pair.vault_a),
        "vaultBId": account_id_hex(pair.vault_b),
        "currentTickId": account_id_hex(pair.current_tick),
        "clockId": account_id_hex(pair.clock),
    }))
}

/// Decodes a pool account: whether it holds liquidity, its canonical token ids
/// (`defAHex`/`defBHex`), its reserves (same canonical `a`/`b` order — the
/// caller matches a reserve to its own direction by comparing its token id
/// against `defAHex`), and fee tier. Absent/empty/uninitialized pool →
/// `{ exists: false }`.
pub(super) fn resolve_pool(request: ResolvePoolRequest) -> Result<Value, String> {
    if request.pool.status != "ok" {
        return Ok(json!({ "exists": false }));
    }
    let Ok((_, pool_account)) = decode_account(&request.pool) else {
        return Ok(json!({ "exists": false }));
    };
    let Ok(pool) = PoolDefinition::try_from(&pool_account.data) else {
        return Ok(json!({ "exists": false }));
    };
    if pool.liquidity_pool_supply == 0 {
        return Ok(json!({ "exists": false }));
    }
    let fee_bps =
        u32::try_from(pool.fees).map_err(|_| String::from("pool fee tier exceeds u32"))?;
    Ok(json!({
        "exists": true,
        "defAHex": account_id_hex(pool.definition_token_a_id),
        "defBHex": account_id_hex(pool.definition_token_b_id),
        "reserveA": pool.reserve_a.to_string(),
        "reserveB": pool.reserve_b.to_string(),
        "feeBps": fee_bps,
    }))
}

/// Builds the `SwapExactInput` submission for a pair: the fixed 8-account IDL
/// order (vaults canonical, only the user's input holding signs) and the
/// instruction words (`risc0_zkvm::serde` — the same encoding the guest
/// decodes). Mirrors `plan.rs`'s `ready` output shape.
pub(super) fn swap_plan(request: SwapPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    // Domain errors (a bad pair, an unavailable config) mirror `swap_pair`'s
    // `{ status: "error", code }` shape rather than `Err`, which is reserved for
    // malformed inputs. `SwapRuntime::swap` treats any non-"ready" status as a
    // failed plan, so both map to the same UI outcome.
    if token_in == token_out {
        return Ok(json!({ "status": "error", "code": "same_token_pair" }));
    }
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Ok(json!({ "status": "error", "code": "config_unavailable" }));
    };

    let user_input_holding =
        account_id_from_hex(&request.user_input_holding_id, "user input holding id")?;
    let user_output_holding =
        account_id_from_hex(&request.user_output_holding_id, "user output holding id")?;

    let swap_amount_in = parse_u128(&request.amount_in, "amountIn")?;
    let min_amount_out = parse_u128(&request.min_out, "minOut")?;
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;

    let instruction = risc0_zkvm::serde::to_vec(&amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        deadline,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for SwapExactInput; only user_input_holding signs.
    let account_ids = [
        pair.config,
        pair.pool,
        pair.vault_a,
        pair.vault_b,
        user_input_holding,
        user_output_holding,
        pair.current_tick,
        pair.clock,
    ];
    let signing_requirements = [false, false, false, false, true, false, false, false];

    Ok(json!({
        "status": "ready",
        "programId": request.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
        "deadlineMs": deadline.to_string(),
    }))
}

/// Computes the AMM `ProgramId` (RISC Zero Image ID) of a deployed program
/// binary. `elf` is the hex-encoded `.bin` (a RISC Zero `ProgramBinary`, not a
/// raw ELF) — decoded, image-id computed, returned as 64-char lowercase hex.
pub(super) fn program_id(request: ProgramIdRequest) -> Result<Value, String> {
    let elf = hex::decode(&request.elf).map_err(|error| format!("invalid elf hex: {error}"))?;
    let binary = ProgramBinary::decode(&elf).map_err(|error| format!("{error:?}"))?;
    let image_id: nssa_core::program::ProgramId = binary
        .compute_image_id()
        .map_err(|error| format!("{error:?}"))?
        .into();
    Ok(json!({ "programId": hex::encode(program_id_bytes(image_id)) }))
}

#[cfg(test)]
mod tests {
    use amm_core::PoolDefinition;
    use nssa_core::account::AccountId;

    use super::*;
    use crate::account::{AccountRead, WalletAccount};

    fn pool_read(pool: &PoolDefinition) -> AccountRead {
        AccountRead {
            id: "11".repeat(32),
            status: String::from("ok"),
            account: Some(WalletAccount {
                program_owner: "00".repeat(32),
                balance: "0".repeat(32),
                nonce: "0".repeat(32),
                data: hex::encode(borsh::to_vec(pool).unwrap()),
            }),
        }
    }

    #[test]
    fn resolve_pool_reports_canonical_token_ids() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1_000,
            reserve_a: 111,
            reserve_b: 222,
            fees: 30,
            ..Default::default()
        };

        let value = resolve_pool(ResolvePoolRequest {
            pool: pool_read(&pool),
        })
        .unwrap();

        assert_eq!(value["exists"], true);
        // The Swap UI matches reserveA/reserveB to its own sell/buy direction by
        // comparing the sold token's id against defAHex — so both ids must be
        // present in canonical order.
        assert_eq!(value["defAHex"], account_id_hex(def_a));
        assert_eq!(value["defBHex"], account_id_hex(def_b));
        assert_eq!(value["reserveA"], "111");
        assert_eq!(value["reserveB"], "222");
        assert_eq!(value["feeBps"], 30);
    }

    #[test]
    fn resolve_pool_absent_when_no_liquidity() {
        let pool = PoolDefinition {
            liquidity_pool_supply: 0,
            ..Default::default()
        };
        let value = resolve_pool(ResolvePoolRequest {
            pool: pool_read(&pool),
        })
        .unwrap();
        assert_eq!(value, json!({ "exists": false }));
    }

    #[test]
    fn same_token_is_a_recoverable_domain_error_in_both_ops() {
        let program = "00".repeat(32);
        let same = "aa".repeat(32);
        let expected = json!({ "status": "error", "code": "same_token_pair" });
        // Not reached before the same-token check, so its contents don't matter.
        let dummy_config = AccountRead {
            id: String::new(),
            status: String::from("read_failed"),
            account: None,
        };

        let pair = swap_pair(SwapPairRequest {
            amm_program_id: program.clone(),
            token_in_id: same.clone(),
            token_out_id: same.clone(),
            config: dummy_config.clone(),
        })
        .unwrap();
        assert_eq!(pair, expected);

        let plan = swap_plan(SwapPlanRequest {
            amm_program_id: program,
            token_in_id: same.clone(),
            token_out_id: same,
            config: dummy_config,
            user_input_holding_id: String::new(),
            user_output_holding_id: String::new(),
            amount_in: String::new(),
            min_out: String::new(),
            deadline_ms: String::new(),
        })
        .unwrap();
        assert_eq!(plan, expected);
    }
}
