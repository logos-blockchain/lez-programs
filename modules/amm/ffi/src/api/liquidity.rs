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

use amm_core::{isqrt_product, spot_price_q64_64, MINIMUM_LIQUIDITY};
use serde_json::{json, Value};

use super::{
    pair::{derive_pair, is_canonical_pair},
    CreatePoolPlanRequest, LiquidityQuoteRequest,
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
    let reversed = !is_canonical_pair(token_a, token_b);
    let (canonical_a, canonical_b, canonical_amount_a, canonical_amount_b, user_a, user_b) =
        if reversed {
            (token_b, token_a, amount_b, amount_a, holding_b, holding_a)
        } else {
            (token_a, token_b, amount_a, amount_b, holding_a, holding_b)
        };

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

    Ok(json!({
        "programId": request.amm_program_id,
        "accountIds": account_ids.into_iter().map(account_id_hex).collect::<Vec<_>>(),
        "signingRequirements": signing_requirements,
        "instruction": instruction,
    }))
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
}
