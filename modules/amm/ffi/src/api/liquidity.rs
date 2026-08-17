//! Liquidity operations — pool-creation quoting and the `NewDefinition` submission
//! plan. Same lean, transport-independent pattern as `swap.rs`: pure functions
//! returning JSON `Value`, and the token pair canonicalized server-side so callers
//! keep no ordering logic.
//!
//! `create_pool_quote` is a **pure create-pool preview**: a function of the caller's
//! own inputs (the two deposit amounts) with no chain reads and no commitment
//! — it prices the opening LP and price via the same `amm_core` primitives the guest
//! runs (`isqrt_product`, `MINIMUM_LIQUIDITY`, `spot_price_q64_64`), so the preview
//! equals what `new_definition` mints. The caller decides create-vs-add by pool
//! existence before calling; a stale preview or a raced create just reverts on the
//! guest's `assert pool uninitialized`.

use amm_core::{
    isqrt_product, mul_div_floor, spot_price_q64_64, PoolDefinition, FEE_BPS_DENOMINATOR,
    MINIMUM_LIQUIDITY,
};
use nssa_core::account::AccountId;
use serde_json::{json, Value};

use super::{
    pair::{derive_pair, is_canonical_pair},
    quote::minimum_opening_pair,
    AddLiquidityPlanRequest, AddLiquidityQuoteRequest, CreatePoolPlanRequest,
    CreatePoolQuoteRequest, RemoveLiquidityPlanRequest, RemoveLiquidityQuoteRequest,
    SyncReservesPlanRequest,
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

/// Prices a create-pool deposit — dual mode, matching the legacy create quote (minus its
/// funding/account-preview machinery). Pure: no chain reads, no fee (the fee isn't part of
/// the pool PDA nor the pricing).
///
/// The opening price *is* the deposit ratio. With **amounts** supplied, the op uses them and
/// derives the price (`spot_price_q64_64`); **price-only** (no amounts), it takes
/// `price` (Q64.64, canonical) and uses `minimum_opening_pair` — the smallest
/// deposit at that price that clears the permanently-locked `MINIMUM_LIQUIDITY`. Either way it
/// also returns that `minimum*` pair (the form validates entered amounts against it) and
/// `expected_lp = floor(sqrt(a·b)) - MINIMUM_LIQUIDITY` (LP is orientation-independent — the
/// product is symmetric). Errors: `same_token_pair`, `amount_required` (price-only without a
/// price), `invalid_raw_amount`, `amount_must_be_positive`, `amount_too_low` (deposits too
/// small to clear the locked minimum).
pub(super) fn create_pool_quote(request: CreatePoolQuoteRequest) -> Result<Value, String> {
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }

    // Amounts define the opening price; without them the price input drives the minimum.
    let amounts = if request.amount_a.is_some() || request.amount_b.is_some() {
        Some((
            positive_amount(request.amount_a.as_deref())?,
            positive_amount(request.amount_b.as_deref())?,
        ))
    } else {
        None
    };
    let price = match amounts {
        Some((amount_a, amount_b)) => spot_price_q64_64(amount_a, amount_b),
        None => positive_amount(request.price.as_deref())?,
    };
    let (minimum_a, minimum_b) = minimum_opening_pair(price)?;
    let (actual_a, actual_b) = amounts.unwrap_or((minimum_a, minimum_b));

    // LP math (shared with the guest's new_definition via amm_core): the initial LP must
    // clear the permanently-locked minimum before the creator receives any.
    let initial_lp = isqrt_product(actual_a, actual_b);
    let expected_lp = initial_lp
        .checked_sub(MINIMUM_LIQUIDITY)
        .filter(|user_lp| *user_lp > 0)
        .ok_or("amount_too_low")?;

    Ok(json!({
        "actualAmountA": actual_a.to_string(),
        "actualAmountB": actual_b.to_string(),
        "minimumAmountA": minimum_a.to_string(),
        "minimumAmountB": minimum_b.to_string(),
        "expectedLp": expected_lp.to_string(),
        "lockedLp": MINIMUM_LIQUIDITY.to_string(),
        "price": price.to_string(),
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

    let amount_a = positive_amount(request.amount_a.as_deref())?;
    let amount_b = positive_amount(request.amount_b.as_deref())?;
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
/// actual ratio-matched deposits (display order), the LP minted (`expectedLp`), the
/// slippage floor on that LP (`minimumLp = floor(delta_lp · (1 − slippage))`, the
/// submit's `min_amount_liquidity` — like the swap quotes' `minReceived`), and the pool's
/// spot price (`price`, token B per token A in display order). Errors: `same_token_pair`,
/// `no_pool`, `pair_mismatch` (the pool isn't for this pair), `invalid_slippage` (≥ 100%),
/// bad amounts (`amount_required`, `invalid_raw_amount`, `amount_must_be_positive`),
/// `amount_too_low` (the deposit rounds to zero LP), `minimum_lp_zero` (slippage leaves no
/// LP floor — the guest requires a nonzero minimum).
pub(super) fn add_liquidity_quote(request: AddLiquidityQuoteRequest) -> Result<Value, String> {
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let max_a = positive_amount(Some(&request.max_amount_a))?;
    let max_b = positive_amount(Some(&request.max_amount_b))?;
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

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

    // Slippage floor on the LP minted (orientation-independent — LP is symmetric). The guest
    // requires a nonzero `min_amount_liquidity`, so reject a slippage that rounds it to zero.
    let slippage_complement = FEE_BPS_DENOMINATOR - u128::from(request.slippage_bps);
    let minimum_lp = mul_div_floor(delta_lp, slippage_complement, FEE_BPS_DENOMINATOR);
    if minimum_lp == 0 {
        return Err(String::from("minimum_lp_zero"));
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
        "amountA": display_a.to_string(),
        "amountB": display_b.to_string(),
        "expectedLp": delta_lp.to_string(),
        "minimumLp": minimum_lp.to_string(),
        "price": price.to_string(),
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

    let max_a = positive_amount(Some(&request.max_amount_a))?;
    let max_b = positive_amount(Some(&request.max_amount_b))?;
    let min_lp = positive_amount(Some(&request.min_lp))?;
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

/// Prices removing liquidity: burning `lp_amount` of the pool returns the proportional
/// share of each reserve — `withdraw = floor(reserve · lp / supply)`, the same math the guest
/// (`amm_program::remove::remove_liquidity`) runs. `slippage_bps` sets the `minimumAmount*Raw`
/// floors the submit passes as the guest's nonzero `min_amount_to_remove_token_*`. Amounts are
/// returned in the caller's (display) token order. Errors: `same_token_pair`, `invalid_slippage`,
/// `no_pool`, `insufficient_pool_liquidity` (the burn exceeds the supply unlocked above the
/// permanently-locked `MINIMUM_LIQUIDITY`), `pair_mismatch`, `amount_too_low` (a withdrawal
/// rounds to zero), `minimum_amount_zero` (slippage rounds a floor to zero), plus the shared
/// amount-parse codes.
pub(super) fn remove_liquidity_quote(
    request: RemoveLiquidityQuoteRequest,
) -> Result<Value, String> {
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let lp_amount = positive_amount(Some(&request.lp_amount))?;
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

    // Decode the pool; absent / undecodable / zero-supply ⇒ nothing to remove from.
    let pool = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
        .filter(|pool| pool.liquidity_pool_supply != 0)
        .ok_or_else(|| String::from("no_pool"))?;
    if pool.reserve_a == 0 || pool.reserve_b == 0 {
        return Err(String::from("no_pool"));
    }

    // The pool permanently locks MINIMUM_LIQUIDITY at creation, so a burn can only draw on the
    // supply beyond it — the guest asserts `remove_amount <= supply - MINIMUM_LIQUIDITY`.
    let unlocked = pool
        .liquidity_pool_supply
        .checked_sub(MINIMUM_LIQUIDITY)
        .filter(|unlocked| *unlocked > 0)
        .ok_or_else(|| String::from("no_pool"))?;
    if lp_amount > unlocked {
        return Err(String::from("insufficient_pool_liquidity"));
    }

    // Orient the caller's (display) tokens to the pool's canonical (a, b) order so the reserve
    // math lines up with the guest; `reversed` flips the results back to display order.
    let reversed = if token_a == pool.definition_token_a_id && token_b == pool.definition_token_b_id
    {
        false
    } else if token_a == pool.definition_token_b_id && token_b == pool.definition_token_a_id {
        true
    } else {
        return Err(String::from("pair_mismatch"));
    };

    // Guest math: floor(reserve · lp / supply) per side.
    let withdraw_a = mul_div_floor(pool.reserve_a, lp_amount, pool.liquidity_pool_supply);
    let withdraw_b = mul_div_floor(pool.reserve_b, lp_amount, pool.liquidity_pool_supply);
    if withdraw_a == 0 || withdraw_b == 0 {
        return Err(String::from("amount_too_low"));
    }

    // Slippage floors — the guest requires both `min_amount_to_remove_token_*` nonzero.
    let slippage_complement = FEE_BPS_DENOMINATOR - u128::from(request.slippage_bps);
    let minimum_a = mul_div_floor(withdraw_a, slippage_complement, FEE_BPS_DENOMINATOR);
    let minimum_b = mul_div_floor(withdraw_b, slippage_complement, FEE_BPS_DENOMINATOR);
    if minimum_a == 0 || minimum_b == 0 {
        return Err(String::from("minimum_amount_zero"));
    }

    // Back to display order for the response; the price uses the display-oriented reserves.
    let (display_a, display_b, minimum_display_a, minimum_display_b) = if reversed {
        (withdraw_b, withdraw_a, minimum_b, minimum_a)
    } else {
        (withdraw_a, withdraw_b, minimum_a, minimum_b)
    };
    let (reserve_display_a, reserve_display_b) = if reversed {
        (pool.reserve_b, pool.reserve_a)
    } else {
        (pool.reserve_a, pool.reserve_b)
    };
    let price = spot_price_q64_64(reserve_display_a, reserve_display_b);

    Ok(json!({
        "amountA": display_a.to_string(),
        "amountB": display_b.to_string(),
        "minimumAmountA": minimum_display_a.to_string(),
        "minimumAmountB": minimum_display_b.to_string(),
        "price": price.to_string(),
    }))
}

/// Builds the `RemoveLiquidity` submission for an existing pool. Like `add_liquidity_plan` it
/// orients the `(min_amount, holding)` pair to the pool's ALREADY-STORED order — vaults and the
/// LP definition come from `pool_data`, which the guest asserts against — but only
/// `user_holding_lp` signs (it is burned), and there is no fresh holding: the existing token
/// a/b holdings receive the withdrawal. `min_amount_*_raw` are the caller's slippage floors and
/// must be positive (the guest rejects a zero). Emits the fixed 10-account IDL order.
/// Recoverable failures fail closed as `Err` (`same_token_pair`, `config_unavailable`,
/// `no_pool`, `pair_mismatch`, bad amounts).
pub(super) fn remove_liquidity_plan(request: RemoveLiquidityPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }
    let holding_a = account_id_from_hex(&request.user_holding_a_id, "user holding A id")?;
    let holding_b = account_id_from_hex(&request.user_holding_b_id, "user holding B id")?;
    let user_lp = account_id_from_hex(&request.user_holding_lp_id, "user LP holding id")?;

    let lp_amount = positive_amount(Some(&request.lp_amount))?;
    let min_a = positive_amount(Some(&request.min_amount_a))?;
    let min_b = positive_amount(Some(&request.min_amount_b))?;
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;

    // config / pool / current_tick / clock are order-independent PDAs, so derive_pair takes the
    // tokens in the caller's order. (Its vaults are canonical and unused here — the guest
    // asserts the vaults against the pool's stored ids, taken below.)
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

    // Orient (min amount, holding) to the pool's STORED order — NOT is_canonical_pair. The guest
    // transfers vault_a (== pool.vault_a_id, which holds definition_token_a_id) into
    // user_holding_a, so user_a / min_pool_a must be that token's holding / floor. A pool created
    // outside the FFI can store a non-canonical order, so keying off is_canonical_pair would
    // route a withdrawal into the wrong holding.
    let (min_pool_a, min_pool_b, user_a, user_b) =
        if token_a == pool.definition_token_a_id && token_b == pool.definition_token_b_id {
            (min_a, min_b, holding_a, holding_b)
        } else if token_a == pool.definition_token_b_id && token_b == pool.definition_token_a_id {
            (min_b, min_a, holding_b, holding_a)
        } else {
            return Err(String::from("pair_mismatch"));
        };

    let instruction = risc0_zkvm::serde::to_vec(&amm_core::Instruction::RemoveLiquidity {
        remove_liquidity_amount: lp_amount,
        min_amount_to_remove_token_a: min_pool_a,
        min_amount_to_remove_token_b: min_pool_b,
        deadline,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for RemoveLiquidity; only user_holding_lp (burned) signs.
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
        false, false, false, false, false, false, false, true, false, false,
    ];

    Ok(plan_response(
        &request.amm_program_id,
        account_ids,
        &signing_requirements,
        instruction,
    ))
}

/// Builds the `SyncReserves` submission — a permissionless keeper op that refreshes the pool's
/// stored reserves to the live vault balances (and its TWAP tick). A unit instruction: no
/// amounts, deadline, or holdings, and nothing signs. config / pool / current_tick / clock are
/// order-independent PDAs from `derive_pair`; the vaults come from the pool's stored ids in
/// `pool_data` (read-only, but the guest still asserts them). Fixed 6-account IDL order.
/// Recoverable failures fail closed as `Err` (`same_token_pair`, `config_unavailable`,
/// `no_pool`).
pub(super) fn sync_reserves_plan(request: SyncReservesPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_a = account_id_from_hex(&request.token_a_id, "token A id")?;
    let token_b = account_id_from_hex(&request.token_b_id, "token B id")?;
    if token_a == token_b {
        return Err(String::from("same_token_pair"));
    }

    // config / pool / current_tick / clock are order-independent PDAs, so the token order does
    // not matter for derive_pair.
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    // The vaults are asserted against the pool's stored ids, so take them from pool_data (a pool
    // created outside the FFI can store a non-canonical order — see add/remove/swap plans).
    let Some(pool) = hex::decode(&request.pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
    else {
        return Err(String::from("no_pool"));
    };

    let instruction = risc0_zkvm::serde::to_vec(&amm_core::Instruction::SyncReserves)
        .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // Fixed IDL account order for SyncReserves; nothing signs (permissionless keeper op).
    let account_ids = [
        pair.config,
        pair.pool,
        pool.vault_a_id,
        pool.vault_b_id,
        pair.current_tick,
        pair.clock,
    ];
    let signing_requirements = [false, false, false, false, false, false];

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

    fn quote_request(token_a: AccountId, token_b: AccountId) -> CreatePoolQuoteRequest {
        CreatePoolQuoteRequest {
            token_a_id: account_id_hex(token_a),
            token_b_id: account_id_hex(token_b),
            price: None,
            amount_a: Some(String::from("1000000")),
            amount_b: Some(String::from("4000000")),
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
    fn create_quote_prices_supplied_amounts() {
        let token_a = AccountId::new([0xAA; 32]);
        let token_b = AccountId::new([0xBB; 32]);
        let value = create_pool_quote(quote_request(token_a, token_b)).unwrap();

        // Amounts supplied ⇒ actual == the amounts; the price is derived from them.
        assert_eq!(value["actualAmountA"], "1000000");
        assert_eq!(value["actualAmountB"], "4000000");
        assert_eq!(value["lockedLp"], MINIMUM_LIQUIDITY.to_string());
        // initial_lp = isqrt(1_000_000 * 4_000_000) = 2_000_000; creator LP = minus lock.
        let initial_lp = isqrt_product(1_000_000, 4_000_000);
        assert_eq!(
            value["expectedLp"],
            (initial_lp - MINIMUM_LIQUIDITY).to_string()
        );
        let price = spot_price_q64_64(1_000_000, 4_000_000);
        assert_eq!(value["price"], price.to_string());
        // The minimum opening deposit for that price is echoed for the form to validate against.
        let (min_a, min_b) = minimum_opening_pair(price).unwrap();
        assert_eq!(value["minimumAmountA"], min_a.to_string());
        assert_eq!(value["minimumAmountB"], min_b.to_string());
        // Lean preview — no commitment / status / submittability fields.
        assert!(value.get("quoteHash").is_none());
        assert!(value.get("canSubmit").is_none());
        assert!(value.get("poolStatus").is_none());
    }

    #[test]
    fn create_quote_price_only_returns_the_minimum_opening_deposit() {
        let token_a = AccountId::new([0xAA; 32]);
        let token_b = AccountId::new([0xBB; 32]);
        let price = spot_price_q64_64(1_000_000, 4_000_000);
        let (min_a, min_b) = minimum_opening_pair(price).unwrap();

        let value = create_pool_quote(CreatePoolQuoteRequest {
            token_a_id: account_id_hex(token_a),
            token_b_id: account_id_hex(token_b),
            price: Some(price.to_string()),
            amount_a: None,
            amount_b: None,
        })
        .unwrap();

        // Price-only ⇒ the actual deposit is the minimum opening pair for that price.
        assert_eq!(value["actualAmountA"], min_a.to_string());
        assert_eq!(value["actualAmountB"], min_b.to_string());
        assert_eq!(value["minimumAmountA"], min_a.to_string());
        assert_eq!(value["minimumAmountB"], min_b.to_string());
        assert_eq!(value["price"], price.to_string());
    }

    #[test]
    fn create_quote_lp_is_orientation_independent() {
        let token_a = AccountId::new([0xAA; 32]);
        let token_b = AccountId::new([0xBB; 32]);
        let ab = create_pool_quote(quote_request(token_a, token_b)).unwrap();
        // Swap display order and the paired amounts: the LP figure is symmetric.
        let mut ba = quote_request(token_b, token_a);
        ba.amount_a = Some(String::from("4000000"));
        ba.amount_b = Some(String::from("1000000"));
        let ba = create_pool_quote(ba).unwrap();
        assert_eq!(ab["expectedLp"], ba["expectedLp"]);
    }

    #[test]
    fn create_quote_rejects_same_token_and_tiny_amounts() {
        let token = AccountId::new([0xAA; 32]);
        assert_eq!(
            create_pool_quote(quote_request(token, token)),
            Err(String::from("same_token_pair"))
        );

        // isqrt(1 * 1) = 1 ≤ MINIMUM_LIQUIDITY ⇒ the pool can't open.
        let token_b = AccountId::new([0xBB; 32]);
        let mut tiny = quote_request(token, token_b);
        tiny.amount_a = Some(String::from("1"));
        tiny.amount_b = Some(String::from("1"));
        assert_eq!(create_pool_quote(tiny), Err(String::from("amount_too_low")));
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
            amount_a: Some(String::from("1000000")), // deposit for display token_a
            amount_b: Some(String::from("4000000")), // deposit for display token_b
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
            amount_a: Some(String::from("1")),
            amount_b: Some(String::from("1")),
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
            max_amount_a: String::from("10000"),
            max_amount_b: String::from("100000"),
            slippage_bps: 50,
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ab["amountA"], "10000");
        assert_eq!(ab["amountB"], "20000");
        assert_eq!(ab["expectedLp"], "10000");
        // minimumLp = floor(10000 * (10000 - 50) / 10000) = 9950 (slippage floor on LP).
        assert_eq!(ab["minimumLp"], "9950");
        assert_eq!(
            ab["price"],
            spot_price_q64_64(1_000_000, 2_000_000).to_string()
        );
        assert!(ab.get("lockedLp").is_none());
        assert!(ab.get("initialPrice").is_none());

        // Reverse display order: the actual amounts and the price flip to display order.
        let ba = add_liquidity_quote(AddLiquidityQuoteRequest {
            token_a_id: account_id_hex(def_b),
            token_b_id: account_id_hex(def_a),
            max_amount_a: String::from("100000"),
            max_amount_b: String::from("10000"),
            slippage_bps: 50,
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ba["amountA"], "20000"); // display token def_b side
        assert_eq!(ba["amountB"], "10000"); // display token def_a side
        assert_eq!(ba["expectedLp"], "10000");
        assert_eq!(
            ba["price"],
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
                    max_amount_a: max_a.into(),
                    max_amount_b: max_b.into(),
                    slippage_bps: 50,
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
        // A deposit that mints only 1 LP: 50 bps slippage floors the minimum to 0, which the
        // guest's nonzero `min_amount_liquidity` rejects.
        assert_eq!(
            add_liquidity_quote(req(def_a, def_b, "1", "2", pool_hex(&pool))),
            Err(String::from("minimum_lp_zero"))
        );
        // Slippage at/above 100%.
        assert_eq!(
            add_liquidity_quote(AddLiquidityQuoteRequest {
                token_a_id: account_id_hex(def_a),
                token_b_id: account_id_hex(def_b),
                max_amount_a: String::from("10000"),
                max_amount_b: String::from("10000"),
                slippage_bps: 10_000,
                pool_data: pool_hex(&pool),
            }),
            Err(String::from("invalid_slippage"))
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
                max_amount_a: ma.to_string(),
                max_amount_b: mb.to_string(),
                min_lp: String::from("500"),
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
                max_amount_a: String::from("1"),
                max_amount_b: String::from("1"),
                min_lp: String::from("1"),
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

    #[test]
    fn remove_quote_prices_via_guest_formula_and_orients() {
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

        // Display == canonical. Burning 10% of supply returns 10% of each reserve.
        let ab = remove_liquidity_quote(RemoveLiquidityQuoteRequest {
            token_a_id: account_id_hex(def_a),
            token_b_id: account_id_hex(def_b),
            lp_amount: String::from("100000"),
            slippage_bps: 50,
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ab["amountA"], "100000"); // floor(1_000_000 * 100_000 / 1_000_000)
        assert_eq!(ab["amountB"], "200000"); // floor(2_000_000 * 100_000 / 1_000_000)
                                             // minimum = floor(withdraw * (10000 - 50) / 10000) — the slippage floor per side.
        assert_eq!(ab["minimumAmountA"], "99500");
        assert_eq!(ab["minimumAmountB"], "199000");
        assert_eq!(
            ab["price"],
            spot_price_q64_64(1_000_000, 2_000_000).to_string()
        );

        // Reverse display order: withdrawals, minimums, and the price all flip to display order.
        let ba = remove_liquidity_quote(RemoveLiquidityQuoteRequest {
            token_a_id: account_id_hex(def_b),
            token_b_id: account_id_hex(def_a),
            lp_amount: String::from("100000"),
            slippage_bps: 50,
            pool_data: pool_hex(&pool),
        })
        .unwrap();
        assert_eq!(ba["amountA"], "200000"); // display token def_b side
        assert_eq!(ba["amountB"], "100000"); // display token def_a side
        assert_eq!(ba["minimumAmountA"], "199000");
        assert_eq!(ba["minimumAmountB"], "99500");
        assert_eq!(
            ba["price"],
            spot_price_q64_64(2_000_000, 1_000_000).to_string()
        );
    }

    #[test]
    fn remove_quote_rejects_no_pool_mismatch_and_bounds() {
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
        let req = |token_a: AccountId, token_b: AccountId, lp: &str, data: String| {
            RemoveLiquidityQuoteRequest {
                token_a_id: account_id_hex(token_a),
                token_b_id: account_id_hex(token_b),
                lp_amount: lp.into(),
                slippage_bps: 50,
                pool_data: data,
            }
        };

        // Same token pair.
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_a, "1", pool_hex(&pool))),
            Err(String::from("same_token_pair"))
        );
        // Empty / undecodable pool data, and a zero-supply pool.
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_b, "1", String::new())),
            Err(String::from("no_pool"))
        );
        let empty = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 0,
            ..Default::default()
        };
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_b, "1", pool_hex(&empty))),
            Err(String::from("no_pool"))
        );
        // Burning more than the supply unlocked above MINIMUM_LIQUIDITY (1_000_000 - 1000).
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_b, "999001", pool_hex(&pool))),
            Err(String::from("insufficient_pool_liquidity"))
        );
        // A decoded pool that isn't for this pair.
        let other = AccountId::new([0xCC; 32]);
        assert_eq!(
            remove_liquidity_quote(req(def_a, other, "1", pool_hex(&pool))),
            Err(String::from("pair_mismatch"))
        );
        // Slippage at/above 100%.
        assert_eq!(
            remove_liquidity_quote(RemoveLiquidityQuoteRequest {
                token_a_id: account_id_hex(def_a),
                token_b_id: account_id_hex(def_b),
                lp_amount: String::from("100000"),
                slippage_bps: 10_000,
                pool_data: pool_hex(&pool),
            }),
            Err(String::from("invalid_slippage"))
        );
        // A lopsided pool where one side's withdrawal floors to zero.
        let lopsided = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 1,
            fees: 30,
            ..Default::default()
        };
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_b, "1", pool_hex(&lopsided))),
            Err(String::from("amount_too_low"))
        );
        // Withdrawals of 1 and 2: 50 bps slippage floors the token-A minimum to 0, which the
        // guest's nonzero `min_amount_to_remove_token_a` rejects.
        assert_eq!(
            remove_liquidity_quote(req(def_a, def_b, "1", pool_hex(&pool))),
            Err(String::from("minimum_amount_zero"))
        );
    }

    #[test]
    fn remove_plan_orients_holdings_to_the_pools_stored_order() {
        let program = "00".repeat(32);
        let amm = parse_program_id(&program).unwrap();

        // Same non-canonical pool as the add-plan test: definition_token_a_id = token_a even
        // though is_canonical_pair's canonical-a is token_b. The plan must follow the POOL's
        // stored order so each withdrawal lands in the right holding.
        let token_a = AccountId::new([0x11; 32]);
        let token_b = AccountId::new([0x22; 32]);
        assert!(!is_canonical_pair(token_a, token_b));

        let vault_a = AccountId::new([0xA1; 32]);
        let vault_b = AccountId::new([0xB1; 32]);
        let lp_def = AccountId::new([0xCC; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: token_a,
            definition_token_b_id: token_b,
            vault_a_id: vault_a,
            vault_b_id: vault_b,
            liquidity_pool_id: lp_def,
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
        };

        let holding_a = AccountId::new([0x0A; 32]); // token_a holding (receives)
        let holding_b = AccountId::new([0x0B; 32]); // token_b holding (receives)
        let lp = AccountId::new([0x0C; 32]); // burned (signs)

        let run = |ta: String,
                   tb: String,
                   min_a: &str,
                   min_b: &str,
                   ha: String,
                   hb: String|
         -> (Vec<String>, serde_json::Value, serde_json::Value) {
            let value = remove_liquidity_plan(RemoveLiquidityPlanRequest {
                amm_program_id: program.clone(),
                config: valid_config(amm),
                token_a_id: ta,
                token_b_id: tb,
                lp_amount: String::from("100000"),
                min_amount_a: min_a.to_string(),
                min_amount_b: min_b.to_string(),
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
            (
                ids,
                value["instruction"].clone(),
                value["signingRequirements"].clone(),
            )
        };

        // The instruction the guest must receive: token_a's floor with vault_a's token,
        // token_b's floor with vault_b's — regardless of the caller's argument order.
        let expected_instruction = {
            let words = risc0_zkvm::serde::to_vec(&amm_core::Instruction::RemoveLiquidity {
                remove_liquidity_amount: 100_000,
                min_amount_to_remove_token_a: 90_000, // token_a's floor
                min_amount_to_remove_token_b: 180_000, // token_b's floor
                deadline: 1_000,
            })
            .unwrap();
            serde_json::json!(words.iter().map(|w| u64::from(*w)).collect::<Vec<u64>>())
        };
        // Only user_holding_lp (index 7) signs — a/b just receive.
        let expected_signers = serde_json::json!([
            false, false, false, false, false, false, false, true, false, false,
        ]);
        let assert_aligned =
            |ids: &[String], instruction: &serde_json::Value, signers: &serde_json::Value| {
                assert_eq!(ids[0], account_id_hex(compute_config_pda(amm)));
                assert_eq!(
                    ids[1],
                    account_id_hex(compute_pool_pda(amm, token_a, token_b))
                );
                assert_eq!(ids[2], account_id_hex(vault_a));
                assert_eq!(ids[3], account_id_hex(vault_b));
                assert_eq!(ids[4], account_id_hex(lp_def));
                assert_eq!(ids[5], account_id_hex(holding_a));
                assert_eq!(ids[6], account_id_hex(holding_b));
                assert_eq!(ids[7], account_id_hex(lp));
                assert_eq!(instruction, &expected_instruction);
                assert_eq!(signers, &expected_signers);
            };

        // Caller order == the pool's stored order → NO swap.
        let (ids, instruction, signers) = run(
            account_id_hex(token_a),
            account_id_hex(token_b),
            "90000",
            "180000",
            account_id_hex(holding_a),
            account_id_hex(holding_b),
        );
        assert_aligned(&ids, &instruction, &signers);

        // Caller order reversed vs the pool → SWAP, so user_a stays token_a's holding and each
        // floor follows its token.
        let (ids, instruction, signers) = run(
            account_id_hex(token_b),
            account_id_hex(token_a),
            "180000",
            "90000",
            account_id_hex(holding_b),
            account_id_hex(holding_a),
        );
        assert_aligned(&ids, &instruction, &signers);
    }

    #[test]
    fn remove_plan_fails_closed() {
        let token = AccountId::new([0xAA; 32]);
        let other = AccountId::new([0xBB; 32]);
        let base = |token_a: AccountId, token_b: AccountId, pool_data: String| {
            RemoveLiquidityPlanRequest {
                amm_program_id: "00".repeat(32),
                config: read_failed(),
                token_a_id: account_id_hex(token_a),
                token_b_id: account_id_hex(token_b),
                lp_amount: String::from("1"),
                min_amount_a: String::from("1"),
                min_amount_b: String::from("1"),
                deadline_ms: String::from("1"),
                user_holding_a_id: account_id_hex(token_a),
                user_holding_b_id: account_id_hex(token_b),
                user_holding_lp_id: account_id_hex(token_a),
                pool_data,
            }
        };

        // Same token pair — rejected before any config/pool work.
        assert_eq!(
            remove_liquidity_plan(base(token, token, String::new())),
            Err(String::from("same_token_pair"))
        );
        // Unavailable config (read_failed) surfaces before the pool decode.
        assert_eq!(
            remove_liquidity_plan(base(token, other, String::new())),
            Err(String::from("config_unavailable"))
        );
        // Valid config but no pool data → no_pool (decode happens after derive_pair).
        let amm = parse_program_id(&"00".repeat(32)).unwrap();
        let mut no_pool = base(token, other, String::new());
        no_pool.config = valid_config(amm);
        assert_eq!(remove_liquidity_plan(no_pool), Err(String::from("no_pool")));
    }

    #[test]
    fn sync_plan_emits_pool_stored_vaults_and_signs_nothing() {
        let program = "00".repeat(32);
        let amm = parse_program_id(&program).unwrap();
        let token_a = AccountId::new([0x11; 32]);
        let token_b = AccountId::new([0x22; 32]);
        let vault_a = AccountId::new([0xA1; 32]);
        let vault_b = AccountId::new([0xB1; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: token_a,
            definition_token_b_id: token_b,
            vault_a_id: vault_a,
            vault_b_id: vault_b,
            liquidity_pool_id: AccountId::new([0xCC; 32]),
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
            fees: 30,
        };
        let value = sync_reserves_plan(SyncReservesPlanRequest {
            amm_program_id: program.clone(),
            config: valid_config(amm),
            token_a_id: account_id_hex(token_a),
            token_b_id: account_id_hex(token_b),
            pool_data: pool_hex(&pool),
        })
        .unwrap();

        let ids = value["accountIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<String>>();
        assert_eq!(ids.len(), 6);
        assert_eq!(ids[0], account_id_hex(compute_config_pda(amm)));
        assert_eq!(
            ids[1],
            account_id_hex(compute_pool_pda(amm, token_a, token_b))
        );
        assert_eq!(ids[2], account_id_hex(vault_a)); // pool's stored vaults
        assert_eq!(ids[3], account_id_hex(vault_b));
        // ids[4] current_tick, ids[5] clock — order-independent PDAs.
        assert_eq!(
            value["signingRequirements"],
            serde_json::json!([false, false, false, false, false, false])
        );
        let expected_instruction = {
            let words = risc0_zkvm::serde::to_vec(&amm_core::Instruction::SyncReserves).unwrap();
            serde_json::json!(words.iter().map(|w| u64::from(*w)).collect::<Vec<u64>>())
        };
        assert_eq!(value["instruction"], expected_instruction);
    }

    #[test]
    fn sync_plan_fails_closed() {
        let token = AccountId::new([0xAA; 32]);
        let other = AccountId::new([0xBB; 32]);
        let base =
            |token_a: AccountId, token_b: AccountId, pool_data: String| SyncReservesPlanRequest {
                amm_program_id: "00".repeat(32),
                config: read_failed(),
                token_a_id: account_id_hex(token_a),
                token_b_id: account_id_hex(token_b),
                pool_data,
            };

        // Same token pair — rejected before any config/pool work.
        assert_eq!(
            sync_reserves_plan(base(token, token, String::new())),
            Err(String::from("same_token_pair"))
        );
        // Unavailable config (read_failed) surfaces before the pool decode.
        assert_eq!(
            sync_reserves_plan(base(token, other, String::new())),
            Err(String::from("config_unavailable"))
        );
        // Valid config but no pool data → no_pool (decode happens after derive_pair).
        let amm = parse_program_id(&"00".repeat(32)).unwrap();
        let mut no_pool = base(token, other, String::new());
        no_pool.config = valid_config(amm);
        assert_eq!(sync_reserves_plan(no_pool), Err(String::from("no_pool")));
    }
}
