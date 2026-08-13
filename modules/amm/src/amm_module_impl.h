#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <logos_json.h>            // LogosMap / LogosList (nlohmann::json aliases)
#include <logos_module_context.h>  // LogosModuleContext base + modules()

// AMM business logic as a universal core Logos module.
//
// Orchestration only: the AMM domain math (PDA derivation, on-chain account
// decoding, quote/plan computation, and instruction encoding) lives in the Rust
// `amm_ffi` crate and is reached through its JSON FFI (amm_ffi.h — one
// `char* op(const char*)` per operation, request and response both JSON). This
// module sequences those ops with chain I/O delegated to the
// `logos_execution_zone` wallet module (reached via modules().logos_execution_zone).
//
// The same surface is consumed by the QML UI (via modules().amm_module) and
// headlessly (logoscore call amm_module ...). The swap / add-liquidity
// orchestration and the backend's network-context derivation are made Qt-free
// (std::string / LogosMap / LogosList / nlohmann::json) as the universal
// authoring model requires.
//
// Public methods ARE the module's API; the Qt plugin glue is generated from
// this header because metadata.json sets "interface": "universal". Keep the
// header Qt-free — std types only.
class AmmModuleImpl : public LogosModuleContext {
public:
    AmmModuleImpl() = default;
    ~AmmModuleImpl() = default;

    /// Derives the pool PDA for the (def_a_hex, def_b_hex) pair and reads/decodes the
    /// pool account. On success: `{ status:"ok", error:"", poolId, defAHex, defBHex,
    /// vaultAId, vaultBId, lpDefinitionId, reserveA, reserveB, liquiditySupply, feeBps }`
    /// — the A/B fields oriented to the caller's requested order (A is def_a_hex's).
    /// Otherwise `{ status:"error", error:<code> }`: `no_program_bin` (AMM_PROGRAM_BIN
    /// unset/unreadable/bad), `amm_not_initialized` (config undecodable), `bad_config`
    /// (bad ids / internal decode failure), `same_token_pair`, or `no_pool` for the
    /// ordinary "no pool / no liquidity yet" state (still carries `poolId`).
    LogosMap resolvePoolAccount(const std::string& def_a_hex, const std::string& def_b_hex);

    /// Decodes the singleton AMM config account. On success: `{ status:"ok", error:"",
    /// configId, ammProgramId, authority, tokenProgramId, twapOracleProgramId }` (ids
    /// base58). `{ status:"error", error:"config_missing" }` when AMM_PROGRAM_BIN is
    /// unset/unreadable; `config_unavailable` when the config isn't on-chain yet; or
    /// `backend_error` when the backend FFI call fails.
    LogosMap configAccount();

    /// Submits an `UpdateConfig` transferring admin authority to `request.newAuthorityId`
    /// (base58 or hex). Only the current admin can sign, so the connected wallet must control it.
    /// On success `{ status:"ok", error:"", transactionId:<hex> }`; on failure:
    /// `{ status:"error", error:<code> }` — `config_missing`, `invalid_account_id`,
    /// `wallet_submission_failed`, `backend_error`, or a plan code (e.g. `config_unavailable`).
    LogosMap transferOwnership(const LogosMap& request);

    /// Prices a `SwapExactInput` for the (token_in_hex, token_out_hex) pair:
    /// reads the pool and returns `{ status:"ok", error:"", expectedOutRaw,
    /// minReceivedRaw, priceImpactBps }`, oriented and computed server-side via
    /// the shared on-chain formula. `amount_in` accepts a JSON integer or a
    /// decimal string (JSON floats rejected); `slippage_bps` is basis points.
    /// On failure: `{ status:"error", error:<code> }` — `no_pool` (no pool /
    /// liquidity), `config_missing` (AMM_PROGRAM_BIN unset/unreadable),
    /// `bad_amount`, `invalid_slippage` (`slippage_bps` out of range), or
    /// `backend_error`. Pool metadata (reserves, fee) comes from `resolvePool`,
    /// so it isn't echoed here.
    LogosMap swapExactInQuote(const std::string& token_in_hex,
                       const std::string& token_out_hex,
                       const nlohmann::json& amount_in,
                       int64_t slippage_bps);

    /// Prices a `SwapExactOutput` for the (token_in_hex, token_out_hex) pair:
    /// reads the pool and returns `{ status:"ok", error:"", requiredInRaw,
    /// maxInRaw, priceImpactBps }`, oriented and computed server-side via the
    /// shared on-chain formula. `amount_out` accepts a JSON integer or a decimal
    /// string (JSON floats rejected); `slippage_bps` is basis points. On failure:
    /// `{ status:"error", error:<code> }` — `no_pool` (no pool / liquidity),
    /// `output_exceeds_liquidity` (amount_out ≥ reserve), `config_missing`
    /// (AMM_PROGRAM_BIN unset/unreadable), `bad_amount`, `invalid_slippage`
    /// (`slippage_bps` out of range), or `backend_error`.
    LogosMap swapExactOutQuote(const std::string& token_in_hex,
                       const std::string& token_out_hex,
                       const nlohmann::json& amount_out,
                       int64_t slippage_bps);

    /// Submits an on-chain SwapExactInput transaction against the pool for
    /// (def_a_hex = token in, def_b_hex = token out). amount_in / min_out are
    /// u128 base-unit amounts; deadline is a u64 unix-ms timestamp. Each accepts
    /// EITHER a small JSON integer (bare `1000` on the CLI) OR a decimal string
    /// (what the UI passes, and what the CLI must use for any big value — large
    /// amounts and the unix-ms deadline — as a quote-wrapped arg like
    /// '"1000000000000000000"'). Declared `nlohmann::json` so the generated
    /// dispatch hands us the raw value; JSON floats are rejected rather than
    /// submit a silently-rounded amount. Returns the tx hash, or an empty string
    /// on failure (no pool, unreadable AMM_PROGRAM_BIN, bad inputs, failed tx).
    std::string swapExactInput(const std::string& def_a_hex,
                               const std::string& def_b_hex,
                               const std::string& user_input_holding_hex,
                               const std::string& user_output_holding_hex,
                               const nlohmann::json& amount_in,
                               const nlohmann::json& min_out,
                               const nlohmann::json& deadline);

    /// Submits an on-chain SwapExactOutput transaction against the pool for
    /// (def_a_hex = token in, def_b_hex = token out). amount_out / max_in are
    /// u128 base-unit amounts; deadline is a u64 unix-ms timestamp. Same argument
    /// conventions as swapExactInput (JSON integer or decimal string; floats
    /// rejected). Returns the tx hash, or an empty string on failure (no pool,
    /// unreadable AMM_PROGRAM_BIN, bad inputs, failed tx).
    std::string swapExactOutput(const std::string& def_a_hex,
                                const std::string& def_b_hex,
                                const std::string& user_input_holding_hex,
                                const std::string& user_output_holding_hex,
                                const nlohmann::json& amount_out,
                                const nlohmann::json& max_in,
                                const nlohmann::json& deadline);

    /// Prices creating a pool for (tokenAId, tokenBId) from the two deposit amounts.
    /// A pure preview — no chain reads, and no fee needed (the fee is not part of the
    /// pool PDA and doesn't affect the opening LP/price). Returns `{ status:"ok",
    /// error:"", amountARaw, amountBRaw, expectedLpRaw, lockedLpRaw, initialPriceRaw }`
    /// computed via the shared `amm_core` opening-LP math, so `expectedLpRaw` is
    /// exactly what the guest mints. `request` carries `{ tokenAId, tokenBId,
    /// amountARaw, amountBRaw }` (ids hex or base58, normalized to hex; amounts a JSON
    /// integer or decimal string). On failure: `{ status:"error", error:<code> }` —
    /// `invalid_token_id`, `same_token_pair`, `bad_amount` (an amount field is present
    /// but not a valid integer — e.g. a float, from `jsonAmountToDecimal`),
    /// `amount_required` (an amount field is omitted), `invalid_raw_amount` (non-digit or
    /// beyond the u128 range), `amount_must_be_positive` (zero), `amount_too_low`
    /// (deposits below the locked minimum), or `backend_error`. `amount_required`,
    /// `invalid_raw_amount`, `amount_must_be_positive`, `same_token_pair`, and
    /// `amount_too_low` come from the FFI; the rest from the module. The caller decides
    /// create-vs-add by pool existence before calling this.
    LogosMap createPoolQuote(const LogosMap& request);

    /// Submits a `NewDefinition` transaction creating the pool for the request's pair.
    /// `request` carries `{ tokenAId, tokenBId, holdingAId, holdingBId, lpHoldingId,
    /// amountARaw, amountBRaw, feeBps, deadlineMs }` (ids hex or base58, normalized to
    /// hex; amounts/deadline a JSON integer or decimal string, deadline a u64 unix-ms).
    /// The caller provides `lpHoldingId` — a fresh (empty) account the guest initializes
    /// and mints the creator's LP tokens into; a new pool has no pre-existing LP holding,
    /// and the module never creates wallet accounts. On success:
    /// `{ status:"ok", error:"", transactionId:<hex tx hash> }`. On failure:
    /// `{ status:"error", error:<code> }` — `config_missing`, `backend_error`,
    /// `invalid_account_id`, `bad_amount` (malformed amount/deadline), `bad_fee_bps_amount`
    /// (`feeBps` not a JSON integer), `wallet_submission_failed`, or a plan code (e.g.
    /// `invalid_fee_tier`, `config_unavailable`). Unlike the swaps, a submit failure carries
    /// a code so the create-pool UI can tell the user why.
    LogosMap createPool(const LogosMap& request);

    /// Prices an `AddLiquidity` into the existing pool for (tokenAId, tokenBId) from the
    /// two max deposit amounts. Reads the pool server-side (like the swap quotes) and runs
    /// the guest's proportional-deposit math. Returns the same shape as `createPoolQuote`
    /// minus the create-only locked LP: `{ status:"ok", error:"", amountARaw, amountBRaw,
    /// expectedLpRaw, priceRaw }` — the actual ratio-matched deposits (display order), the
    /// LP minted, and the pool's spot price. Slippage is applied at submit, not here.
    /// `request` carries `{ tokenAId, tokenBId, maxAmountARaw, maxAmountBRaw }` (ids hex or
    /// base58, normalized to hex; amounts a JSON integer or decimal string). On failure:
    /// `{ status:"error", error:<code> }` — `invalid_token_id`, `config_missing`,
    /// `bad_amount`, `no_pool`, `pair_mismatch`, `amount_too_low`, or `backend_error`.
    LogosMap addLiquidityQuote(const LogosMap& request);

    /// Submits an `AddLiquidity` transaction into the request's pool. `request` carries
    /// `{ tokenAId, tokenBId, holdingAId, holdingBId, lpHoldingId, maxAmountARaw,
    /// maxAmountBRaw, minLpRaw, deadlineMs }` (ids hex or base58, normalized to hex;
    /// amounts/deadline a JSON integer or decimal string). `minLpRaw` is the caller's
    /// slippage floor on the LP minted (the UI derives it from the quote's expectedLpRaw
    /// and its slippage control). `lpHoldingId` is the holding that receives the minted LP.
    /// On success: `{ status:"ok", error:"", transactionId:<hex tx hash> }`. On failure:
    /// `{ status:"error", error:<code> }` — `config_missing`, `backend_error`,
    /// `invalid_account_id`, `bad_amount`, `same_token_pair` (from `amm_pool_id` or the plan),
    /// `wallet_submission_failed`, or a plan code (e.g. `no_pool`, `pair_mismatch`,
    /// `config_unavailable`).
    LogosMap addLiquidity(const LogosMap& request);

    /// Prices a `RemoveLiquidity` from the existing pool for (tokenAId, tokenBId): burning
    /// `lpAmountRaw` returns the proportional share of each reserve. Reads the pool
    /// server-side (like the add quote) and runs the guest's `floor(reserve·lp/supply)` math.
    /// Returns `{ status:"ok", error:"", amountARaw, amountBRaw, minimumAmountARaw,
    /// minimumAmountBRaw, priceRaw }` — the withdrawals (display order), the slippage floors
    /// the submit enforces, and the pool's spot price. `request` carries `{ tokenAId, tokenBId,
    /// lpAmountRaw, slippageBps }` (ids hex or base58, normalized to hex; amount a JSON integer
    /// or decimal string). On failure: `{ status:"error", error:<code> }` — `invalid_token_id`,
    /// `config_missing`, `bad_amount`, `invalid_slippage`, `no_pool`, `pair_mismatch`,
    /// `insufficient_pool_liquidity`, `amount_too_low`, `minimum_amount_zero`, or
    /// `backend_error`.
    LogosMap removeLiquidityQuote(const LogosMap& request);

    /// Submits a `RemoveLiquidity` transaction against the request's pool. `request` carries
    /// `{ tokenAId, tokenBId, holdingAId, holdingBId, lpHoldingId, lpAmountRaw, minAmountARaw,
    /// minAmountBRaw, deadlineMs }` (ids hex or base58, normalized to hex; amounts/deadline a
    /// JSON integer or decimal string). `lpHoldingId` is the existing holding burned; the token
    /// a/b holdings receive the withdrawal (no fresh account, unlike add/create). `minAmount*Raw`
    /// are the caller's slippage floors on the tokens withdrawn. On success:
    /// `{ status:"ok", error:"", transactionId:<hex tx hash> }`. On failure:
    /// `{ status:"error", error:<code> }` — `config_missing`, `backend_error`,
    /// `invalid_account_id`, `bad_amount`, `wallet_submission_failed`, or a plan code (e.g.
    /// `no_pool`, `config_unavailable`).
    LogosMap removeLiquidity(const LogosMap& request);

    /// Submits a `SyncReserves` transaction for the (tokenAId, tokenBId) pool — a
    /// permissionless keeper op that refreshes the pool's stored reserves to the live vault
    /// balances and its TWAP tick. `request` carries just `{ tokenAId, tokenBId }` (ids hex or
    /// base58, normalized to hex): no amounts, deadline, or holdings, and nothing signs. On
    /// success: `{ status:"ok", error:"", transactionId:<hex tx hash> }`. On failure:
    /// `{ status:"error", error:<code> }` — `invalid_token_id`, `config_missing`,
    /// `backend_error`, `wallet_submission_failed`, or a plan code (e.g. `no_pool`,
    /// `config_unavailable`).
    LogosMap syncReserves(const LogosMap& request);

    /// Lists the connected wallet's fungible token holdings for the account
    /// selector: `[{ accountId (hex), accountType:"TokenHolding", definitionId
    /// (base58), definitionIdHex (hex), balanceRaw }]` — one row per holding
    /// account, every token, including zero-balance holdings. Narrowing to a
    /// specific token is the selector's job. `wallet_open` gates the wallet read;
    /// an empty list on a closed wallet, unset AMM_PROGRAM_BIN, or a decode failure.
    /// (Thin stopgap — token-holding listing is wallet/token data; see
    /// token_holdings.rs.)
    LogosList tokenHoldings(bool wallet_open);

    /// Lists the AMM's supported fee tiers as raw basis points, ascending:
    /// `[1, 5, 30, 100]`. Pure and input-free — the list is `amm_core`'s
    /// `SUPPORTED_FEE_TIERS` (the same set the guest enforces), so the UI's fee
    /// selector never hardcodes or drifts from the program. The app formats the
    /// labels and decides selectability.
    LogosList feeTiers();

    /// Reads the token list config at TOKENS_CONFIG (a JSON array of
    /// { symbol, name, definitionId, holding, decimals }) and returns it,
    /// normalizing definitionId/holding to lowercase hex. Empty list if
    /// TOKENS_CONFIG is unset / unreadable / not a JSON array.
    LogosList tokenList();

    /// Resolves an app-provided set of token ids into liquidity selector rows.
    /// `request` carries `{ tokenIds: [<definition id>, …] }` (base58 or hex,
    /// normalized to hex here) — the app owns the set: its configured tokens plus any
    /// custom/pasted ids it remembers (held-but-unlisted tokens are not auto-added by
    /// the app). Reads each definition and (when `wallet_open`) the wallet, then returns
    /// `[{ definitionId (base58), name, totalSupply, holdingId, balance }]`. Every
    /// row has the same fields — a token the wallet doesn't hold gets `holdingId:""`
    /// and `balance:"0"` — held tokens first. A requested id whose definition is
    /// unreadable / non-fungible is omitted (the app treats a missing row as
    /// unresolved). Empty list if AMM_PROGRAM_BIN is unset or the config read fails.
    /// (`tokenIds` is wrapped in a map, not passed as a bare list, because the
    /// universal-module glue only supports map/scalar inputs.)
    LogosList resolveTokens(const LogosMap& request, bool wallet_open);

private:
    // 64-char lowercase-hex AMM program id via the amm_ffi `program_id` op
    // over the AMM_PROGRAM_BIN bytes (empty if unset/unreadable/bad).
    std::string ammProgramId();

    // Reads AMM_PROGRAM_BIN into a byte vector (empty on unset/unreadable/empty).
    std::vector<uint8_t> loadAmmElf();

    // Normalizes an account id given as 64-char hex or base58 to lowercase hex
    // (base58 via the wallet module). Empty string if it is neither.
    std::string normalizeAccountId(const std::string& id);

    // Derives the config account id (amm_config_id) and reads it, returning the
    // account-read shape the amm_ffi ops embed. Null json when the config_id
    // op itself fails (readPublicAccount always yields at least {id,status}).
    nlohmann::json readConfig(const std::string& amm_program_id);

    // Reads a public account through the wallet module and returns the
    // { id, status, account:{ program_owner, balance, nonce, data } } shape the
    // amm_ffi ops expect (see the app-side accountReadJson). `account` is
    // omitted when the read has no data (uninitialized/nonexistent).
    nlohmann::json readPublicAccount(const std::string& account_id);

    // The user's own public account reads, fresh each call (empty when the wallet
    // is closed). Each read is a live sequencer round-trip.
    nlohmann::json walletAccountReads(bool wallet_open);
};
