#include "amm_module_impl.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

// Generated at build time by logos-cpp-generator. Defines `LogosModules` with
// one std-typed accessor per metadata.json dependency — here
// `logos_execution_zone`. Included only in the .cpp so the impl header the
// generator parses stays free of Qt and codegen types.
#include "logos_sdk.h"

extern "C" {
#include "amm_ffi.h"
}

namespace {

using json = nlohmann::json;

// AMM_DEBUG-gated tracing. The module runs in its own logos_host process whose
// stderr the daemon captures, so these lines surface in the daemon log.
bool ammDebug() {
    static const bool on = std::getenv("AMM_DEBUG") != nullptr;
    return on;
}
#define AMM_TRACE(msg)                                                    \
    do {                                                                  \
        if (ammDebug()) std::cerr << "[amm-debug] " << msg << std::endl;  \
    } while (0)

// Absolute path to the deployed AMM program's compiled binary (amm.bin). The
// module can't derive this itself: the wallet module's bundled AMM program may
// differ from whatever is deployed on the target sequencer, and the bytes are
// what determine the program id (and every PDA derived from it).
constexpr char AMM_PROGRAM_BIN_ENV[] = "AMM_PROGRAM_BIN";

// Absolute path to the JSON token-list config consumed by tokenList().
constexpr char TOKENS_CONFIG_ENV[] = "TOKENS_CONFIG";

int hexVal(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// True when `s` is exactly `len` hex digits (case-insensitive).
bool isHexLen(const std::string& s, size_t len) {
    if (s.size() != len) return false;
    for (const char c : s)
        if (hexVal(c) < 0) return false;
    return true;
}

// True when `s` is an even-length run of hex digits (case-insensitive).
bool isHexEven(const std::string& s) {
    if (s.size() % 2 != 0) return false;
    for (const char c : s)
        if (hexVal(c) < 0) return false;
    return true;
}

std::string toHex(const uint8_t* p, size_t n) {
    static const char* const kDigits = "0123456789abcdef";
    std::string s;
    s.reserve(n * 2);
    for (size_t i = 0; i < n; ++i) {
        s.push_back(kDigits[p[i] >> 4]);
        s.push_back(kDigits[p[i] & 0x0f]);
    }
    return s;
}

// Exception-safe field accessor over a json object: returns "" when the key is
// missing or not a string (nlohmann's value()/get() throw on a type mismatch).
std::string jStr(const json& obj, const char* key) {
    const auto it = obj.find(key);
    return (it != obj.end() && it->is_string()) ? it->get<std::string>() : std::string();
}

// Coerce a swap amount arg — arriving as EITHER a JSON number (bare `1000` on
// the CLI) or a decimal string ("1000", a big u128 the UI passes, or a
// quote-wrapped big value on the CLI) — to its canonical decimal-string form.
// Small values fit a JSON integer; big values (amounts above the JSON/int64
// range, and unix-ms deadlines) must be strings. Returns false for JSON floats,
// negatives, or non-numeric strings, leaving `out` unset.
bool jsonAmountToDecimal(const json& j, std::string& out) {
    if (j.is_string()) {
        std::string s = j.get<std::string>();
        // Trim whitespace, then a single pair of wrapping double quotes if
        // present. This is what lets an EXACT u128 above 2^53 be passed from the
        // logoscore CLI: such a value can't be a JSON number (a bare number that
        // large is a lossy double), so it must be a string — but the CLI folds a
        // quoted arg's quotes INTO the value (`"1e18"` arrives as the literal
        // chars "\"1e18\""). Stripping the wrapper recovers the clean digits. A
        // clean string from the UI (no wrapping quotes) is unaffected.
        auto trim = [](std::string& x) {
            const size_t a = x.find_first_not_of(" \t\n\r");
            const size_t b = x.find_last_not_of(" \t\n\r");
            x = (a == std::string::npos) ? std::string() : x.substr(a, b - a + 1);
        };
        trim(s);
        if (s.size() >= 2 && s.front() == '"' && s.back() == '"') {
            s = s.substr(1, s.size() - 2);
            trim(s);
        }
        // A valid base-unit amount is a non-empty run of decimal digits. Reject
        // anything else (empty, signs, decimal points, exponents, letters) here
        // rather than passing it downstream to surface as an opaque backend error.
        if (s.empty() || s.find_first_not_of("0123456789") != std::string::npos)
            return false;
        out = s;
        return true;
    }
    if (j.is_number_unsigned()) {
        out = std::to_string(j.get<uint64_t>());
        return true;
    }
    if (j.is_number_integer()) {
        const int64_t v = j.get<int64_t>();
        if (v < 0) return false;
        out = std::to_string(v);
        return true;
    }
    // Reject JSON floats (and null/object/array). logoscore promotes any bare
    // number beyond ~2^31 to a double, so a large bare value arrives here as a
    // float — already rounded upstream. Rather than silently accept it, require
    // such big numbers as a (quote-wrapped) STRING, which is bit-exact.
    return false;
}

// json string/bool arrays -> std vectors, and a u32-word array -> the
// little-endian bytes the wallet module's byte-string `instruction` expects.
std::vector<std::string> jsonStrVec(const json& arr) {
    std::vector<std::string> out;
    if (!arr.is_array()) return out;
    out.reserve(arr.size());
    for (const auto& v : arr)
        if (v.is_string()) out.push_back(v.get<std::string>());
    return out;
}

std::vector<bool> jsonBoolVec(const json& arr) {
    std::vector<bool> out;
    if (!arr.is_array()) return out;
    out.reserve(arr.size());
    for (const auto& v : arr)
        out.push_back(v.is_boolean() && v.get<bool>());
    return out;
}

std::vector<uint8_t> jsonWordsToLeBytes(const json& arr) {
    std::vector<uint8_t> out;
    if (!arr.is_array()) return out;
    out.reserve(arr.size() * sizeof(uint32_t));
    for (const auto& v : arr) {
        const uint32_t word = v.is_number() ? static_cast<uint32_t>(v.get<uint64_t>()) : 0;
        out.push_back(static_cast<uint8_t>(word & 0xff));
        out.push_back(static_cast<uint8_t>((word >> 8) & 0xff));
        out.push_back(static_cast<uint8_t>((word >> 16) & 0xff));
        out.push_back(static_cast<uint8_t>((word >> 24) & 0xff));
    }
    return out;
}

// Result of an amm_ffi JSON op: the `{ ok, value, error }` envelope decoded.
// `error` carries the op's failure code (e.g. "no_pool") when `ok` is false, so
// callers can surface it in their own response envelope.
struct FfiResult {
    bool ok = false;
    json value;
    std::string error;
};

// Serialize `request`, hand it to an amm_ffi op, and decode its
// `{ ok, value, error }` envelope. Mirrors apps/amm/src/AmmClient.cpp. `value`
// is only populated (and `ok` true) when the op reports success with an object.
FfiResult call(char* (*op)(const char*), const json& request) {
    const std::string payload = request.dump();
    char* raw = op(payload.c_str());
    if (raw == nullptr) {
        AMM_TRACE("amm_ffi op returned null");
        return {};
    }
    const std::string response(raw);
    amm_free(raw);

    const auto doc = json::parse(response, nullptr, /*allow_exceptions=*/false);
    if (!doc.is_object()) {
        AMM_TRACE("amm_ffi op returned invalid JSON");
        return {};
    }
    if (!doc.value("ok", false)) {
        std::string error = doc.value("error", std::string());
        AMM_TRACE("amm_ffi op failure: " << error);
        return {false, json(), std::move(error)};
    }
    const auto it = doc.find("value");
    if (it == doc.end() || !it->is_object()) {
        AMM_TRACE("amm_ffi op value is not an object");
        return {};
    }
    return {true, *it, {}};
}

// new-position response envelope builders.
json issue(const std::string& code, const json& blockingFields = json::array()) {
    return {
        {"code", code},
        {"recoverable", true},
        {"blockingFields", blockingFields},
        {"details", json::object()},
    };
}

json publicError(const std::string& code,
                 const json& blockingFields = json::array(),
                 const json& details = json::object()) {
    json error = issue(code, blockingFields);
    error["details"] = details;
    return {
        {"status", "error"},
        {"canSubmit", false},
        {"code", code},
        {"errors", json::array({error})},
        {"warnings", json::array()},
        {"accountPreview", json::array()},
    };
}

json contextState(const std::string& status,
                  const std::string& network_id,
                  const std::string& network_fingerprint,
                  const std::string& code = {}) {
    json state = {
        {"status", status},
        {"networkId", network_id},
        {"networkFingerprint", network_fingerprint},
        {"tokens", json::array()},
        {"feeTiers", json::array()},
        {"warnings", json::array()},
    };
    if (!code.empty()) state["code"] = code;
    return state;
}

// A json array of the strings at `obj[key]` (empty array when absent/wrong type).
json stringArray(const json& obj, const char* key) {
    const auto it = obj.find(key);
    if (it == obj.end() || !it->is_array()) return json::array();
    json out = json::array();
    for (const auto& v : *it)
        if (v.is_string()) out.push_back(v);
    return out;
}

}  // namespace

std::vector<uint8_t> AmmModuleImpl::loadAmmElf() {
    const char* path = std::getenv(AMM_PROGRAM_BIN_ENV);
    if (path == nullptr || *path == '\0') return {};
    std::ifstream file(path, std::ios::binary);
    if (!file) return {};
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(file)),
                                std::istreambuf_iterator<char>());
}

std::string AmmModuleImpl::ammProgramId() {
    const std::vector<uint8_t> elf = loadAmmElf();
    if (elf.empty()) return {};
    // Hand the deployed binary to the amm_ffi program_id op, which decodes it
    // and computes the Image ID — 64-char lowercase hex, little-endian per u32
    // word (matches `spel program-id` and the on-chain *_program_id fields).
    const FfiResult r = call(amm_program_id, json{{"elf", toHex(elf.data(), elf.size())}});
    if (!r.ok) {
        AMM_TRACE("ammProgramId: amm_program_id op failed");
        return {};
    }
    return jStr(r.value, "programId");
}

AmmModuleImpl::Network AmmModuleImpl::network() {
    // AMM_PROGRAM_BIN / TOKENS_CONFIG are fixed for the process lifetime and this
    // runs on the hot reply path, so resolve the program id + token ids once.
    if (!networkResolved) {
        const std::string id = ammProgramId();
        if (id.empty()) {
            // Not resolvable yet (AMM_PROGRAM_BIN unset/unreadable). Don't cache a
            // transient miss — a later call retries.
            Network net;
            net.status = "config_missing";
            return net;
        }
        programId = id;
        tokenIds.clear();
        for (const auto& token : tokenList()) {
            const std::string token_id = jStr(token, "definitionId");
            if (!token_id.empty()) tokenIds.push_back(token_id);
        }
        networkResolved = true;
    }

    Network net;
    net.amm_program_id = programId;
    // The program id changes per deployment, so it doubles as the network
    // fingerprint (a quote can't be replayed against a different program).
    net.fingerprint = programId;
    net.token_ids = tokenIds;
    net.status = "ready";
    return net;
}

std::string AmmModuleImpl::normalizeAccountId(const std::string& id) {
    size_t start = 0;
    size_t end = id.size();
    while (start < end && std::isspace(static_cast<unsigned char>(id[start]))) ++start;
    while (end > start && std::isspace(static_cast<unsigned char>(id[end - 1]))) --end;
    std::string t = id.substr(start, end - start);

    if (isHexLen(t, 64)) {
        std::transform(t.begin(), t.end(), t.begin(),
                       [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
        return t;
    }

    // Try base58 -> hex via the wallet module ("" on failure).
    std::string hex = modules().logos_execution_zone.account_id_from_base58(t);
    std::transform(hex.begin(), hex.end(), hex.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return hex;
}

nlohmann::json AmmModuleImpl::readPublicAccount(const std::string& account_id) {
    // Matches the app-side accountReadJson: WalletAccountRead defaults status to
    // "read_failed" and only flips to "ok" when every field is well-formed hex;
    // the `account` key is present only for an ok read.
    json result = {{"id", account_id}, {"status", "read_failed"}};

    const std::string account_json =
        modules().logos_execution_zone.get_account_public(account_id);
    const auto obj = json::parse(account_json, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object()) return result;

    const std::string owner = jStr(obj, "program_owner");
    const std::string balance = jStr(obj, "balance");
    const std::string nonce = jStr(obj, "nonce");
    const std::string data = jStr(obj, "data");
    if (!isHexLen(owner, 64) || !isHexLen(balance, 32) || !isHexLen(nonce, 32)
        || !isHexEven(data)) {
        return result;
    }

    result["status"] = "ok";
    result["account"] = {
        {"program_owner", owner},
        {"balance", balance},
        {"nonce", nonce},
        {"data", data},
    };
    return result;
}

nlohmann::json AmmModuleImpl::walletAccountReads(bool wallet_open, bool refresh) {
    if (!wallet_open) {
        walletAccounts = json();  // invalidate — nothing to read while closed
        return json::array();
    }
    // Each readPublicAccount is a live sequencer round-trip, so serve the cached
    // set unless the caller forces a reload (submit / explicit UI refresh).
    if (!refresh && !walletAccounts.is_null())
        return walletAccounts;

    // Normalize the wallet module's [any] return (a vector or a json array)
    // through json so we can iterate/type-check it uniformly.
    json accounts = modules().logos_execution_zone.list_accounts();
    if (!accounts.is_array())
        return json::array();  // transient — don't cache

    json out = json::array();
    for (const auto& entry : accounts) {
        if (!entry.is_object()) continue;
        // The AMM path uses only public accounts (LP holdings / token balances).
        if (!entry.value("is_public", true)) continue;
        const std::string id = jStr(entry, "account_id");
        if (id.empty()) continue;
        out.push_back(readPublicAccount(id));
    }
    walletAccounts = out;
    return out;
}

nlohmann::json AmmModuleImpl::readConfig(const Network& net) {
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", net.amm_program_id}});
    if (!configResult.ok) return json();  // null: config_id op failed
    return readPublicAccount(jStr(configResult.value, "configId"));
}

LogosMap AmmModuleImpl::resolvePool(const std::string& def_a_hex,
                                    const std::string& def_b_hex) {
    // A hard failure carries a stable `error` code (no_program_bin /
    // amm_not_initialized / bad_config) so SwapCard can surface it via poolError.
    // `no_pool` is the ordinary "no pool / no liquidity yet" state, which SwapCard
    // treats as its normal empty state (SwapCard.qml `error !== "no_pool"`). The
    // underlying FFI error string is AMM_TRACE'd to the daemon log.
    auto failed = [](const std::string& error) {
        return LogosMap{{"exists", false}, {"error", error}};
    };

    const Network net = network();
    if (net.status != "ready")
        // config_missing == no program id from AMM_PROGRAM_BIN (unset/unreadable/bad).
        return failed("no_program_bin");

    const json config = readConfig(net);
    if (config.is_null())
        return failed("bad_config");  // amm_config_id op failed (malformed program id)

    // The liquidity view passes base58 ids; the swap card passes hex. Normalize both to hex
    // (idempotent for hex) so the FFI id derivation works either way.
    const std::string token_a = normalizeAccountId(def_a_hex);
    const std::string token_b = normalizeAccountId(def_b_hex);

    const FfiResult pairResult = call(amm_swap_pair, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
        {"config", config},
    });
    if (!pairResult.ok)
        return failed("bad_config");  // FFI op failed (e.g. bad token-id hex)
    if (jStr(pairResult.value, "status") != "ok") {
        // config_unavailable (undecodable config) surfaces as amm_not_initialized;
        // same_token_pair is passed through as-is.
        const std::string code = jStr(pairResult.value, "code");
        if (code == "config_unavailable")
            return failed("amm_not_initialized");
        return failed(code.empty() ? "bad_config" : code);
    }

    const json pool = readPublicAccount(jStr(pairResult.value, "poolId"));
    const FfiResult resolveResult = call(amm_resolve_pool, json{{"pool", pool}});
    if (!resolveResult.ok)
        return failed("bad_config");  // amm_resolve_pool op failed
    // resolve_pool returns { exists:false } (no error) for a missing pool / no
    // liquidity; re-tag it "no_pool" — the code SwapCard expects for that state.
    json resolved = resolveResult.value;
    if (!resolved.value("exists", false))
        return failed("no_pool");
    // resolve_pool labels the reserves in the pool's STORED order (reserveA is defAHex's).
    // Orient them to the CALLER's requested order so reserveA is token_a's reserve — the
    // pool's stored order needn't match (it can be non-canonical, e.g. the testnet setup's
    // pool). Both callers then read reserveA/reserveB as their own token-a/token-b directly.
    if (jStr(resolved, "defAHex") != token_a) {
        resolved["reserveA"].swap(resolved["reserveB"]);
        resolved["defAHex"].swap(resolved["defBHex"]);
    }
    return resolved;  // { exists:true, reserveA, reserveB, feeBps } in the caller's order
}

LogosMap AmmModuleImpl::swapExactInQuote(const std::string& token_in_hex,
                                  const std::string& token_out_hex,
                                  const nlohmann::json& amount_in,
                                  int64_t slippage_bps) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // amountIn arrives as a JSON number (CLI) or decimal string (UI); coerce to a
    // canonical decimal string (rejects floats — see jsonAmountToDecimal).
    std::string amount_in_decimal;
    if (!jsonAmountToDecimal(amount_in, amount_in_decimal))
        return error("bad_amount");

    // slippageBps is a fraction of 100% in basis points. The FFI request field is
    // a u32, so a negative value would fail deserialization with an opaque serde
    // message; gate the full range here for a stable code (100% = 10000 bps, the
    // FFI's FEE_BPS_DENOMINATOR, which also rejects the upper bound as a backstop).
    if (slippage_bps < 0 || slippage_bps >= 10000)
        return error("invalid_slippage");

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // Derive the pool id (config-free) and read the pool account; its raw data is
    // handed to the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_in_hex},
        {"tokenOutId", token_out_hex},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    const FfiResult quoteResult = call(amm_swap_exact_in_quote, json{
        {"tokenInId", token_in_hex},
        {"tokenOutId", token_out_hex},
        {"amountInRaw", amount_in_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap the priced payload { expectedOutRaw, minReceivedRaw,
    // priceImpactBps } in the standard envelope.
    LogosMap out = quoteResult.value;
    out["status"] = "ok";
    out["error"] = "";
    return out;
}

LogosMap AmmModuleImpl::swapExactOutQuote(const std::string& token_in_hex,
                                          const std::string& token_out_hex,
                                          const nlohmann::json& amount_out,
                                          int64_t slippage_bps) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    std::string amount_out_decimal;
    if (!jsonAmountToDecimal(amount_out, amount_out_decimal))
        return error("bad_amount");

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // Derive the pool id (config-free) and read the pool account; its raw data is
    // handed to the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_in_hex},
        {"tokenOutId", token_out_hex},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    const FfiResult quoteResult = call(amm_swap_exact_out_quote, json{
        {"tokenInId", token_in_hex},
        {"tokenOutId", token_out_hex},
        {"amountOutRaw", amount_out_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { requiredInRaw, maxInRaw, priceImpactBps } in the envelope.
    LogosMap out = quoteResult.value;
    out["status"] = "ok";
    out["error"] = "";
    return out;
}

std::string AmmModuleImpl::swapExactInput(const std::string& def_a_hex,
                                          const std::string& def_b_hex,
                                          const std::string& user_input_holding_hex,
                                          const std::string& user_output_holding_hex,
                                          const nlohmann::json& amount_in,
                                          const nlohmann::json& min_out,
                                          const nlohmann::json& deadline) {
    std::string amount_in_decimal;
    std::string min_out_decimal;
    std::string deadline_decimal;
    if (!jsonAmountToDecimal(amount_in, amount_in_decimal)
        || !jsonAmountToDecimal(min_out, min_out_decimal)
        || !jsonAmountToDecimal(deadline, deadline_decimal)) {
        AMM_TRACE("swapExactInput: FAIL amount/deadline not a number or decimal string");
        return {};
    }

    const Network net = network();
    if (net.status != "ready") {
        AMM_TRACE("swapExactInput: FAIL network not ready (" << net.status << ")");
        return {};
    }

    const json config = readConfig(net);
    if (config.is_null()) {
        AMM_TRACE("swapExactInput: FAIL config_id op failed");
        return {};
    }

    // Read the pool so the plan can use its stored vault ids (the guest asserts
    // the vaults in the pool's creation order — see amm_swap_exact_in_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
    });
    if (!poolId.ok) {
        AMM_TRACE("swapExactInput: FAIL amm_pool_id");
        return {};
    }
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // amm_swap_exact_in_plan resolves the pool accounts, encodes SwapExactInput,
    // and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_swap_exact_in_plan, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
        {"config", config},
        {"poolData", pool_data},
        {"userInputHoldingId", user_input_holding_hex},
        {"userOutputHoldingId", user_output_holding_hex},
        {"amountIn", amount_in_decimal},
        {"minOut", min_out_decimal},
        {"deadlineMs", deadline_decimal},
    });
    if (!planResult.ok) {
        AMM_TRACE("swapExactInput: FAIL amm_swap_exact_in_plan: " << planResult.error);
        return {};
    }
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("swapExactInput: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("swapExactInput: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false)) {
        AMM_TRACE("swapExactInput: FAIL tx not successful");
        return {};
    }
    return jStr(obj, "tx_hash");
}

std::string AmmModuleImpl::swapExactOutput(const std::string& def_a_hex,
                                           const std::string& def_b_hex,
                                           const std::string& user_input_holding_hex,
                                           const std::string& user_output_holding_hex,
                                           const nlohmann::json& amount_out,
                                           const nlohmann::json& max_in,
                                           const nlohmann::json& deadline) {
    std::string amount_out_decimal;
    std::string max_in_decimal;
    std::string deadline_decimal;
    if (!jsonAmountToDecimal(amount_out, amount_out_decimal)
        || !jsonAmountToDecimal(max_in, max_in_decimal)
        || !jsonAmountToDecimal(deadline, deadline_decimal)) {
        AMM_TRACE("swapExactOutput: FAIL amount/deadline not a number or decimal string");
        return {};
    }

    const Network net = network();
    if (net.status != "ready") {
        AMM_TRACE("swapExactOutput: FAIL network not ready (" << net.status << ")");
        return {};
    }

    const json config = readConfig(net);
    if (config.is_null()) {
        AMM_TRACE("swapExactOutput: FAIL config_id op failed");
        return {};
    }

    // Read the pool so the plan can use its stored vault ids (the guest asserts
    // the vaults in the pool's creation order — see amm_swap_exact_out_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
    });
    if (!poolId.ok) {
        AMM_TRACE("swapExactOutput: FAIL amm_pool_id");
        return {};
    }
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // amm_swap_exact_out_plan resolves the pool accounts, encodes SwapExactOutput,
    // and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_swap_exact_out_plan, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
        {"config", config},
        {"poolData", pool_data},
        {"userInputHoldingId", user_input_holding_hex},
        {"userOutputHoldingId", user_output_holding_hex},
        {"amountOut", amount_out_decimal},
        {"maxIn", max_in_decimal},
        {"deadlineMs", deadline_decimal},
    });
    if (!planResult.ok) {
        AMM_TRACE("swapExactOutput: FAIL amm_swap_exact_out_plan: " << planResult.error);
        return {};
    }
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("swapExactOutput: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("swapExactOutput: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false)) {
        AMM_TRACE("swapExactOutput: FAIL tx not successful");
        return {};
    }
    return jStr(obj, "tx_hash");
}

LogosMap AmmModuleImpl::liquidityQuote(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // Pure preview — no program id / chain reads / fee. Normalize the pair to hex (the
    // liquidity UI still sources base58 ids from newPositionContext; transitional).
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    // amountARaw/amountBRaw arrive as a JSON number (CLI) or decimal string (UI);
    // coerce to canonical decimal strings (rejects floats — see jsonAmountToDecimal).
    // If an amount field is present but malformed, return bad_amount; otherwise leave it
    // out so the FFI returns amount_required.
    json quoteRequest = {
        {"tokenAId", token_a},
        {"tokenBId", token_b},
    };
    // initialPriceRealRaw is the Q64.64 opening price; used when no amounts are supplied
    // (price-only ⇒ the op returns the minimum opening deposit). Left out if absent.
    std::string price_decimal;
    if (jsonAmountToDecimal(request.value("initialPriceRealRaw", json()), price_decimal))
        quoteRequest["initialPriceRealRaw"] = price_decimal;
    if (request.contains("amountARaw")) {
        std::string amount_a_decimal;
        if (!jsonAmountToDecimal(request.at("amountARaw"), amount_a_decimal))
            return error("bad_amount");
        quoteRequest["amountARaw"] = amount_a_decimal;
    }
    if (request.contains("amountBRaw")) {
        std::string amount_b_decimal;
        if (!jsonAmountToDecimal(request.at("amountBRaw"), amount_b_decimal))
            return error("bad_amount");
        quoteRequest["amountBRaw"] = amount_b_decimal;
    }

    const FfiResult quoteResult = call(amm_liquidity_quote, quoteRequest);
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { actualAmountARaw, actualAmountBRaw, minimumAmountARaw,
    // minimumAmountBRaw, expectedLpRaw, lockedLpRaw, initialPriceRealRaw } in the envelope.
    LogosMap out = quoteResult.value;
    out["status"] = "ok";
    out["error"] = "";
    return out;
}

LogosMap AmmModuleImpl::createPool(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // config_missing == no program id from AMM_PROGRAM_BIN (same as swapExactInQuote).
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // amm_create_pool_plan needs the config account for the twap program id the
    // current-tick PDA derives from; a bad/absent config surfaces from the plan as
    // config_unavailable (no bespoke check here — same as the swap plans).
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return error("backend_error");
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Normalize the pair + user holdings (incl. the caller-provided LP holding) to hex
    // (base58 tolerated — transitional). A new pool has no pre-existing LP holding, so
    // lpHoldingId is a fresh account the caller supplies; an empty/invalid id fails here.
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    const std::string holding_a = normalizeAccountId(jStr(request, "holdingAId"));
    const std::string holding_b = normalizeAccountId(jStr(request, "holdingBId"));
    const std::string user_lp = normalizeAccountId(jStr(request, "lpHoldingId"));
    if (token_a.empty() || token_b.empty() || holding_a.empty() || holding_b.empty()
        || user_lp.empty())
        return error("invalid_account_id");

    std::string amount_a_decimal;
    std::string amount_b_decimal;
    std::string deadline_decimal;
    if (!jsonAmountToDecimal(request.value("amountARaw", json()), amount_a_decimal)
        || !jsonAmountToDecimal(request.value("amountBRaw", json()), amount_b_decimal)
        || !jsonAmountToDecimal(request.value("deadlineMs", json()), deadline_decimal))
        return error("bad_amount");

    // feeBps deserializes into a u32 in the plan request, so a missing / null / float / string
    // value would fail the FFI's serde parse and leak an "invalid request JSON" error instead of
    // a stable code. Require a JSON integer here; fee-tier support is validated in the plan op.
    const json fee_val = request.value("feeBps", json());
    if (!fee_val.is_number_integer())
        return error("bad_fee_bps_amount");

    // amm_create_pool_plan resolves the pool accounts (canonicalizing the pair),
    // encodes NewDefinition (with the fee), and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_create_pool_plan, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"amountARaw", amount_a_decimal},
        {"amountBRaw", amount_b_decimal},
        {"feeBps", fee_val},
        {"deadlineMs", deadline_decimal},
        {"userHoldingAId", holding_a},
        {"userHoldingBId", holding_b},
        {"userHoldingLpId", user_lp},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("createPool: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("createPool: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    // The native tx hash (64-char hex) is returned as-is — hex everywhere.
    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
}

LogosMap AmmModuleImpl::addLiquidityQuote(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // Normalize the pair to hex (the liquidity UI still sources base58 ids; transitional).
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    std::string max_a_decimal;
    std::string max_b_decimal;
    if (!jsonAmountToDecimal(request.value("maxAmountARaw", json()), max_a_decimal)
        || !jsonAmountToDecimal(request.value("maxAmountBRaw", json()), max_b_decimal))
        return error("bad_amount");

    // Derive the pool id (config-free) and read the pool account; its raw data is handed
    // to the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // slippageBps is a fraction of 100% in basis points; the pricing op uses it to derive
    // minimumLpRaw (the LP floor the submit accepts). Require an integer JSON number and reject
    // everything else with a stable invalid_slippage: is_number() would also accept a float
    // (and get<int64_t>() on a number_float THROWS, terminating the module), while a string /
    // bool would otherwise fall through to a silent 0. A missing field defaults to 0 (no
    // slippage). A negative or >= 100% value is likewise invalid_slippage.
    const json slippage_val = request.value("slippageBps", json(0));
    if (!slippage_val.is_number_integer())
        return error("invalid_slippage");
    const int64_t slippage_bps = slippage_val.get<int64_t>();
    if (slippage_bps < 0 || slippage_bps >= 10000)
        return error("invalid_slippage");

    const FfiResult quoteResult = call(amm_add_liquidity_quote, json{
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"maxAmountARaw", max_a_decimal},
        {"maxAmountBRaw", max_b_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { amountARaw, amountBRaw, expectedLpRaw, minimumLpRaw, priceRaw }.
    LogosMap out = quoteResult.value;
    out["status"] = "ok";
    out["error"] = "";
    return out;
}

LogosMap AmmModuleImpl::addLiquidity(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // config_missing == no program id from AMM_PROGRAM_BIN (same as createPool).
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // amm_add_liquidity_plan needs the config account for the twap program id the
    // current-tick PDA derives from; a bad/absent config surfaces from the plan.
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return error("backend_error");
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Normalize the pair + user holdings (incl. the LP holding that receives the minted LP)
    // to hex (base58 tolerated — transitional).
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    const std::string holding_a = normalizeAccountId(jStr(request, "holdingAId"));
    const std::string holding_b = normalizeAccountId(jStr(request, "holdingBId"));
    const std::string user_lp = normalizeAccountId(jStr(request, "lpHoldingId"));
    if (token_a.empty() || token_b.empty() || holding_a.empty() || holding_b.empty()
        || user_lp.empty())
        return error("invalid_account_id");

    std::string max_a_decimal;
    std::string max_b_decimal;
    std::string min_lp_decimal;
    std::string deadline_decimal;
    if (!jsonAmountToDecimal(request.value("maxAmountARaw", json()), max_a_decimal)
        || !jsonAmountToDecimal(request.value("maxAmountBRaw", json()), max_b_decimal)
        || !jsonAmountToDecimal(request.value("minLpRaw", json()), min_lp_decimal)
        || !jsonAmountToDecimal(request.value("deadlineMs", json()), deadline_decimal))
        return error("bad_amount");

    // Read the pool so the plan can use its stored vault / LP-definition ids (the guest
    // asserts the provided vaults/LP against them — see amm_add_liquidity_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // amm_add_liquidity_plan resolves the pool accounts (canonicalizing the pair), encodes
    // AddLiquidity (with the slippage floor), and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_add_liquidity_plan, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"maxAmountARaw", max_a_decimal},
        {"maxAmountBRaw", max_b_decimal},
        {"minLpRaw", min_lp_decimal},
        {"deadlineMs", deadline_decimal},
        {"userHoldingAId", holding_a},
        {"userHoldingBId", holding_b},
        {"userHoldingLpId", user_lp},
        {"poolData", pool_data},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("addLiquidity: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("addLiquidity: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
}

LogosMap AmmModuleImpl::removeLiquidityQuote(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // Normalize the pair to hex (the liquidity UI still sources base58 ids; transitional).
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    std::string lp_amount_decimal;
    if (!jsonAmountToDecimal(request.value("lpAmountRaw", json()), lp_amount_decimal))
        return error("bad_amount");

    // Derive the pool id (config-free) and read the pool account; its raw data is handed to
    // the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // slippageBps derives the minimumAmount*Raw floors the submit enforces. Require an integer
    // JSON number and reject everything else with a stable invalid_slippage (same as
    // addLiquidityQuote): is_number() would also accept a float (and get<int64_t>() on a
    // number_float THROWS, terminating the module), while a string / bool would fall through to a
    // silent 0. A missing field defaults to 0 (no slippage). Negative or >= 100% is likewise
    // invalid_slippage.
    const json slippage_val = request.value("slippageBps", json(0));
    if (!slippage_val.is_number_integer())
        return error("invalid_slippage");
    const int64_t slippage_bps = slippage_val.get<int64_t>();
    if (slippage_bps < 0 || slippage_bps >= 10000)
        return error("invalid_slippage");

    const FfiResult quoteResult = call(amm_remove_liquidity_quote, json{
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"lpAmountRaw", lp_amount_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { amountARaw, amountBRaw, minimumAmountARaw, minimumAmountBRaw, priceRaw }.
    LogosMap out = quoteResult.value;
    out["status"] = "ok";
    out["error"] = "";
    return out;
}

LogosMap AmmModuleImpl::removeLiquidity(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // config_missing == no program id from AMM_PROGRAM_BIN (same as addLiquidity).
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // amm_remove_liquidity_plan needs the config account for the twap program id the
    // current-tick PDA derives from; a bad/absent config surfaces from the plan.
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return error("backend_error");
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Normalize the pair + user holdings. Unlike add/create there is no fresh account: the LP
    // holding already exists (it is burned) and the token a/b holdings receive the withdrawal.
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    const std::string holding_a = normalizeAccountId(jStr(request, "holdingAId"));
    const std::string holding_b = normalizeAccountId(jStr(request, "holdingBId"));
    const std::string user_lp = normalizeAccountId(jStr(request, "lpHoldingId"));
    if (token_a.empty() || token_b.empty() || holding_a.empty() || holding_b.empty()
        || user_lp.empty())
        return error("invalid_account_id");

    std::string lp_amount_decimal;
    std::string min_a_decimal;
    std::string min_b_decimal;
    std::string deadline_decimal;
    if (!jsonAmountToDecimal(request.value("lpAmountRaw", json()), lp_amount_decimal)
        || !jsonAmountToDecimal(request.value("minAmountARaw", json()), min_a_decimal)
        || !jsonAmountToDecimal(request.value("minAmountBRaw", json()), min_b_decimal)
        || !jsonAmountToDecimal(request.value("deadlineMs", json()), deadline_decimal))
        return error("bad_amount");

    // Read the pool so the plan can use its stored vault / LP-definition ids (the guest
    // asserts the provided vaults/LP against them — see amm_remove_liquidity_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    // amm_remove_liquidity_plan resolves the pool accounts, encodes RemoveLiquidity (with the
    // per-side slippage floors + caller deadline), and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_remove_liquidity_plan, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"lpAmountRaw", lp_amount_decimal},
        {"minAmountARaw", min_a_decimal},
        {"minAmountBRaw", min_b_decimal},
        {"deadlineMs", deadline_decimal},
        {"userHoldingAId", holding_a},
        {"userHoldingBId", holding_b},
        {"userHoldingLpId", user_lp},
        {"poolData", pool_data},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("removeLiquidity: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("removeLiquidity: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
}

LogosList AmmModuleImpl::tokenList() {
    LogosList out = LogosList::array();

    const char* path = std::getenv(TOKENS_CONFIG_ENV);
    if (path == nullptr || *path == '\0') return out;

    std::ifstream file(path);
    if (!file) return out;
    const std::string content((std::istreambuf_iterator<char>(file)),
                              std::istreambuf_iterator<char>());

    const auto arr = json::parse(content, nullptr, /*allow_exceptions=*/false);
    if (!arr.is_array()) return out;

    for (const auto& entry : arr) {
        if (!entry.is_object()) continue;

        // definitionId/holding may be base58 or hex — normalize both to
        // lowercase hex so downstream consumers can assume hex.
        const std::string definition_id = normalizeAccountId(jStr(entry, "definitionId"));
        const std::string holding = normalizeAccountId(jStr(entry, "holding"));
        if (definition_id.empty() || holding.empty()) continue;

        // decimals must be a non-negative integer. A present-but-non-integer
        // value (e.g. "decimals": "18") would make value<int>() throw
        // type_error.302; that exception becomes dispatch_failed and the Qt
        // caller gets an EMPTY list — one malformed entry dropping every token.
        // Validate and skip just this entry (a wrong decimals would misrender
        // amounts, so fail closed) instead.
        const auto decimals = entry.find("decimals");
        if (decimals == entry.end() || !decimals->is_number_unsigned()) continue;

        json token;
        token["symbol"] = jStr(entry, "symbol");
        token["name"] = jStr(entry, "name");
        token["definitionId"] = definition_id;
        token["holding"] = holding;
        token["decimals"] = decimals->get<int>();
        out.push_back(token);
    }
    return out;
}

LogosList AmmModuleImpl::tokenHoldings(bool wallet_open) {
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return LogosList::array();

    // The config gives the token_program_id that identifies which wallet accounts
    // are token holdings (decoded by the FFI op).
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return LogosList::array();
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Fresh wallet read each call — the selector wants current holdings/balances.
    const json wallet_accounts = walletAccountReads(wallet_open, /*refresh=*/true);

    const FfiResult result = call(amm_token_holdings, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"walletAccounts", wallet_accounts},
    });
    if (!result.ok)
        return LogosList::array();

    LogosList out = LogosList::array();
    const auto it = result.value.find("holdings");
    if (it != result.value.end() && it->is_array())
        for (const auto& holding : *it) out.push_back(holding);
    return out;
}

LogosMap AmmModuleImpl::newPositionContext(const LogosMap& request,
                                           bool wallet_open,
                                           bool refresh_wallet_accounts) {
    const Network net = network();
    if (net.status != "ready")
        return contextState(net.status, net.id, net.fingerprint);

    const json walletAccounts = walletAccountReads(wallet_open, refresh_wallet_accounts);

    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", net.amm_program_id}});
    if (!configResult.ok)
        return contextState("error", net.id, net.fingerprint, "backend_error");
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    json configured = json::array();
    for (const auto& id : net.token_ids) configured.push_back(id);
    const json recent = stringArray(request, "recentTokenIds");
    const json resolved = stringArray(request, "resolvedTokenIds");

    const FfiResult tokenResult = call(amm_token_ids, json{
        {"ammProgramId", net.amm_program_id},
        {"config", config},
        {"walletAccounts", walletAccounts},
        {"configuredTokenIds", configured},
        {"recentTokenIds", recent},
        {"resolvedTokenIds", resolved},
    });
    const json tokenManifest = tokenResult.value;
    if (!tokenResult.ok || jStr(tokenManifest, "status") != "ok") {
        const std::string code =
            tokenResult.ok ? jStr(tokenManifest, "code") : std::string("backend_error");
        return contextState("error", net.id, net.fingerprint,
                            code.empty() ? "backend_error" : code);
    }

    json definitions = json::array();
    for (const auto& id : tokenManifest.value("tokenIds", json::array()))
        if (id.is_string()) definitions.push_back(readPublicAccount(id.get<std::string>()));

    const FfiResult contextResult = call(amm_context, json{
        {"networkId", net.id},
        {"networkFingerprint", net.fingerprint},
        {"ammProgramId", net.amm_program_id},
        {"walletAvailable", wallet_open},
        {"config", config},
        {"walletAccounts", walletAccounts},
        {"tokenDefinitions", definitions},
        {"configuredTokenIds", configured},
        {"recentTokenIds", recent},
        {"resolvedTokenIds", resolved},
    });
    return contextResult.ok
        ? contextResult.value
        : contextState("error", net.id, net.fingerprint, "backend_error");
}

