//! Swap operations — pool discovery/decode, swap-transaction planning, and
//! program-id derivation. Same transport-independent pattern as the
//! new-position ops: pure functions returning JSON `Value`, PDAs reused from
//! `pair::derive_pair` so the swap path never re-derives seeds.

use amm_core::{
    compute_pool_pda, compute_protocol_fee_pda, mul_div_ceil, mul_div_floor, price_impact_bps,
    protocol_fee_amount, spot_price_q64_64, swap_exact_in_amounts, swap_exact_out_amounts,
    ConfigFacts, PoolDefinition, FEE_BPS_DENOMINATOR,
};
use clock_core::{clock_account_id, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::account::AccountId;
use risc0_binfmt::ProgramBinary;
use serde_json::{json, Value};

use super::{
    clock::now_from_clock,
    config::load_config,
    envelope::{identity_of, plan_response, pool_from_hex, signer, slot},
    pair::{derive_pair, is_canonical_pair},
    PoolIdRequest, ProgramIdRequest, ResolvePoolRequest, SwapExactInPlanRequest,
    SwapExactInQuoteRequest, SwapExactOutPlanRequest, SwapExactOutQuoteRequest, SwapPairRequest,
};
use crate::account::{account_id_from_hex, account_id_hex, parse_program_id, AccountRead};

/// Reads the instance-wide swap fee (basis points) from an AMM config account read. Fees moved
/// from `PoolDefinition` to `AmmConfig::swap_fee_bps`, so quotes and pool resolution source the
/// fee here. Returns `invalid_config` if the read can't be decoded as an `AmmConfig`.
fn swap_fee_bps_from_config(amm_program: AccountId, config: &AccountRead) -> Result<u128, String> {
    load_config(amm_program, config)
        .map(|(_, config)| config.swap_fee_bps)
        .map_err(|_| String::from("invalid_config"))
}

/// Which way a swap is priced.
#[derive(Clone, Copy)]
enum Exactness {
    /// The trader fixes the input; the curve decides the output.
    Input,
    /// The trader fixes the output; the curve decides the input.
    Output,
}

/// The four figures a swap declares beyond the caller's own bounds.
///
/// All four used to be the handler's own business: it read the pool, priced the
/// trade, and wrote the result. A plan phase can read nothing now, so the client
/// derives them here and the pool's effect decides whether they were true -- and
/// it does so with two different kinds of pin side by side. `amount_out` (or
/// `amount_in`) is *bounded*, so a stale read only ever leaves the surplus with
/// the pool; `protocol_fee` and `new_price` are pinned *exactly*, the latter
/// because a tick reaches consumers who act on it. The price of that exactness is
/// that a swap retries when the pool moved underneath it.
#[derive(Debug)]
struct SwapPins {
    amount_in: u128,
    amount_out: u128,
    protocol_fee: u128,
    new_price: u128,
}

/// Prices a swap exactly as `amm_program::swap`'s apply half will re-price it.
///
/// `amount` is the trader's fixed side -- the input for [`Exactness::Input`], the
/// output for [`Exactness::Output`].
fn swap_pins(
    pool: &PoolDefinition,
    facts: &ConfigFacts,
    definition_id_in: AccountId,
    amount: u128,
    exactness: Exactness,
) -> Result<SwapPins, String> {
    let (input, output) = pool
        .sides(definition_id_in)
        .ok_or_else(|| String::from("pair_mismatch"))?;
    let (reserve_in, reserve_out) = (input.reserve, output.reserve);
    if reserve_in == 0 || reserve_out == 0 {
        return Err(String::from("no_pool"));
    }

    let (effective_amount_in, amount_in, amount_out) = match exactness {
        Exactness::Input => {
            let (effective, quote) =
                swap_exact_in_amounts(amount, reserve_in, reserve_out, facts.swap_fee_bps);
            (effective, amount, quote)
        }
        Exactness::Output => {
            let (effective, required_in) =
                swap_exact_out_amounts(amount, reserve_in, reserve_out, facts.swap_fee_bps)
                    .ok_or_else(|| String::from("output_exceeds_liquidity"))?;
            (effective, required_in, amount)
        }
    };
    if amount_in == 0 || amount_out == 0 || amount_out >= reserve_out {
        return Err(String::from("amount_too_small"));
    }

    // The fee split, recomputed on-chain from these same two amounts and the
    // pinned fee tiers: `swap_fee` is what the trader forfeits to the pool,
    // `protocol_fee` the slice of it diverted to the protocol holding.
    let swap_fee = amount_in
        .checked_sub(effective_amount_in)
        .ok_or_else(|| String::from("invalid_fee_tier"))?;
    let protocol_fee = protocol_fee_amount(swap_fee, facts.protocol_fee_bps);

    let net_amount_in = amount_in
        .checked_sub(protocol_fee)
        .ok_or_else(|| String::from("invalid_fee_tier"))?;
    let post_reserve_in = reserve_in
        .checked_add(net_amount_in)
        .ok_or_else(|| String::from("reserve_overflow"))?;
    let post_reserve_out = reserve_out
        .checked_sub(amount_out)
        .ok_or_else(|| String::from("output_exceeds_liquidity"))?;
    let (reserve_a, reserve_b) =
        pool.reserves_in_ab_order(definition_id_in, post_reserve_in, post_reserve_out);

    Ok(SwapPins {
        amount_in,
        amount_out,
        protocol_fee,
        new_price: spot_price_q64_64(reserve_a, reserve_b),
    })
}

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
        // The programs whose shards those last two slots select -- the caller
        // cannot derive either for itself.
        "twapOracleProgramId": account_id_hex(pair.twap_oracle_program),
        "clockProgramId": account_id_hex(clock_account_id()),
    }))
}

/// Decodes a pool account: whether it holds liquidity, its canonical token ids
/// (`defAHex`/`defBHex`), its reserves (same canonical `a`/`b` order — the
/// caller matches a reserve to its own direction by comparing its token id
/// against `defAHex`), and fee tier. On success returns a `{ status: "ok",
/// error: "", poolId, defAHex, defBHex, vaultAId, vaultBId, lpDefinitionId,
/// reserveA, reserveB, liquiditySupply, feeBps }` envelope; an absent / empty /
/// uninitialized pool is the `{ status: "error", error: "no_pool", poolId }`
/// envelope (the derived `poolId` is still carried, for address derivation).
pub(super) fn resolve_pool(request: ResolvePoolRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    // The pool's presence IS the signal: existing pools return their decoded state; a missing /
    // uninitialized pool is the `no_pool` error (still carrying the derived `poolId` for address
    // derivation). The read's id is hex (the module reads by hex); undecodable ⇒ "".
    let pool_id = account_id_from_hex(&request.pool.id, "pool id")
        .map(|id| id.to_string())
        .unwrap_or_default();
    let missing = || json!({ "status": "error", "error": "no_pool", "poolId": pool_id });

    if request.pool.status != "ok" {
        return Ok(missing());
    }
    // The pool read's AMM shard: v0.2.5 decoded `account.data`, which only had one
    // program's bytes in it to begin with.
    let Ok((_, pool_shard)) = crate::account::program_shard(&request.pool, amm_program, "pool")
    else {
        return Ok(missing());
    };
    let Ok(pool) = PoolDefinition::try_from(&pool_shard) else {
        return Ok(missing());
    };
    if pool.liquidity_pool_supply == 0 {
        return Ok(missing());
    }
    // The fee is instance-wide (AmmConfig::swap_fee_bps), not a pool field.
    let fee_bps = u32::try_from(swap_fee_bps_from_config(amm_program, &request.config)?)
        .map_err(|_| String::from("invalid_fee_tier"))?;
    Ok(json!({
        "status": "ok",
        "error": "",
        "poolId": pool_id,
        "defAHex": account_id_hex(pool.definition_token_a_id),
        "defBHex": account_id_hex(pool.definition_token_b_id),
        "vaultAId": pool.vault_a_id.to_string(),
        "vaultBId": pool.vault_b_id.to_string(),
        "lpDefinitionId": pool.liquidity_pool_id.to_string(),
        "reserveA": pool.reserve_a.to_string(),
        "reserveB": pool.reserve_b.to_string(),
        "liquiditySupply": pool.liquidity_pool_supply.to_string(),
        "feeBps": fee_bps,
    }))
}

/// Derives the pool PDA for a swap pair (tokens in either order). The pool is now namespaced by
/// its AMM instance, so the caller supplies the instance's `config` account: its id is the
/// namespace root the pool PDA is derived under (only the config account is decoded — unlike
/// `swap_pair`, no config-dependent tick PDA is derived, so `AmmConfig` need not be valid).
pub(super) fn pool_id(request: PoolIdRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    // The config's address is the namespace root the pool derives under.
    let (config_id, config) = load_config(amm_program, &request.config)?;
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    Ok(json!({
        "poolId": account_id_hex(compute_pool_pda(amm_program, config_id, token_a, token_b)),
        // Which program's shard to ask for at each address a plan path reads. In
        // v0.2.5 a read named only an address, so none of this was needed; now the
        // module must know whose state it wants, and this is the one derivation op
        // every plan path already calls.
        "tokenProgramId": account_id_hex(config.token_program_id),
        "twapOracleProgramId": account_id_hex(config.twap_oracle_program_id),
        // The canonical clock: every plan that chains into the TWAP oracle declares
        // `now` and so has to read it. Both ids are constants the caller cannot
        // derive for itself.
        "clockId": account_id_hex(CLOCK_01_PROGRAM_ACCOUNT_ID),
        "clockProgramId": account_id_hex(clock_account_id()),
    }))
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
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let amount_in = parse_u128(&request.amount_in, "amountIn")?;
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

    // The swap fee is instance-wide (AmmConfig::swap_fee_bps), read from the config.
    let swap_fee_bps = swap_fee_bps_from_config(amm_program, &request.config)?;

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
        swap_exact_in_amounts(amount_in, reserve_in, reserve_out, swap_fee_bps);
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
        "expectedOut": expected_out.to_string(),
        "minReceived": min_received.to_string(),
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
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let amount_out = parse_u128(&request.amount_out, "amountOut")?;
    if amount_out == 0 {
        // The guest's exact_output_swap_logic rejects a zero output before any
        // transfer, so a zero-output preview would claim an unexecutable quote
        // (swap_exact_out_amounts would otherwise return a free (0, 0)).
        return Err(String::from("amount_too_small"));
    }
    if u128::from(request.slippage_bps) >= FEE_BPS_DENOMINATOR {
        return Err(String::from("invalid_slippage"));
    }

    // The swap fee is instance-wide (AmmConfig::swap_fee_bps), read from the config.
    let swap_fee_bps = swap_fee_bps_from_config(amm_program, &request.config)?;

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
        swap_exact_out_amounts(amount_out, reserve_in, reserve_out, swap_fee_bps)
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
        "requiredIn": required_in.to_string(),
        "maxIn": max_in.to_string(),
        "priceImpactBps": price_impact_bps,
    }))
}

/// Builds the `SwapExactInput` submission for a pair: the fixed 9-slot IDL order
/// (vaults from the pool's stored ids, only the user's input holding signs) and
/// the borsh-encoded instruction bytes (the encoding the guest decodes). On
/// success returns exactly the tx-submission fields (`programId`, `accountIds`,
/// `programAccountIds`, `signingRequirements`, `instruction`; the deadline is
/// already baked into the instruction bytes). Every recoverable domain failure
/// fails CLOSED as `Err` (surfaced through the FFI envelope as `{ ok: false,
/// error }`): `same_token_pair`, `config_unavailable`, and a
/// missing/undecodable pool as `Err("no_pool")` — so the caller stops on
/// `planResult.ok` rather than submit a tx with empty account/instruction
/// vectors.
///
/// Four figures the v0.2.5 handler worked out for itself — the output, the
/// protocol fee, the post-swap price, and the direction the trade runs in — are
/// declared here instead, because the guest's plan phase sees no account data.
/// See [`SwapPins`]; `clock` is read for the same reason.
pub(super) fn swap_exact_in_plan(request: SwapExactInPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    // Take the vaults from the pool's STORED ids, in its `def_a`/`def_b` (creation)
    // order — the guest asserts the provided vaults against `pool.vault_a_id` /
    // `vault_b_id`, which needn't match `derive_pair`'s canonical order for a
    // non-canonically created pool. (`pair.pool`/`current_tick`/`clock` are
    // order-independent, so they still come from `derive_pair`.)
    let pool = pool_from_hex(&request.pool_data)?;

    let user_input_holding =
        account_id_from_hex(&request.user_input_holding_id, "user input holding id")?;
    let user_output_holding =
        account_id_from_hex(&request.user_output_holding_id, "user output holding id")?;

    let swap_amount_in = parse_u128(&request.amount_in, "amountIn")?;
    let min_amount_out = parse_u128(&request.min_out, "minOut")?;
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;
    let now = now_from_clock(&request.clock)?;

    let pins = swap_pins(
        &pool,
        &pair.facts,
        token_in,
        swap_amount_in,
        Exactness::Input,
    )?;

    let instruction = borsh::to_vec(&amm_core::Instruction::SwapExactInput {
        swap_amount_in,
        min_amount_out,
        deadline,
        facts: pair.facts,
        identity: identity_of(&pool),
        definition_id_in: token_in,
        protocol_fee: pins.protocol_fee,
        new_price: pins.new_price,
        amount_out: pins.amount_out,
        now,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // The protocol fee is taken in the input token, so its holding is the input token's
    // instance-wide protocol PDA under this config namespace (created lazily on first fee).
    let protocol_fee_holding = compute_protocol_fee_pda(amm_program, pair.config, token_in);

    Ok(plan_response(
        &request.amm_program_id,
        &swap_slots(
            &pair,
            &pool,
            user_input_holding,
            user_output_holding,
            protocol_fee_holding,
        ),
        instruction,
    ))
}

/// The nine slots both swap directions share, in the fixed IDL order.
///
/// Four programs across them: the AMM holds the config and the pool, the Token
/// Program holds every vault and holding (including the protocol-fee PDA), the
/// configured oracle holds the current tick, and the clock is the Clock Program's.
fn swap_slots(
    pair: &super::pair::PairIds,
    pool: &PoolDefinition,
    user_input_holding: AccountId,
    user_output_holding: AccountId,
    protocol_fee_holding: AccountId,
) -> [super::envelope::Slot; 9] {
    let amm = pair.amm_program;
    let token = pair.facts.token_program_id;
    [
        slot(pair.config, amm),
        slot(pair.pool, amm),
        slot(pool.vault_a_id, token),
        slot(pool.vault_b_id, token),
        signer(user_input_holding, token),
        slot(user_output_holding, token),
        slot(pair.current_tick, pair.twap_oracle_program),
        slot(pair.clock, clock_account_id()),
        slot(protocol_fee_holding, token),
    ]
}

/// Builds the `SwapExactOutput` submission for a pair: the same fixed 8-account
/// IDL order and error contract as `swap_exact_in_plan` — every recoverable
/// failure fails closed as `Err` (`same_token_pair`, `config_unavailable`, and
/// `Err("no_pool")` for a missing/undecodable pool). Only the encoded
/// instruction differs, carrying `exact_amount_out` / `max_amount_in` in place
/// of the exact-in bounds.
pub(super) fn swap_exact_out_plan(request: SwapExactOutPlanRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let token_in = account_id_from_hex(&request.token_in_id, "token in id")?;
    let token_out = account_id_from_hex(&request.token_out_id, "token out id")?;
    if token_in == token_out {
        return Err(String::from("same_token_pair"));
    }
    let (token_a, token_b) = canonical_pair(token_in, token_out);
    let Ok(pair) = derive_pair(amm_program, token_a, token_b, &request.config) else {
        return Err(String::from("config_unavailable"));
    };

    // Take the vaults from the pool's STORED ids, in its `def_a`/`def_b` (creation)
    // order — the guest asserts the provided vaults against `pool.vault_a_id` /
    // `vault_b_id`, which needn't match `derive_pair`'s canonical order for a
    // non-canonically created pool. (`pair.pool`/`current_tick`/`clock` are
    // order-independent, so they still come from `derive_pair`.)
    let pool = pool_from_hex(&request.pool_data)?;

    let user_input_holding =
        account_id_from_hex(&request.user_input_holding_id, "user input holding id")?;
    let user_output_holding =
        account_id_from_hex(&request.user_output_holding_id, "user output holding id")?;

    let exact_amount_out = parse_u128(&request.amount_out, "amountOut")?;
    let max_amount_in = parse_u128(&request.max_in, "maxIn")?;
    let deadline = parse_u64(&request.deadline_ms, "deadlineMs")?;
    let now = now_from_clock(&request.clock)?;

    let pins = swap_pins(
        &pool,
        &pair.facts,
        token_in,
        exact_amount_out,
        Exactness::Output,
    )?;

    let instruction = borsh::to_vec(&amm_core::Instruction::SwapExactOutput {
        exact_amount_out,
        max_amount_in,
        deadline,
        facts: pair.facts,
        identity: identity_of(&pool),
        definition_id_in: token_in,
        protocol_fee: pins.protocol_fee,
        new_price: pins.new_price,
        amount_in: pins.amount_in,
        now,
    })
    .map_err(|error| format!("instruction serialization failed: {error}"))?;

    // The protocol fee is taken in the input token, so its holding is the input token's
    // instance-wide protocol PDA under this config namespace (created lazily on first fee).
    let protocol_fee_holding = compute_protocol_fee_pda(amm_program, pair.config, token_in);

    Ok(plan_response(
        &request.amm_program_id,
        &swap_slots(
            &pair,
            &pool,
            user_input_holding,
            user_output_holding,
            protocol_fee_holding,
        ),
        instruction,
    ))
}

/// Computes a binary's RISC Zero ImageID. `elf` is the hex-encoded `.bin` (a
/// `ProgramBinary`, not a raw ELF); the result is 64-char lowercase hex.
///
/// Since LEZ v0.2.5 this is **not** the program's address. A program is addressed
/// by its deployed `ProgramHeader` account, and that account's id is chosen at
/// deploy time -- `CreateHeader` writes the header into whatever undeployed
/// account the deployer names. Use this to check that a binary matches a header's
/// `image_id` field, not to work out where the program lives. The response field
/// is `imageId` rather than the old `programId` so the distinction is not lost at
/// the boundary.
pub(super) fn program_id(request: ProgramIdRequest) -> Result<Value, String> {
    let elf = hex::decode(&request.elf).map_err(|error| format!("invalid elf hex: {error}"))?;
    let binary = ProgramBinary::decode(&elf).map_err(|error| format!("{error:?}"))?;
    let image_id: lee_core::program::ProgramId = binary
        .compute_image_id()
        .map_err(|error| format!("{error:?}"))?
        .into();
    Ok(json!({ "imageId": account_id_hex(AccountId::from_builtin_program(image_id)) }))
}

#[cfg(test)]
mod tests {
    use amm_core::{AmmConfig, PoolDefinition};
    use lee_core::account::{AccountId, ShardData};

    use super::*;
    use crate::{account::account_read, api::clock::clock_read};

    /// The AMM program every fixture here is keyed under. Both the config and the
    /// pool are *its* shards, so a read now names it as well as an address.
    const AMM: AccountId = AccountId::new([0_u8; 32]);
    const TOKEN: AccountId = AccountId::new([0x01_u8; 32]);
    const TWAP: AccountId = AccountId::new([0x02_u8; 32]);

    fn amm_hex() -> String {
        account_id_hex(AMM)
    }

    /// A decodable AMM config read carrying `swap_fee_bps = 30` — the quotes/resolve read the
    /// instance-wide fee from here now that it is no longer a pool field. 30 bps matches the fee
    /// the pricing assertions were computed against.
    fn valid_config() -> AccountRead {
        account_read(
            AccountId::new([0xEE; 32]),
            AMM,
            &ShardData::from(&AmmConfig {
                token_program_id: TOKEN,
                twap_oracle_program_id: TWAP,
                authority: AccountId::new([0x09; 32]),
                swap_fee_bps: 30,
                protocol_fee_bps: 0,
            }),
        )
    }

    fn pool_read(pool: &PoolDefinition) -> AccountRead {
        account_read(AccountId::new([0x11; 32]), AMM, &ShardData::from(pool))
    }

    #[test]
    fn resolve_pool_reports_canonical_token_ids_and_derived_accounts() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let vault_a = AccountId::new([0xC1; 32]);
        let vault_b = AccountId::new([0xC2; 32]);
        let lp = AccountId::new([0xD0; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            vault_a_id: vault_a,
            vault_b_id: vault_b,
            liquidity_pool_id: lp,
            liquidity_pool_supply: 1_000,
            reserve_a: 111,
            reserve_b: 222,
        };

        let value = resolve_pool(ResolvePoolRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            pool: pool_read(&pool),
        })
        .unwrap();

        assert_eq!(value["status"], "ok");
        assert_eq!(value["poolId"], AccountId::new([0x11; 32]).to_string());
        // The Swap UI matches reserveA/reserveB to its own sell/buy direction by
        // comparing the sold token's id against defAHex — so both ids must be
        // present in canonical order.
        assert_eq!(value["defAHex"], account_id_hex(def_a));
        assert_eq!(value["defBHex"], account_id_hex(def_b));
        assert_eq!(value["vaultAId"], vault_a.to_string());
        assert_eq!(value["vaultBId"], vault_b.to_string());
        assert_eq!(value["lpDefinitionId"], lp.to_string());
        assert_eq!(value["reserveA"], "111");
        assert_eq!(value["reserveB"], "222");
        assert_eq!(value["liquiditySupply"], "1000");
        assert_eq!(value["feeBps"], 30);
    }

    #[test]
    fn resolve_pool_absent_is_a_no_pool_error_carrying_the_pool_id() {
        let pool = PoolDefinition {
            liquidity_pool_supply: 0,
            ..Default::default()
        };
        let value = resolve_pool(ResolvePoolRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            pool: pool_read(&pool),
        })
        .unwrap();
        assert_eq!(
            value,
            json!({
                "status": "error",
                "error": "no_pool",
                "poolId": AccountId::new([0x11; 32]).to_string(),
            })
        );
    }

    #[test]
    fn same_token_pair_is_rejected_by_swap_pair_and_exact_in_plan() {
        let program = "00".repeat(32);
        let same = "aa".repeat(32);
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
        assert_eq!(
            pair,
            json!({ "status": "error", "code": "same_token_pair" })
        );

        // The plan op routes the same domain failure through the FFI envelope (Err).
        let plan_error = swap_exact_in_plan(SwapExactInPlanRequest {
            amm_program_id: program,
            token_in_id: same.clone(),
            token_out_id: same,
            config: dummy_config,
            user_input_holding_id: String::new(),
            user_output_holding_id: String::new(),
            amount_in: String::new(),
            min_out: String::new(),
            deadline_ms: String::new(),
            clock: clock_read(1_000),
            pool_data: String::new(),
        })
        .unwrap_err();
        assert_eq!(plan_error, "same_token_pair");
    }

    #[test]
    fn exact_in_plan_routes_config_unavailable_via_err() {
        // Distinct tokens (past the same-token check) with an unavailable config:
        // derive_pair fails, and the plan must surface it as Err("config_unavailable")
        // — not a legacy Ok({ status, code }) shape.
        let plan_error = swap_exact_in_plan(SwapExactInPlanRequest {
            amm_program_id: "00".repeat(32),
            token_in_id: "aa".repeat(32),
            token_out_id: "bb".repeat(32),
            config: AccountRead {
                id: String::new(),
                status: String::from("read_failed"),
                account: None,
            },
            user_input_holding_id: String::new(),
            user_output_holding_id: String::new(),
            amount_in: String::new(),
            min_out: String::new(),
            deadline_ms: String::new(),
            clock: clock_read(1_000),
            pool_data: String::new(),
        })
        .unwrap_err();
        assert_eq!(plan_error, "config_unavailable");
    }

    #[test]
    fn swap_exact_out_plan_routes_same_token_through_the_envelope() {
        let program = "00".repeat(32);
        let same = "aa".repeat(32);
        let dummy_config = AccountRead {
            id: String::new(),
            status: String::from("read_failed"),
            account: None,
        };

        let plan_error = swap_exact_out_plan(SwapExactOutPlanRequest {
            amm_program_id: program,
            token_in_id: same.clone(),
            token_out_id: same,
            config: dummy_config,
            user_input_holding_id: String::new(),
            user_output_holding_id: String::new(),
            amount_out: String::new(),
            max_in: String::new(),
            deadline_ms: String::new(),
            clock: clock_read(1_000),
            pool_data: String::new(),
        })
        .unwrap_err();
        assert_eq!(plan_error, "same_token_pair");
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
            ..Default::default()
        };

        // Sell A → receive B: reserveIn = reserve_a, reserveOut = reserve_b.
        let ab = swap_exact_in_quote(SwapExactInQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in: "10000".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        // expectedOut comes from the shared on-chain formula (single source of truth).
        let (_, expected_out) = swap_exact_in_amounts(10_000, 1_000_000, 2_000_000, 30);
        assert_eq!(ab["expectedOut"], expected_out.to_string());
        assert_eq!(
            ab["minReceived"],
            (expected_out * (FEE_BPS_DENOMINATOR - 50) / FEE_BPS_DENOMINATOR).to_string()
        );
        assert!(ab["priceImpactBps"].is_number());
        // Only the priced results are echoed — no pool metadata.
        assert!(ab.get("reserveIn").is_none());
        assert!(ab.get("feeBps").is_none());
        assert!(ab.get("poolStatus").is_none());

        // Reverse direction orients reserves the other way.
        let ba = swap_exact_in_quote(SwapExactInQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_b),
            token_out_id: account_id_hex(def_a),
            amount_in: "10000".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        let (_, expected_out_ba) = swap_exact_in_amounts(10_000, 2_000_000, 1_000_000, 30);
        assert_eq!(ba["expectedOut"], expected_out_ba.to_string());
    }

    #[test]
    fn swap_exact_in_quote_no_pool_is_an_error() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let req = |pool_data: String| SwapExactInQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in: "10000".into(),
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
            ..Default::default()
        };
        let req = |amount: &str| SwapExactInQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in: amount.into(),
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
        assert!(swap_exact_in_quote(req("10000")).unwrap()["expectedOut"].is_string());
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
            ..Default::default()
        };
        let quote = swap_exact_in_quote(SwapExactInQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_in: "2".into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        })
        .unwrap();
        assert!(quote["expectedOut"].is_string());
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
            ..Default::default()
        };
        let req = |amount_out: &str| SwapExactOutQuoteRequest {
            amm_program_id: amm_hex(),
            config: valid_config(),
            token_in_id: account_id_hex(def_a),
            token_out_id: account_id_hex(def_b),
            amount_out: amount_out.into(),
            slippage_bps: 50,
            pool_data: pool_data_hex(&pool),
        };

        // Sell A to receive exactly 10_000 B.
        let q = swap_exact_out_quote(req("10000")).unwrap();
        let (_, required_in) = swap_exact_out_amounts(10_000, 1_000_000, 2_000_000, 30).unwrap();
        assert_eq!(q["requiredIn"], required_in.to_string());
        // maxIn = required_in * (10000 + 50) / 10000, rounded up.
        assert_eq!(
            q["maxIn"],
            (required_in * 10_050).div_ceil(10_000).to_string()
        );
        assert!(q["priceImpactBps"].is_number());
        // Only the input-side results are echoed — no output/reserves.
        assert!(q.get("expectedOut").is_none());
        assert!(q.get("reserveIn").is_none());

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
            ..Default::default()
        };
        assert_eq!(
            swap_exact_out_quote(SwapExactOutQuoteRequest {
                amm_program_id: amm_hex(),
                config: valid_config(),
                token_in_id: account_id_hex(def_a),
                token_out_id: account_id_hex(def_b),
                amount_out: "10000".into(),
                slippage_bps: 50,
                pool_data: pool_data_hex(&empty_side),
            }),
            Err(String::from("no_pool"))
        );
    }

    /// The four figures a swap plan declares, spelled out rather than re-derived.
    ///
    /// `swap_pins` and the guest's apply half both price through `amm_core`, so
    /// asserting one against the other would be tautological. Fixed numbers for a
    /// known pool catch drift in either of them.
    #[test]
    fn swap_plans_declare_the_priced_amounts_the_pool_will_recompute() {
        let def_a = AccountId::new([0xAA; 32]);
        let def_b = AccountId::new([0xBB; 32]);
        let pool = PoolDefinition {
            definition_token_a_id: def_a,
            definition_token_b_id: def_b,
            vault_a_id: AccountId::new([0xC1; 32]),
            vault_b_id: AccountId::new([0xC2; 32]),
            liquidity_pool_id: AccountId::new([0xD0; 32]),
            liquidity_pool_supply: 1_000_000,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000,
        };
        // 30 bps swap fee, a fifth of it to the protocol.
        let facts = ConfigFacts {
            token_program_id: TOKEN,
            twap_oracle_program_id: TWAP,
            authority: AccountId::new([0x09; 32]),
            swap_fee_bps: 30,
            protocol_fee_bps: 2_000,
        };

        // Selling 10_000 of token A: 30 bps (30) is the pool's fee, a fifth of it
        // (6) the protocol's cut, and the curve pays out against the remaining
        // 9_970 of effective input.
        let pins = swap_pins(&pool, &facts, def_a, 10_000, Exactness::Input).unwrap();
        assert_eq!(pins.amount_in, 10_000);
        assert_eq!(pins.amount_out, 19_743);
        assert_eq!(pins.protocol_fee, 6);
        // The post-swap price is the pool's, from the reserves *net* of the
        // protocol's cut -- those tokens leave the vault for the fee holding.
        assert_eq!(
            pins.new_price,
            spot_price_q64_64(1_000_000 + 10_000 - 6, 2_000_000 - 19_743)
        );

        // Buying the same output the other way round: the mirror bound, so the
        // declared input is what the curve *demands* rather than what it affords.
        let mirror = swap_pins(&pool, &facts, def_a, pins.amount_out, Exactness::Output).unwrap();
        assert_eq!(mirror.amount_out, pins.amount_out);
        assert_eq!(mirror.amount_in, 10_000);
        assert_eq!(mirror.protocol_fee, 6);

        // A direction that is not one of the pool's two tokens is not a swap.
        assert_eq!(
            swap_pins(
                &pool,
                &facts,
                AccountId::new([0xEE; 32]),
                10_000,
                Exactness::Input
            )
            .unwrap_err(),
            "pair_mismatch"
        );
    }

    /// A config account read whose id is `config_id` (the namespace root pool derivation uses).
    /// `pool_id` wants only the id, but it still asks for the AMM's shard -- which is what
    /// confirms there is an instance at that address -- so the shard must be non-empty.
    fn config_read(config_id: AccountId) -> AccountRead {
        account_read(
            config_id,
            AMM,
            &ShardData::from(&AmmConfig {
                token_program_id: TOKEN,
                twap_oracle_program_id: TWAP,
                authority: AccountId::new([0x09; 32]),
                swap_fee_bps: 30,
                protocol_fee_bps: 0,
            }),
        )
    }

    #[test]
    fn pool_id_is_order_independent_and_matches_core() {
        let program = "00".repeat(32);
        let config_id = AccountId::new([0xEE; 32]);
        let a = AccountId::new([0xCC; 32]);
        let b = AccountId::new([0xDD; 32]);

        let ab = pool_id(PoolIdRequest {
            amm_program_id: program.clone(),
            token_in_id: account_id_hex(a),
            token_out_id: account_id_hex(b),
            config: config_read(config_id),
        })
        .unwrap();
        let ba = pool_id(PoolIdRequest {
            amm_program_id: program.clone(),
            token_in_id: account_id_hex(b),
            token_out_id: account_id_hex(a),
            config: config_read(config_id),
        })
        .unwrap();
        // Canonical ordering makes the pool id independent of swap direction.
        assert_eq!(ab, ba);

        // And it matches amm_core's PDA for the canonical pair under this config namespace.
        let amm = parse_program_id(&program).unwrap();
        let (ca, cb) = if is_canonical_pair(a, b) {
            (a, b)
        } else {
            (b, a)
        };
        assert_eq!(
            ab["poolId"],
            account_id_hex(compute_pool_pda(amm, config_id, ca, cb))
        );

        // Same token in/out is rejected.
        assert!(pool_id(PoolIdRequest {
            amm_program_id: program,
            token_in_id: account_id_hex(a),
            token_out_id: account_id_hex(a),
            config: config_read(config_id),
        })
        .is_err());
    }
}
