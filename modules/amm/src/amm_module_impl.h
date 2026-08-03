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
// `amm_client` crate and is reached through its JSON FFI (amm_client.h — one
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

    /// Derives the pool PDAs (config / pool / vaults / current-tick) for the
    /// (def_a_hex, def_b_hex) pair and reads the pool's on-chain reserves.
    /// On success: `{ exists:true, reserveA, reserveB, feeBps }` (reserveA/
    /// reserveB in the pool's canonical def order). Otherwise
    /// `{ exists:false, error:<code> }`: `no_program_bin` (AMM_PROGRAM_BIN
    /// unset/unreadable/bad), `amm_not_initialized` (config undecodable),
    /// `bad_config` (bad ids / internal decode failure), `same_token_pair`, or
    /// `no_pool` for the ordinary "no pool / no liquidity yet" state.
    LogosMap resolvePool(const std::string& def_a_hex, const std::string& def_b_hex);

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

    /// Reads the token list config at TOKENS_CONFIG (a JSON array of
    /// { symbol, name, definitionId, holding, decimals }) and returns it,
    /// normalizing definitionId/holding to lowercase hex. Empty list if
    /// TOKENS_CONFIG is unset / unreadable / not a JSON array.
    LogosList tokenList();

    /// New-position (add-liquidity) view state: reads the AMM config + the
    /// user's wallet accounts and returns the new-position context map the
    /// UI renders (available tokens, fee tiers, warnings). `wallet_open` gates
    /// whether wallet accounts are included; `refresh_wallet_accounts` forces a
    /// fresh read rather than a cached one.
    LogosMap newPositionContext(const LogosMap& request,
                                bool wallet_open,
                                bool refresh_wallet_accounts);

    /// Prices an add-liquidity request against current on-chain state and
    /// returns the new-position quote map (quoteHash, canSubmit,
    /// requiresFreshLp, amounts, warnings). Read-only — no submission.
    LogosMap quoteNewPosition(const LogosMap& request, bool wallet_open);

    /// Submits an add-liquidity transaction. Re-quotes and validates `quote_hash`
    /// against the current quote. If the quote requires a fresh LP holding and
    /// `fresh_lp_id` is empty, returns `{ "status": "requires_fresh_lp", ... }`
    /// WITHOUT submitting — the caller (the app backend, which owns the wallet
    /// keyset) creates the account and calls again with its id. Otherwise builds
    /// the plan (injecting the fresh LP account when given) and submits, then
    /// returns the new-position submitted/error map.
    LogosMap submitNewPosition(const LogosMap& request,
                               const std::string& quote_hash,
                               bool wallet_open,
                               const std::string& fresh_lp_id);

private:
    // Off-chain "network" context, derived from the process env (the same
    // sources the app backend used): AMM deployment id from AMM_PROGRAM_BIN,
    // configured token set from TOKENS_CONFIG. `status` is "ready" once the
    // program id resolves, else "config_missing".
    struct Network {
        std::string id = "lez";
        std::string status;
        std::string fingerprint;      // == amm_program_id (binds a quote to the deploy)
        std::string amm_program_id;   // 64-char lowercase hex
        std::vector<std::string> token_ids;
    };
    // AMM_PROGRAM_BIN / TOKENS_CONFIG are fixed for the process lifetime, and
    // this runs on the hot reply path (every op), so it resolves the program id
    // + token ids ONCE and caches them (networkResolved). Cached only on
    // success, so a startup miss (bin not readable yet) retries.
    Network network();

    // 64-char lowercase-hex AMM program id via the amm_client `program_id` op
    // over the AMM_PROGRAM_BIN bytes (empty if unset/unreadable/bad).
    std::string ammProgramId();

    // Reads AMM_PROGRAM_BIN into a byte vector (empty on unset/unreadable/empty).
    std::vector<uint8_t> loadAmmElf();

    // Normalizes an account id given as 64-char hex or base58 to lowercase hex
    // (base58 via the wallet module). Empty string if it is neither.
    std::string normalizeAccountId(const std::string& id);

    // Derives the config account id (amm_config_id) and reads it, returning the
    // account-read shape the amm_client ops embed. Null json when the config_id
    // op itself fails (readPublicAccount always yields at least {id,status}).
    nlohmann::json readConfig(const Network& net);

    // Reads a public account through the wallet module and returns the
    // { id, status, account:{ program_owner, balance, nonce, data } } shape the
    // amm_client ops expect (see the app-side accountReadJson). `account` is
    // omitted when the read has no data (uninitialized/nonexistent).
    nlohmann::json readPublicAccount(const std::string& account_id);

    // The user's own public account reads (empty when the wallet is closed).
    // Cached across calls (walletAccounts); `refresh` reloads instead of serving
    // the cache — quote reuses it, submit forces fresh — since each read is a
    // live sequencer round-trip.
    nlohmann::json walletAccountReads(bool wallet_open, bool refresh);

    // Builds the { networkId, networkFingerprint, ammProgramId, request,
    // snapshot } input shared by quoteNewPosition / submitNewPosition. On a
    // recoverable precondition failure, sets *error to a new-position error
    // map and returns a null json.
    nlohmann::json buildQuoteInput(const LogosMap& request,
                                   const Network& net,
                                   bool wallet_open,
                                   bool fresh_wallet_accounts,
                                   nlohmann::json* error);

    // Guards against a re-entrant in-flight request (e.g. a double-submit) on the
    // shared module instance. Released per call, so the app's fresh-LP resubmit
    // still proceeds.
    bool m_requestPending = false;

    // Process-lifetime network config, resolved once (see network()). Serialized
    // module dispatch means no locking is needed; there is no invalidation, as
    // runtime env reload is not supported.
    bool networkResolved = false;
    std::string programId;
    std::vector<std::string> tokenIds;

    // Cache of the user's public account reads for the context/quote path (each
    // read is a live sequencer round-trip). Null until first read; `refresh`
    // reloads it, and it's dropped when the wallet closes. See walletAccountReads.
    nlohmann::json walletAccounts;
};
