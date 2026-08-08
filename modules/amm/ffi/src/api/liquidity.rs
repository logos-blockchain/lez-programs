//! Liquidity operations — pool-creation quoting and the `NewDefinition` submission
//! plan. Same lean, transport-independent pattern as `swap.rs`: pure functions
//! returning JSON `Value`, and the token pair canonicalized server-side so callers
//! keep no ordering logic.
//!
//! `liquidity_quote` is a **pure create-pool preview**: a function of the caller's
//! own inputs (the two deposit amounts) with no chain reads and no commitment
//! — it prices the opening LP and price via the same `amm_core` primitives the guest
//! runs (`isqrt_product`, `MINIMUM_LIQUIDITY`, `spot_price_q64_64`), so the preview
//! equals what `new_definition` mints. The caller decides create-vs-add by pool
//! existence before calling; a stale preview or a raced create just reverts on the
//! guest's `assert pool uninitialized`.

use amm_core::{
    isqrt_product, mul_div_floor, spot_price_q64_64, PoolDefinition, MINIMUM_LIQUIDITY,
};
use nssa_core::account::AccountId;
use serde_json::{json, Value};

use super::{
    pair::{derive_pair, is_canonical_pair},
    AddLiquidityPlanRequest, AddLiquidityQuoteRequest, CreatePoolPlanRequest,
    LiquidityQuoteRequest,
};
use crate::account::{account_id_from_hex, account_id_hex, parse_program_id};

/// Parses a required, strictly-positive base-unit amount. Empty / non-digit /
/// zero inputs surface as stable codes so the UI can flag the offending field.
fn positive_amount(value: Option<&str>) -> Result<u128, String> {
    let value = value
        .filter(|raw| !raw.is_empty())
        .ok_or("amount_required")?;
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(String::from("invalid_raw_amount"));
    }
    let amount = value.parse::<u128>().map_err(|_| "invalid_raw_amount")?;
    if amount == 0 {
        return Err(String::from("amount_must_be_positive"));
    }
    Ok(amount)
}

fn parse_u64(value: &str, label: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|error| format!("invalid {label}: {error}"))
}

/// Canonicalizes a token pair and moves each token's paired `(amount, holding)` with it, so
/// the returned `a` side is the canonical token-a — lining up with `derive_pair`'s canonical
/// `vault_a`. The guest derives `vault_a` from `user_holding_a`'s definition and debits
/// `amount_a` into it, so the `(token, amount, holding)` triple must stay together (see
/// `create_pool_plan`). Shared by the create and add plans.
fn canonical_triples(
    token_a: AccountId,
    token_b: AccountId,
    amount_a: u128,
    amount_b: u128,
    holding_a: AccountId,
    holding_b: AccountId,
) -> (AccountId, AccountId, u128, u128, AccountId, AccountId) {
    if is_canonical_pair(token_a, token_b) {
        (token_a, token_b, amount_a, amount_b, holding_a, holding_b)
    } else {
        (token_b, token_a, amount_b, amount_a, holding_b, holding_a)
    }
}

/// The tx-submission envelope shared by the create and add plans: the fixed IDL account ids
/// as hex, their signer flags, and the risc0-encoded instruction words.
fn plan_response(
    program_id: &str,
    account_ids: impl IntoIterator<Item = AccountId>,
    signing_requirements: &[bool],
    instruction: Vec<u32>,
) -> Value {
    json!({
        "programId": program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    })
}

/// Prices a create-pool deposit: the LP the creator receives and the opening price.
///
/// Pure — no chain reads. The fee tier is not needed: it is not part of the pool PDA
/// (`compute_pool_pda_seed` hashes only the pair) and does not enter the pricing —
/// only the two deposit amounts do. `expected_lp = floor(sqrt(a*b)) - MINIMUM_LIQUIDITY`
/// (the post-permanent-lock remainder the guest mints to the creator); `initialPriceRaw`
/// is the `Q64.64` display price (token B per token A, in the caller's order). The LP
/// figure is orientation-independent (the product is symmetric); the price follows the
/// display order. Errors are stable short codes: `same_token_pair`, `amount_required`,
/// `invalid_raw_amount`, `amount_must_be_positive`, `amount_too_low` (deposits too small
/// to clear the locked minimum — the pool can't open).
pub(super) fn liquidity_quote(request: LiquidityQuoteRequest) -> Result<Value, String> {
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }

    let amount_a = positive_amount(request.amount_a_raw.as_deref())?;
    let amount_b = positive_amount(request.amount_b_raw.as_deref())?;

    // LP math (shared with the guest's new_definition via amm_core): the initial LP
    // must clear the permanently-locked minimum before the creator receives any.
    let initial_lp = isqrt_product(amount_a, amount_b);
    let expected_lp = initial_lp
        .checked_sub(MINIMUM_LIQUIDITY)
        .filter(|user_lp| *user_lp > 0)
        .ok_or("amount_too_low")?;
    // Display-order price: token B per token A (the caller's orientation).
    let initial_price = spot_price_q64_64(amount_a, amount_b);

    Ok(json!({
        "amountARaw": amount_a.to_string(),
        "amountBRaw": amount_b.to_string(),
        "expectedLpRaw": expected_lp.to_string(),
        "lockedLpRaw": MINIMUM_LIQUIDITY.to_string(),
        "initialPriceRaw": initial_price.to_string(),
    }))
}

/// Builds the `NewDefinition` submission for creating a pool.
///
/// The pool PDA is order-independent (`compute_pool_pda_seed` sorts the pair
/// internally), but `derive_pair` yields the vaults / current-tick in **canonical**
/// order (`vault_a` = the larger token id's vault). The guest, in turn, derives
/// `vault_a` from `user_holding_a`'s definition and transfers `token_a_amount` out
/// of it — so the `(vault_a, user_holding_a, token_a_amount)` triple must all name
/// the same token or balances land in the wrong vault. This op therefore
/// canonicalizes the pair and moves its amounts / user holdings **as one unit**, so
/// `user_a` is the canonical token-a holding, `canonical_amount_a` its deposit, and
/// `pair.vault_a` its vault. Returns the fixed 11-account IDL order with only the
/// three user holdings (a, b, LP) signing. Recoverable failures fail closed as `Err`
/// (`same_token_pair`, `config_unavailable`, bad amounts) so the caller never
/// submits an empty plan.
pub(super) fn create_pool_plan(request: CreatePoolPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let holding_a = account_id_from_hex(&request.user_holding_a_id, "user holding A id")?;
    let holding_b = account_id_from_hex(&request.user_holding_b_id, "user holding B id")?;
    let user_lp = account_id_from_hex(&request.user_holding_lp_id, "user LP holding id")?;

    let amount_a = positive_amount(request.amount_a_raw.as_deref())?;
    let amount_b = positive_amount(request.amount_b_raw.as_deref())?;
    if !amm_core::is_supported_fee_tier(u128::from(request.fee_bps)) {
        return Err(String::from("invalid_fee_tier"));
    }
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;
    // Canonical orientation: (token, amount, holding) all move together, so user_a is
    // the canonical token-a holding and canonical_amount_a its deposit — matching the
    // canonical vault_a derive_pair returns (see the doc comment).
    let (canonical_a, canonical_b, canonical_amount_a, canonical_amount_b, user_a, user_b) =
        canonical_triples(token_a, token_b, amount_a, amount_b, holding_a, holding_b);

    let Ok(pair) = derive_pair(amm_program, canonical_a, canonical_b, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    let instruction = risc0_zkvm::serde::to_vec(&amm_core::Instruction::NewDefinition {
        token_a_amount: canonical_amount_a,
        token_b_amount: canonical_amount_b,
        fees: u128::from(request.fee_bps),
        deadline,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for NewDefinition; only the user holdings (a, b, LP) sign.
    let account_ids = [
        pair.config,
        pair.pool,
        pair.vault_a,
        pair.vault_b,
        pair.lp_definition,
        pair.lp_lock_holding,
        user_a,
        user_b,
        user_lp,
        pair.current_tick,
        pair.clock,
    ];
    let signing_requirements = [
        false, false, false, false, false, false, true, true, true, false, false,
    ];

    Ok(plan_response(
        &request.amm_program_id,
        account_ids,
        &signing_requirements,
        instruction,
    ))
}

/// Prices an `AddLiquidity` into an existing pool. Mirrors the swap quotes: decode the
/// pool from `pool_data` (absent / undecodable / zero-supply ⇒ `no_pool`), orient the
/// caller's max amounts to the pool's canonical `(a, b)` order, then run the guest's exact
/// proportional-deposit math (`amm_program::add::add_liquidity`): the ideal→actual clamp
/// and `delta_lp = min(supply·actual_a/reserve_a, supply·actual_b/reserve_b)`. Returns the
/// same shape as the create quote minus the create-only locked LP: the actual ratio-matched
/// deposits (display order), the LP minted, and the pool's spot price (`priceRaw`, token B
/// per token A in display order). The slippage floor is applied at submit, not here — the
/// quote is a pure preview like create. Errors: `same_token_pair`, `no_pool`,
/// `pair_mismatch` (the pool isn't for this pair), bad amounts (`amount_required`,
/// `invalid_raw_amount`, `amount_must_be_positive`), `amount_too_low` (the deposit rounds
/// to zero LP — nothing to mint).
pub(super) fn add_liquidity_quote(request: AddLiquidityQuoteRequest) -> Result<Value, String> {
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let max_a = positive_amount(Some(&request.max_amount_a_raw))?;
    let max_b = positive_amount(Some(&request.max_amount_b_raw))?;

    // Decode the pool; absent / undecodable / empty ⇒ nothing to add to.
    let pool = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
        .filter(|pool| pool.liquidity_pool_supply != 0)
        .ok_or_else(|| String::from("no_pool"))?;

    // Orient the caller's (display) max amounts to the pool's canonical (a, b) order so
    // the math lines up with the guest; `reversed` also flips the results back to display.
    let reversed = if token_a == pool.definition_token_a_id && token_b == pool.definition_token_b_id
    {
        false
    } else if token_a == pool.definition_token_b_id && token_b == pool.definition_token_a_id {
        true
    } else {
        return Err(String::from("pair_mismatch"));
    };
    if pool.reserve_a == 0 || pool.reserve_b == 0 {
        return Err(String::from("no_pool"));
    }
    let (max_canonical_a, max_canonical_b) = if reversed {
        (max_b, max_a)
    } else {
        (max_a, max_b)
    };

    // Guest math (amm_program::add::add_liquidity): proportional deposit clamped to the
    // caller's maxes, then the LP minted for the smaller side.
    let ideal_a = mul_div_floor(pool.reserve_a, max_canonical_b, pool.reserve_b);
    let ideal_b = mul_div_floor(pool.reserve_b, max_canonical_a, pool.reserve_a);
    let actual_a = ideal_a.min(max_canonical_a);
    let actual_b = ideal_b.min(max_canonical_b);
    let delta_lp = std::cmp::min(
        mul_div_floor(pool.liquidity_pool_supply, actual_a, pool.reserve_a),
        mul_div_floor(pool.liquidity_pool_supply, actual_b, pool.reserve_b),
    );
    if actual_a == 0 || actual_b == 0 || delta_lp == 0 {
        return Err(String::from("amount_too_low"));
    }

    // Back to display order for the response; the price uses the display-oriented reserves.
    let (display_a, display_b) = if reversed {
        (actual_b, actual_a)
    } else {
        (actual_a, actual_b)
    };
    let (reserve_display_a, reserve_display_b) = if reversed {
        (pool.reserve_b, pool.reserve_a)
    } else {
        (pool.reserve_a, pool.reserve_b)
    };
    let price = spot_price_q64_64(reserve_display_a, reserve_display_b);

    Ok(json!({
        "amountARaw": display_a.to_string(),
        "amountBRaw": display_b.to_string(),
        "expectedLpRaw": delta_lp.to_string(),
        "priceRaw": price.to_string(),
    }))
}

/// Builds the `AddLiquidity` submission for an existing pool. Same canonicalization as
/// `create_pool_plan` — the `(token, max_amount, holding)` triples move together so `user_a`
/// / `max_canonical_a` line up with the pool's canonical `vault_a`. Vaults and the LP
/// definition come from the pool's STORED ids (`pool_data`), which the guest asserts against
/// (like the swap plans). Emits the fixed 10-account IDL order with only the three user
/// holdings (a, b, LP) signing. `min_amount_liquidity` is the caller's slippage floor and
/// must be positive (the guest rejects a zero). Recoverable failures fail closed as `Err`
/// (`same_token_pair`, `config_unavailable`, `no_pool`, bad amounts).
pub(super) fn add_liquidity_plan(request: AddLiquidityPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let holding_a = account_id_from_hex(&request.user_holding_a_id, "user holding A id")?;
    let holding_b = account_id_from_hex(&request.user_holding_b_id, "user holding B id")?;
    let user_lp = account_id_from_hex(&request.user_holding_lp_id, "user LP holding id")?;

    let max_a = positive_amount(Some(&request.max_amount_a_raw))?;
    let max_b = positive_amount(Some(&request.max_amount_b_raw))?;
    let min_lp = positive_amount(Some(&request.min_lp_raw))?;
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;

    // config / pool / current_tick / clock are order-independent PDAs, so derive_pair takes the
    // tokens in the caller's order. (Its canonical vaults are unused here — the guest asserts the
    // vaults against the pool's stored ids, taken below.)
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    // Vaults + LP definition come from the pool's stored ids (the guest asserts against them).
    let Some(pool) = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
    else {
        return Err(String::from("no_pool"));
    };

    // Orient (max amount, holding) to the pool's STORED (definition_token_a_id,
    // definition_token_b_id) order — NOT is_canonical_pair. The guest transfers user_holding_a
    // into vault_a (== pool.vault_a_id, which holds definition_token_a_id), so user_a / max_pool_a
    // must be that token's holding / cap. A pool created outside the FFI (e.g. a non-canonical
    // `spel new-definition`) can store the opposite order, so keying off is_canonical_pair would
    // send a holding into the wrong vault and the token program rejects the transfer on a
    // sender/recipient definition mismatch.
    let (max_pool_a, max_pool_b, user_a, user_b) =
        if token_a == pool.definition_token_a_id && token_b == pool.definition_token_b_id {
            (max_a, max_b, holding_a, holding_b)
        } else if token_a == pool.definition_token_b_id && token_b == pool.definition_token_a_id {
            (max_b, max_a, holding_b, holding_a)
        } else {
            return Err(String::from("pair_mismatch"));
        };

    let instruction = risc0_zkvm::serde::to_vec(&amm_core::Instruction::AddLiquidity {
        min_amount_liquidity: min_lp,
        max_amount_to_add_token_a: max_pool_a,
        max_amount_to_add_token_b: max_pool_b,
        deadline,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for AddLiquidity; only the user holdings (a, b, LP) sign.
    let account_ids = [
        pair.config,
        pair.pool,
        pool.vault_a_id,
        pool.vault_b_id,
        pool.liquidity_pool_id,
        user_a,
        user_b,
        user_lp,
        pair.current_tick,
        pair.clock,
    ];
    let signing_requirements = [
        false, false, false, false, false, true, true, true, false, false,
    ];

    Ok(plan_response(
        &request.amm_program_id,
        account_ids,
        &signing_requirements,
        instruction,
    ))
}

#[cfg(test)]
mod tests {
    use amm_core::{compute_config_pda, compute_pool_pda, compute_vault_pda, AmmConfig};
    use nssa_core::account::{Account, AccountId, Data};

    use super::*;
    use crate::account::{account_read, AccountRead};

    fn quote_request(token_a: AccountId, token_b: AccountId) -> LiquidityQuoteRequest {
        LiquidityQuoteRequest {
            token_a_id: account_id_hex(token_a),
            token_b_id: account_id_hex(token_b),
            amount_a_raw: Some(String::from("1000000")),
            amount_b_raw: Some(String::from("4000000")),
        }
    }

    fn read_failed() -> AccountRead {
        AccountRead {
            id: String::new(),
            status: String::from("read_failed"),
            account: None,
        }
    }

    /// A valid AMM config account read so `derive_pair` succeeds in plan tests.
    fn valid_config(amm: nssa_core::program::ProgramId) -> AccountRead {
        let token_program = parse_program_id(&"01".repeat(32)).unwrap();
        let twap_program = parse_program_id(&"02".repeat(32)).unwrap();
        let account = Account {
            program_owner: amm,
            data: Data::from(&AmmConfig {
                token_program_id: token_program,
                twap_oracle_program_id: twap_program,
                authority: AccountId::new([0x09; 32]),
            }),
            ..Account::default()
        };
        account_read(compute_config_pda(amm), &account)
    }

    #[test]
    fn create_quote_prices_the_opening() {
        let token_a = AccountId::new([0xAA; 32]);
        let token_b = AccountId::new([0xBB; 32]);
        let value = liquidity_quote(quote_request(token_a, token_b)).unwrap();

        assert_eq!(value["amountARaw"], "1000000");
        assert_eq!(value["amountBRaw"], "4000000");
        assert_eq!(value["lockedLpRaw"], MINIMUM_LIQUIDITY.to_string());
        // initial_lp = isqrt(1_000_000 * 4_000_000) = 2_000_000; creator LP = minus lock.
        let initial_lp = isqrt_product(1_000_000, 4_000_000);
        assert_eq!(
            value["expectedLpRaw"],
            (initial_lp - MINIMUM_LIQUIDITY).to_string()
        );
        assert_eq!(
            value["initialPriceRaw"],
            spot_price_q64_64(1_000_000, 4_000_000).to_string()
        );
        // Lean preview — no commitment / status / submittability fields.
        assert!(value.get("quoteHash").is_none());
        assert!(value.get("canSubmit").is_none());
        assert!(value.get("poolStatus").is_none());
    }

    #[test]
    fn create_quote_lp_is_orientation_independent() {
        let token_a = AccountId::new([0xAA; 32]);
        let token_b = AccountId::new([0xBB; 32]);
        let ab = liquidity_quote(quote_request(token_a, token_b)).unwrap();
        // Swap display order and the paired amounts: the LP figure is symmetric.
        let mut ba = quote_request(token_b, token_a);
        ba.amount_a_raw = Some(String::from("4000000"));
        ba.amount_b_raw = Some(String::from("1000000"));
        let ba = liquidity_quote(ba).unwrap();
        assert_eq!(ab["expectedLpRaw"], ba["expectedLpRaw"]);
    }

    #[test]
    fn create_quote_rejects_same_token_and_tiny_amounts() {
        let token = AccountId::new([0xAA; 32]);
        assert_eq!(
            liquidity_quote(quote_request(token, token)),
            Err(String::from("same_token_pair"))
        );

        // isqrt(1 * 1) = 1 ≤ MINIMUM_LIQUIDITY ⇒ the pool can't open.
        let token_b = AccountId::new([0xBB; 32]);
        let mut tiny = quote_request(token, token_b);
        tiny.amount_a_raw = Some(String::from("1"));
        tiny.amount_b_raw = Some(String::from("1"));
        assert_eq!(liquidity_quote(tiny), Err(String::from("amount_too_low")));
    }

    #[test]
    fn create_plan_pairs_each_amount_with_its_canonical_vault() {
        let program = "00".repeat(32);
        let amm = parse_program_id(&program).unwrap();

        // Display order is NON-canonical (token_a < token_b), so canonical `a` is the
        // display `b`. This is the case where a misapplied swap would corrupt balances.
        let token_a = AccountId::new([0x11; 32]);
        let token_b = AccountId::new([0x22; 32]);
        assert!(!is_canonical_pair(token_a, token_b));
        let (canonical_a, canonical_b) = (token_b, token_a);

        let holding_a = AccountId::new([0x0A; 32]); // holds display token_a
        let holding_b = AccountId::new([0x0B; 32]); // holds display token_b (canonical a)
        let lp = AccountId::new([0x0C; 32]);

        let value = create_pool_plan(CreatePoolPlanRequest {
            amm_program_id: program.clone(),
            config: valid_config(amm),
            token_a_id: account_id_hex(token_a),
            token_b_id: account_id_hex(token_b),
            amount_a_raw: Some(String::from("1000000")), // deposit for display token_a
            amount_b_raw: Some(String::from("4000000")), // deposit for display token_b
            fee_bps: 30,
            deadline_ms: String::from("1000"),
            user_holding_a_id: account_id_hex(holding_a),
            user_holding_b_id: account_id_hex(holding_b),
            user_holding_lp_id: account_id_hex(lp),
        })
        .unwrap();

        let ids: Vec<&str> = value["accountIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let signers: Vec<bool> = value["signingRequirements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_bool().unwrap())
            .collect();

        let pool = compute_pool_pda(amm, canonical_a, canonical_b);
        assert_eq!(ids[0], account_id_hex(compute_config_pda(amm)));
        assert_eq!(ids[1], account_id_hex(pool));
        // Canonical vaults, in canonical order.
        assert_eq!(
            ids[2],
            account_id_hex(compute_vault_pda(amm, pool, canonical_a))
        );
        assert_eq!(
            ids[3],
            account_id_hex(compute_vault_pda(amm, pool, canonical_b))
        );
        // user_holding_a is the CANONICAL token-a holding = display token_b's holding.
        assert_eq!(ids[6], account_id_hex(holding_b));
        assert_eq!(ids[7], account_id_hex(holding_a));
        assert_eq!(ids[8], account_id_hex(lp));
        assert_eq!(
            signers,
            vec![false, false, false, false, false, false, true, true, true, false, false]
        );

        // The decisive check: the encoded instruction must carry token_a_amount =
        // 4_000_000 (the deposit into canonical vault_a) — the amount the user entered
        // for THAT token (display token_b), not display token_a's 1_000_000. Comparing
        // against the re-encoded expected instruction proves balances follow their
        // tokens through the swap.
        let expected = risc0_zkvm::serde::to_vec(&amm_core::Instruction::NewDefinition {
            token_a_amount: 4_000_000,
            token_b_amount: 1_000_000,
            fees: 30,
            deadline: 1_000,
        })
        .unwrap();
        let expected_words: Vec<u64> = expected.iter().map(|word| u64::from(*word)).collect();
        assert_eq!(value["instruction"], serde_json::json!(expected_words));
    }

    #[test]
    fn create_plan_rejects_same_token() {
        let token = AccountId::new([0xAA; 32]);
        let value = create_pool_plan(CreatePoolPlanRequest {
            amm_program_id: "00".repeat(32),
            config: read_failed(),
            token_a_id: account_id_hex(token),
            token_b_id: account_id_hex(token),
            amount_a_raw: Some(String::from("1")),
            amount_b_raw: Some(String::from("1")),
            fee_bps: 30,
            deadline_ms: String::from("1"),
            user_holding_a_id: account_id_hex(token),
            user_holding_b_id: account_id_hex(token),
            user_holding_lp_id: account_id_hex(token),
        });
        assert_eq!(value, Err(String::from("same_token_pair")));
    }

    fn pool_hex(pool: &PoolDefinition) -> String {
        hex::encode(borsh::to_vec(pool).unwrap())
    }

    #[test]
    fn add_quote_prices_via_guest_formula_and_orients() {
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

        // Display == canonical. maxB is generous, so token A's cap binds and token B is
        // ratio-matched down to 20_000 (proving the ideal→actual clamp).
        let ab = add_liquidity_quote(AddLiquidityQuoteRequest {
            token_a_id: account_id_hex(def_a),
            token_b_id: account_id_hex(def_b),
            max_amount_a_raw: String::from("10000"),
            max_amount_b_raw: String::from("100000"),
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ab["amountARaw"], "10000");
        assert_eq!(ab["amountBRaw"], "20000");
        assert_eq!(ab["expectedLpRaw"], "10000");
        assert_eq!(
            ab["priceRaw"],
            spot_price_q64_64(1_000_000, 2_000_000).to_string()
        );
        // Shape parity with create, minus the create-only locked LP and with priceRaw.
        assert!(ab.get("lockedLpRaw").is_none());
        assert!(ab.get("initialPriceRaw").is_none());

        // Reverse display order: the actual amounts and the price flip to display order.
        let ba = add_liquidity_quote(AddLiquidityQuoteRequest {
            token_a_id: account_id_hex(def_b),
            token_b_id: account_id_hex(def_a),
            max_amount_a_raw: String::from("100000"),
            max_amount_b_raw: String::from("10000"),
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ba["amountARaw"], "20000"); // display token def_b side
        assert_eq!(ba["amountBRaw"], "10000"); // display token def_a side
        assert_eq!(ba["expectedLpRaw"], "10000");
        assert_eq!(
            ba["priceRaw"],
            spot_price_q64_64(2_000_000, 1_000_000).to_string()
        );
    }

    #[test]
    fn add_quote_rejects_no_pool_mismatch_and_tiny_deposits() {
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
        let req =
            |token_a: AccountId, token_b: AccountId, max_a: &str, max_b: &str, data: String| {
                AddLiquidityQuoteRequest {
                    token_a_id: account_id_hex(token_a),
                    token_b_id: account_id_hex(token_b),
                    max_amount_a_raw: max_a.into(),
                    max_amount_b_raw: max_b.into(),
                    pool_data: data,
                }
            };

        // Same token pair.
        assert_eq!(
            add_liquidity_quote(req(def_a, def_a, "1", "1", pool_hex(&pool))),
            Err(String::from("same_token_pair"))
        );
        // Empty / undecodable pool data.
        assert_eq!(
            add_liquidity_quote(req(def_a, def_b, "1", "1", String::new())),
            Err(String::from("no_pool"))
        );
        // Zero-supply pool.
        let empty = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 0,
            ..Default::default()
        };
        assert_eq!(
            add_liquidity_quote(req(def_a, def_b, "1", "1", pool_hex(&empty))),
            Err(String::from("no_pool"))
        );
        // A decoded pool that isn't for this pair.
        let other = AccountId::new([0xCC; 32]);
        assert_eq!(
            add_liquidity_quote(req(def_a, other, "1", "1", pool_hex(&pool))),
            Err(String::from("pair_mismatch"))
        );
        // Deposits so small the minted LP rounds to zero.
        assert_eq!(
            add_liquidity_quote(req(def_a, def_b, "1", "1", pool_hex(&pool))),
            Err(String::from("amount_too_low"))
        );
    }

    #[test]
    fn add_plan_orients_holdings_to_the_pools_stored_order() {
        let program = "00".repeat(32);
        let amm = parse_program_id(&program).unwrap();

        // token_b is is_canonical_pair's "canonical a" (larger id), but the pool was created in
        // the OPPOSITE order — definition_token_a_id = token_a — as a pool created outside the
        // FFI can be (e.g. the testnet setup's `spel new-definition`). The plan must follow the
        // POOL's stored order, NOT is_canonical_pair, or a holding lands in the wrong vault and
        // the token program rejects the transfer on a sender/recipient definition mismatch.
        let token_a = AccountId::new([0x11; 32]);
        let token_b = AccountId::new([0x22; 32]);
        assert!(!is_canonical_pair(token_a, token_b)); // is_canonical_pair's canonical-a is token_b

        let vault_a = AccountId::new([0xA1; 32]); // vault for token_a
        let vault_b = AccountId::new([0xB1; 32]); // vault for token_b
        let lp_def = AccountId::new([0xCC; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: token_a, // stored NON-canonically (token_a first)
            definition_token_b_id: token_b,
            vault_a_id: vault_a,
            vault_b_id: vault_b,
            liquidity_pool_id: lp_def,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
        };

        let holding_a = AccountId::new([0x0A; 32]); // token_a holding
        let holding_b = AccountId::new([0x0B; 32]); // token_b holding
        let lp = AccountId::new([0x0C; 32]);

        // Run the plan for a caller ordering; returns (accountIds, instruction words).
        let run = |ta: String, tb: String, ma: &str, mb: &str, ha: String, hb: String| {
            let value = add_liquidity_plan(AddLiquidityPlanRequest {
                amm_program_id: program.clone(),
                config: valid_config(amm),
                token_a_id: ta,
                token_b_id: tb,
                max_amount_a_raw: ma.to_string(),
                max_amount_b_raw: mb.to_string(),
                min_lp_raw: String::from("500"),
                deadline_ms: String::from("1000"),
                user_holding_a_id: ha,
                user_holding_b_id: hb,
                user_holding_lp_id: account_id_hex(lp),
                pool_data: pool_hex(&pool),
            })
            .unwrap();
            let ids = value["accountIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<String>>();
            (ids, value["instruction"].clone())
        };

        // The instruction the guest must receive: token_a's cap into vault_a (token_a), token_b's
        // cap into vault_b — regardless of the caller's argument order.
        let expected_instruction = {
            let words = risc0_zkvm::serde::to_vec(&amm_core::Instruction::AddLiquidity {
                min_amount_liquidity: 500,
                max_amount_to_add_token_a: 1_000_000, // token_a's cap
                max_amount_to_add_token_b: 4_000_000, // token_b's cap
                deadline: 1_000,
            })
            .unwrap();
            serde_json::json!(words.iter().map(|w| u64::from(*w)).collect::<Vec<u64>>())
        };
        let assert_aligned = |ids: &[String], instruction: &serde_json::Value| {
            assert_eq!(ids[0], account_id_hex(compute_config_pda(amm)));
            assert_eq!(
                ids[1],
                account_id_hex(compute_pool_pda(amm, token_a, token_b))
            );
            assert_eq!(ids[2], account_id_hex(vault_a));
            assert_eq!(ids[3], account_id_hex(vault_b));
            assert_eq!(ids[4], account_id_hex(lp_def));
            // user_holding_a is token_a's holding — the token vault_a holds — NOT the
            // is_canonical_pair canonical-a (token_b) holding.
            assert_eq!(ids[5], account_id_hex(holding_a));
            assert_eq!(ids[6], account_id_hex(holding_b));
            assert_eq!(ids[7], account_id_hex(lp));
            assert_eq!(instruction, &expected_instruction);
        };

        // Caller order == the pool's stored order → NO swap (is_canonical_pair WOULD swap here).
        let (ids, instruction) = run(
            account_id_hex(token_a),
            account_id_hex(token_b),
            "1000000",
            "4000000",
            account_id_hex(holding_a),
            account_id_hex(holding_b),
        );
        assert_aligned(&ids, &instruction);

        // Caller order reversed vs the pool → SWAP, so user_a stays token_a's (vault_a's) holding
        // and each cap follows its token.
        let (ids, instruction) = run(
            account_id_hex(token_b),
            account_id_hex(token_a),
            "4000000",
            "1000000",
            account_id_hex(holding_b),
            account_id_hex(holding_a),
        );
        assert_aligned(&ids, &instruction);
    }

    #[test]
    fn add_plan_fails_closed() {
        let token = AccountId::new([0xAA; 32]);
        let other = AccountId::new([0xBB; 32]);
        let base =
            |token_a: AccountId, token_b: AccountId, pool_data: String| AddLiquidityPlanRequest {
                amm_program_id: "00".repeat(32),
                config: read_failed(),
                token_a_id: account_id_hex(token_a),
                token_b_id: account_id_hex(token_b),
                max_amount_a_raw: String::from("1"),
                max_amount_b_raw: String::from("1"),
                min_lp_raw: String::from("1"),
                deadline_ms: String::from("1"),
                user_holding_a_id: account_id_hex(token_a),
                user_holding_b_id: account_id_hex(token_b),
                user_holding_lp_id: account_id_hex(token_a),
                pool_data,
            };

        // Same token pair — rejected before any config/pool work.
        assert_eq!(
            add_liquidity_plan(base(token, token, String::new())),
            Err(String::from("same_token_pair"))
        );
        // Unavailable config (read_failed) surfaces before the pool decode.
        assert_eq!(
            add_liquidity_plan(base(token, other, String::new())),
            Err(String::from("config_unavailable"))
        );
        // Valid config but no pool data → no_pool (decode happens after derive_pair).
        let amm = parse_program_id(&"00".repeat(32)).unwrap();
        let mut no_pool = base(token, other, String::new());
        no_pool.config = valid_config(amm);
        assert_eq!(add_liquidity_plan(no_pool), Err(String::from("no_pool")));
    }
}
