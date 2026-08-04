//! Swap operations — pool discovery/decode, swap-transaction planning, and
//! program-id derivation. Same transport-independent pattern as the
//! new-position ops: pure functions returning JSON `Value`, PDAs reused from
//! `pair::derive_pair` so the swap path never re-derives seeds.

use amm_core::{
    compute_pool_pda, mul_div_ceil, mul_div_floor, price_impact_bps, swap_exact_in_amounts,
    swap_exact_out_amounts, PoolDefinition, FEE_BPS_DENOMINATOR,
};
use nssa_core::account::AccountId;
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};

use super::{
    pair::{derive_pair, is_canonical_pair},
    PoolIdRequest, ProgramIdRequest, ResolvePoolRequest, SwapExactInPlanRequest,
    SwapExactInQuoteRequest, SwapExactOutQuoteRequest, SwapPairRequest,
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
    let fee_bps = u32::try_from(pool.fees).map_err(|_| String::from("invalid_fee_tier"))?;
    Ok(json!({
        "exists": true,
        "defAHex": account_id_hex(pool.definition_token_a_id),
        "defBHex": account_id_hex(pool.definition_token_b_id),
        "reserveA": pool.reserve_a.to_string(),
        "reserveB": pool.reserve_b.to_string(),
        "feeBps": fee_bps,
    }))
}

/// Derives the pool PDA for a swap pair (tokens in either order). Config-free —
/// the pool address depends only on the AMM program id and the two token ids, so
/// a caller that just needs to read the pool doesn't have to load the config
/// first (unlike `swap_pair`, which also derives the config-dependent tick PDA).
pub(super) fn pool_id(request: PoolIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    Ok(json!({ "poolId": account_id_hex(compute_pool_pda(amm_program, token_a, token_b)) }))
}

/// Prices a `SwapExactInput`: orients the pool's reserves to the requested in/out
/// direction and applies the exact on-chain output via the shared
/// `amm_core::swap_exact_in_amounts` (so `expectedOut` equals what the guest
/// produces), then derives the slippage floor `minReceived`. Read-only preview —
/// no `quoteHash`; `swap_exact_input` re-prices fresh at submit and the on-chain
/// `min_amount_out` is the real guard. Errors are stable short codes callers can
/// branch on: `no_pool` (pool absent / undecodable / no liquidity),
/// `same_token_pair`, `invalid_slippage` (slippage ≥ 100%), `pair_mismatch` (the
/// decoded pool isn't for this pair), `amount_too_small` (the input fee-rounds to
/// zero effective input or zero output — the guest would reject it on submit).
/// Pool metadata (reserves, fee) comes from `resolve_pool`, so it isn't echoed here.
pub(super) fn swap_exact_in_quote(request: SwapExactInQuoteRequest) -> Result<Value, String> {
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let amount_in = parse_u128(&request.amount_in_raw, "amountInRaw")?;
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

    // Decode the pool; absent / undecodable / empty ⇒ nothing to swap against.
    let pool = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
        .filter(|pool| pool.liquidity_pool_supply != 0)
        .ok_or_else(|| String::from("no_pool"))?;

    // Orient reserves to the requested direction: the pool stores canonical
    // (token_a, token_b); the sold token selects the deposit-side reserve.
    let (reserve_in, reserve_out) = if token_in == pool.definition_token_a_id
        && token_out == pool.definition_token_b_id
    {
        (pool.reserve_a, pool.reserve_b)
    } else if token_in == pool.definition_token_b_id && token_out == pool.definition_token_a_id {
        (pool.reserve_b, pool.reserve_a)
    } else {
        return Err(String::from("pair_mismatch"));
    };
    if reserve_in == 0 || reserve_out == 0 {
        return Err(String::from("no_pool"));
    }

    // Exact on-chain pricing (shared with amm_program::swap), then the slippage floor.
    let (effective_in, expected_out) =
        swap_exact_in_amounts(amount_in, reserve_in, reserve_out, pool.fees);
    // Mirror the guest's swap_logic guards: an input that fee-rounds to zero
    // effective input (e.g. "0", or "1" at 30 bps) or yields zero output would be
    // rejected on submit before any transfer, so it must not preview as a valid
    // quote either.
    if effective_in == 0 || expected_out == 0 {
        return Err(String::from("amount_too_small"));
    }
    let slippage_complement = FEE_BPS_DENOMINATOR - u128::from(request.slippage_bps);
    let min_received = mul_div_floor(expected_out, slippage_complement, FEE_BPS_DENOMINATOR);

    // Price impact (display): how far the realized output falls below the naive
    // spot valuation, in bps (fee + curve movement combined). Computed wide so an
    // out-of-range naive valuation can't overflow/panic.
    let price_impact_bps = price_impact_bps(amount_in, expected_out, reserve_in, reserve_out);

    Ok(json!({
        "expectedOutRaw": expected_out.to_string(),
        "minReceivedRaw": min_received.to_string(),
        "priceImpactBps": price_impact_bps,
    }))
}

/// Prices a `SwapExactOutput`: for a desired output amount, computes the input
/// required via the shared `amm_core::swap_exact_out_amounts` (matching what the
/// guest charges) and the slippage ceiling `maxIn`. Read-only preview — no
/// `quoteHash`; the on-chain `max_amount_in` is the real guard. Errors are stable
/// short codes callers can branch on: `no_pool` (pool absent / undecodable / no
/// liquidity, incl. a zero reserve), `same_token_pair`, `invalid_slippage`
/// (slippage ≥ 100%), `pair_mismatch` (the decoded pool isn't for this pair),
/// `amount_too_small` (zero requested output — the guest would reject it),
/// `output_exceeds_liquidity` (asking for at least the whole reserve). Pool
/// metadata (reserves/fee) comes from `resolve_pool`.
pub(super) fn swap_exact_out_quote(request: SwapExactOutQuoteRequest) -> Result<Value, String> {
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let amount_out = parse_u128(&request.amount_out_raw, "amountOutRaw")?;
    if amount_out == 0 {
        // The guest's exact_output_swap_logic rejects a zero output before any
        // transfer, so a zero-output preview would claim an unexecutable quote
        // (swap_exact_out_amounts would otherwise return a free (0, 0)).
        return Err(String::from("amount_too_small"));
    }
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

    // Decode the pool; absent / undecodable / empty ⇒ nothing to swap against.
    let pool = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
        .filter(|pool| pool.liquidity_pool_supply != 0)
        .ok_or_else(|| String::from("no_pool"))?;

    // Orient reserves: the sold token is the deposit (input) side, the bought
    // token the withdraw (output) side.
    let (reserve_in, reserve_out) = if token_in == pool.definition_token_a_id
        && token_out == pool.definition_token_b_id
    {
        (pool.reserve_a, pool.reserve_b)
    } else if token_in == pool.definition_token_b_id && token_out == pool.definition_token_a_id {
        (pool.reserve_b, pool.reserve_a)
    } else {
        return Err(String::from("pair_mismatch"));
    };
    // A pool with a zero reserve on either side has no liquidity to price against;
    // swap_exact_out_amounts would otherwise round required_in to 0 for a positive
    // output. Mirror swap_exact_in_quote and treat it as no_pool.
    if reserve_in == 0 || reserve_out == 0 {
        return Err(String::from("no_pool"));
    }

    // Required input for the desired output (shared with amm_program::swap). None
    // when the pool can't deliver that much (amount_out >= reserve_out).
    let Some((_, required_in)) =
        swap_exact_out_amounts(amount_out, reserve_in, reserve_out, pool.fees)
    else {
        return Err(String::from("output_exceeds_liquidity"));
    };

    // Slippage ceiling: the most the user will pay, rounded up so rounding never
    // trips the on-chain max-in check.
    let slippage_ceiling = FEE_BPS_DENOMINATOR + u128::from(request.slippage_bps);
    let max_in = mul_div_ceil(required_in, slippage_ceiling, FEE_BPS_DENOMINATOR);

    // Price impact (display): how far the required input rises above the naive spot
    // cost (reserve_in * amount_out / reserve_out), in bps (fee + curve combined).
    let spot_in = mul_div_floor(reserve_in, amount_out, reserve_out);
    let price_impact_bps = if spot_in == 0 {
        0
    } else {
        u32::try_from(mul_div_floor(
            required_in.saturating_sub(spot_in),
            FEE_BPS_DENOMINATOR,
            spot_in,
        ))
        .unwrap_or(u32::MAX)
    };

    Ok(json!({
        "requiredInRaw": required_in.to_string(),
        "maxInRaw": max_in.to_string(),
        "priceImpactBps": price_impact_bps,
    }))
}

/// Builds the `SwapExactInput` submission for a pair: the fixed 8-account IDL
/// order (vaults canonical, only the user's input holding signs) and the
/// instruction words (`risc0_zkvm::serde` — the same encoding the guest
/// decodes). Mirrors `plan.rs`'s `ready` output shape.
pub(super) fn swap_exact_in_plan(request: SwapExactInPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    // Domain errors (a bad pair, an unavailable config) mirror `swap_pair`'s
    // `{ status: "error", code }` shape rather than `Err`, which is reserved for
    // malformed inputs. Callers treat any non-"ready" status as a failed plan,
    // so both map to the same outcome.
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

        let plan = swap_exact_in_plan(SwapExactInPlanRequest {
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

    fn pool_data_hex(pool: &PoolDefinition) -> String {
        hex::encode(borsh::to_vec(pool).unwrap())
    }

    #[test]
    fn swap_exact_in_quote_prices_via_shared_formula_and_orients() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
            ..Default::default()
        };

        // Sell A → receive B: reserveIn = reserve_a, reserveOut = reserve_b.
        let ab = swap_exact_in_quote(SwapExactInQuoteRequest {
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in_raw: "10000".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        // expectedOut comes from the shared on-chain formula (single source of truth).
        let (_, expected_out) = swap_exact_in_amounts(10_000, 1_000_000, 2_000_000, 30);
        assert_eq!(ab["expectedOutRaw"], expected_out.to_string());
        assert_eq!(
            ab["minReceivedRaw"],
            (expected_out * (FEE_BPS_DENOMINATOR - 50) / FEE_BPS_DENOMINATOR).to_string()
        );
        assert!(ab["priceImpactBps"].is_number());
        // Only the priced results are echoed — no pool metadata.
        assert!(ab.get("reserveInRaw").is_none());
        assert!(ab.get("feeBps").is_none());
        assert!(ab.get("poolStatus").is_none());

        // Reverse direction orients reserves the other way.
        let ba = swap_exact_in_quote(SwapExactInQuoteRequest {
            token_in_id: account_id_hex(def_b),
            token_out_id: account_id_hex(def_a),
            amount_in_raw: "10000".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        let (_, expected_out_ba) = swap_exact_in_amounts(10_000, 2_000_000, 1_000_000, 30);
        assert_eq!(ba["expectedOutRaw"], expected_out_ba.to_string());
    }

    #[test]
    fn swap_exact_in_quote_no_pool_is_an_error() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let req = |pool_data: String| SwapExactInQuoteRequest {
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in_raw: "10000".into(),
            slippage_bps: 50,
            pool_data,
        };
        // Empty / undecodable pool data.
        assert_eq!(
            swap_exact_in_quote(req(String::new())),
            Err(String::from("no_pool"))
        );
        // Zero-supply pool.
        let empty_pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 0,
            ..Default::default()
        };
        assert_eq!(
            swap_exact_in_quote(req(pool_data_hex(&empty_pool))),
            Err(String::from("no_pool"))
        );
    }

    #[test]
    fn swap_quote_rejects_zero_and_fee_rounded_inputs() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
            ..Default::default()
        };
        let req = |amount: &str| SwapExactInQuoteRequest {
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in_raw: amount.into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        };
        // amount_in = 0 → zero effective input; the guest's swap_logic would reject
        // it before any transfer, so the preview must not report an executable quote.
        assert_eq!(
            swap_exact_in_quote(req("0")),
            Err(String::from("amount_too_small"))
        );
        // amount_in = 1 fee-rounds to zero effective input at 30 bps.
        assert_eq!(
            swap_exact_in_quote(req("1")),
            Err(String::from("amount_too_small"))
        );
        // A normal amount above the fee-rounding floor still quotes.
        assert!(swap_exact_in_quote(req("10000")).unwrap()["expectedOutRaw"].is_string());
    }

    #[test]
    fn swap_quote_handles_out_of_range_spot_valuation() {
        // reserve_in = 1, reserve_out = u128::MAX: the naive spot valuation of the
        // input (reserve_out * amount_in / reserve_in) overflows u128. The quote
        // must still price it (display price impact stays bounded, no panic).
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1,
            reserve_a: 1,
            reserve_b: u128::MAX,
            fees: 30,
            ..Default::default()
        };
        let quote = swap_exact_in_quote(SwapExactInQuoteRequest {
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in_raw: "2".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        assert!(quote["expectedOutRaw"].is_string());
        assert!(
            quote["priceImpactBps"].as_u64().unwrap()
                <= u64::try_from(FEE_BPS_DENOMINATOR).unwrap()
        );
    }

    #[test]
    fn swap_exact_out_quote_requires_input_and_bounds_it() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
            ..Default::default()
        };
        let req = |amount_out_raw: &str| SwapExactOutQuoteRequest {
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_out_raw: amount_out_raw.into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        };

        // Sell A to receive exactly 10_000 B.
        let q = swap_exact_out_quote(req("10000")).unwrap();
        let (_, required_in) = swap_exact_out_amounts(10_000, 1_000_000, 2_000_000, 30).unwrap();
        assert_eq!(q["requiredInRaw"], required_in.to_string());
        // maxIn = required_in * (10000 + 50) / 10000, rounded up.
        assert_eq!(
            q["maxInRaw"],
            (required_in * 10_050).div_ceil(10_000).to_string()
        );
        assert!(q["priceImpactBps"].is_number());
        // Only the input-side results are echoed — no output/reserves.
        assert!(q.get("expectedOutRaw").is_none());
        assert!(q.get("reserveInRaw").is_none());

        // Zero requested output is rejected — the guest rejects exact_amount_out
        // == 0, so a zero-output preview would claim an unexecutable quote.
        assert_eq!(
            swap_exact_out_quote(req("0")),
            Err(String::from("amount_too_small"))
        );

        // Asking for the whole reserve (or more) is unfulfillable.
        assert_eq!(
            swap_exact_out_quote(req("2000000")),
            Err(String::from("output_exceeds_liquidity"))
        );

        // A zero reserve on either side is no liquidity, not a free quote — it
        // passes the non-zero-supply decode filter but must still surface no_pool.
        let empty_side = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1,
            reserve_a: 0,
            reserve_b: 2_000_000,
            fees: 30,
            ..Default::default()
        };
        assert_eq!(
            swap_exact_out_quote(SwapExactOutQuoteRequest {
                token_in_id: account_id_hex(def_a),
                token_out_id: account_id_hex(def_b),
                amount_out_raw: "10000".into(),
                slippage_bps: 50,
                pool_data: pool_data_hex(&empty_side),
            }),
            Err(String::from("no_pool"))
        );
    }

    #[test]
    fn pool_id_is_order_independent_and_matches_core() {
        let program = "00".repeat(32);
        let a = AccountId::new([0xCC; 32]);
        let b = AccountId::new([0xDD; 32]);

        let ab = pool_id(PoolIdRequest {
            amm_program_id: program.clone(),
            token_in_id: account_id_hex(a),
            token_out_id: account_id_hex(b),
        })
        .unwrap();
        let ba = pool_id(PoolIdRequest {
            amm_program_id: program.clone(),
            token_in_id: account_id_hex(b),
            token_out_id: account_id_hex(a),
        })
        .unwrap();
        // Canonical ordering makes the pool id independent of swap direction.
        assert_eq!(ab, ba);

        // And it matches amm_core's PDA for the canonical pair.
        let amm = parse_program_id(&program).unwrap();
        let (ca, cb) = if is_canonical_pair(a, b) {
            (a, b)
        } else {
            (b, a)
        };
        assert_eq!(ab["poolId"], account_id_hex(compute_pool_pda(amm, ca, cb)));

        // Same token in/out is rejected.
        assert!(pool_id(PoolIdRequest {
            amm_program_id: program,
            token_in_id: account_id_hex(a),
            token_out_id: account_id_hex(a),
        })
        .is_err());
    }
}
