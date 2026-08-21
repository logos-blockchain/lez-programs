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

nlohmann::json AmmModuleImpl::walletAccountReads(bool wallet_open) {
    if (!wallet_open)
        return json::array();  // nothing to read while closed

    // Normalize the wallet module's [any] return (a vector or a json array)
    // through json so we can iterate/type-check it uniformly.
    json accounts = modules().logos_execution_zone.list_accounts();
    if (!accounts.is_array())
        return json::array();

    json out = json::array();
    for (const auto& entry : accounts) {
        if (!entry.is_object()) continue;
        // The AMM path uses only public accounts (LP holdings / token balances).
        if (!entry.value("is_public", true)) continue;
        const std::string id = jStr(entry, "account_id");
        if (id.empty()) continue;
        out.push_back(readPublicAccount(id));
    }
    return out;
}

nlohmann::json AmmModuleImpl::readConfig(const std::string& amm_program_id) {
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok) return json();  // null: config_id op failed
    return readPublicAccount(jStr(configResult.value, "configId"));
}

LogosMap AmmModuleImpl::resolvePoolAccount(const std::string& def_a_hex,
                                           const std::string& def_b_hex) {
    // A hard failure carries a stable `error` code (no_program_bin /
    // amm_not_initialized / bad_config) so callers can surface it. `no_pool` is the
    // ordinary "no pool / no liquidity yet" state — also a `status:"error"` result
    // (SwapCard treats it as its normal empty state via `error !== "no_pool"`, the
    // flow routes it to create-pool). The underlying FFI error is AMM_TRACE'd.
    auto failed = [](const std::string& error) {
        return LogosMap{{"status", "error"}, {"error", error}};
    };

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        // no program id from AMM_PROGRAM_BIN (unset/unreadable/bad).
        return failed("no_program_bin");

    const json config = readConfig(amm_program_id);
    if (config.is_null())
        return failed("bad_config");  // amm_config_id op failed (malformed program id)

    // The liquidity view passes base58 ids; the swap card passes hex. Normalize both to hex
    // (idempotent for hex) so the FFI id derivation works either way.
    const std::string token_a = normalizeAccountId(def_a_hex);
    const std::string token_b = normalizeAccountId(def_b_hex);

    const FfiResult pairResult = call(amm_swap_pair, json{
        {"ammProgramId", amm_program_id},
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
    // resolve_pool returns status:"error"/no_pool for a missing pool (pass through) or
    // status:"ok" with the decoded state. It labels reserves/vaults in the pool's STORED
    // order (reserveA is defAHex's); orient them to the CALLER's requested order so reserveA /
    // vaultAId are token_a's — the stored order needn't match (it can be non-canonical, e.g.
    // the testnet setup's pool). Callers then read A/B as their own token-a/token-b directly.
    json resolved = resolveResult.value;
    if (jStr(resolved, "status") == "ok" && jStr(resolved, "defAHex") != token_a) {
        resolved["reserveA"].swap(resolved["reserveB"]);
        resolved["defAHex"].swap(resolved["defBHex"]);
        resolved["vaultAId"].swap(resolved["vaultBId"]);
    }
    return resolved;
}

LogosMap AmmModuleImpl::configAccount() {
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return LogosMap{{"status", "error"}, {"error", "config_missing"}};

    const json config = readConfig(amm_program_id);
    if (config.is_null())
        return LogosMap{{"status", "error"}, {"error", "backend_error"}};

    const FfiResult result = call(amm_config_account, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
    });
    if (!result.ok)
        return LogosMap{{"status", "error"},
                        {"error", result.error.empty() ? "backend_error" : result.error}};
    return result.value;
}

LogosMap AmmModuleImpl::transferOwnership(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // The plan needs the config account to decode the CURRENT admin (the sole signer).
    const json config = readConfig(amm_program_id);
    if (config.is_null())
        return error("config_missing");

    const std::string new_authority = normalizeAccountId(jStr(request, "newAuthorityId"));
    if (new_authority.empty())
        return error("invalid_account_id");

    const FfiResult planResult = call(amm_transfer_ownership_plan, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"newAuthorityId", new_authority},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("transferOwnership: SUBMIT programId=" << program_id
              << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("transferOwnership: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
}

LogosMap AmmModuleImpl::createPriceObservations(const LogosMap& request) {
    return oracleSetupSubmit(request, /*observations=*/true);
}

LogosMap AmmModuleImpl::createOraclePriceAccount(const LogosMap& request) {
    return oracleSetupSubmit(request, /*observations=*/false);
}

LogosMap AmmModuleImpl::oracleSetupSubmit(const LogosMap& request, bool observations) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // The plan derives the feed PDAs from the config's twap_oracle_program_id + the pool.
    const json config = readConfig(amm_program_id);
    if (config.is_null())
        return error("config_missing");

    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    // windowDurationMs may arrive as a JSON number (UI) or a decimal string (CLI); the FFI wants
    // a u64. A zero / unparsable window is rejected — each window is a distinct feed PDA.
    const json window_json = request.value("windowDurationMs", json());
    uint64_t window = 0;
    if (window_json.is_number_unsigned()) {
        window = window_json.get<uint64_t>();
    } else if (window_json.is_number_integer() && window_json.get<int64_t>() > 0) {
        window = window_json.get<uint64_t>();
    } else if (window_json.is_string()) {
        try {
            window = std::stoull(window_json.get<std::string>());
        } catch (...) {
            window = 0;
        }
    }
    if (window == 0)
        return error("invalid_window");

    auto* const plan_op =
        observations ? amm_create_price_observations_plan : amm_create_oracle_price_account_plan;
    const FfiResult planResult = call(plan_op, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"windowDurationMs", window},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction =
        jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    // Surface a stable error code when the target PDA already exists instead of failing at submit.
    const size_t target_index = observations ? 3 : 2;
    if (accounts.size() > target_index) {
        const json existing = readPublicAccount(accounts[target_index]);
        if (jStr(existing, "status") == "ok")
            return error("already_exists");
    }

    AMM_TRACE("oracleSetup(" << (observations ? "observations" : "priceAccount")
              << "): SUBMIT programId=" << program_id << " accounts=" << accounts.size());
    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("oracleSetup: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
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

    // tokenList() moved app-side, so token ids arrive as configured (base58 or
    // hex); normalize to the hex the FFI expects.
    const std::string token_in = normalizeAccountId(token_in_hex);
    const std::string token_out = normalizeAccountId(token_out_hex);
    if (token_in.empty() || token_out.empty())
        return error("invalid_token_id");

    // Derive the pool id (config-free) and read the pool account; its raw data is
    // handed to the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_in},
        {"tokenOutId", token_out},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    const FfiResult quoteResult = call(amm_swap_exact_in_quote, json{
        {"tokenInId", token_in},
        {"tokenOutId", token_out},
        {"amountIn", amount_in_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap the priced payload { expectedOut, minReceived,
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

    // tokenList() moved app-side, so token ids arrive as configured (base58 or
    // hex); normalize to the hex the FFI expects.
    const std::string token_in = normalizeAccountId(token_in_hex);
    const std::string token_out = normalizeAccountId(token_out_hex);

    // Derive the pool id (config-free) and read the pool account; its raw data is
    // handed to the pricing op. An absent account has no data → `no_pool`.
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_in},
        {"tokenOutId", token_out},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    const FfiResult quoteResult = call(amm_swap_exact_out_quote, json{
        {"tokenInId", token_in},
        {"tokenOutId", token_out},
        {"amountOut", amount_out_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { requiredIn, maxIn, priceImpactBps } in the envelope.
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

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty()) {
        AMM_TRACE("swapExactInput: FAIL no program id (AMM_PROGRAM_BIN unset/unreadable)");
        return {};
    }

    const json config = readConfig(amm_program_id);
    if (config.is_null()) {
        AMM_TRACE("swapExactInput: FAIL config_id op failed");
        return {};
    }

    // tokenList() moved app-side, so token/holding ids arrive as configured
    // (base58 or hex); normalize to the hex the FFI expects.
    const std::string def_a = normalizeAccountId(def_a_hex);
    const std::string def_b = normalizeAccountId(def_b_hex);
    const std::string input_holding = normalizeAccountId(user_input_holding_hex);
    const std::string output_holding = normalizeAccountId(user_output_holding_hex);
    if (def_a.empty() || def_b.empty() || input_holding.empty() || output_holding.empty()) {
        AMM_TRACE("swapExactInput: FAIL invalid token/holding id");
        return {};
    }

    // Read the pool so the plan can use its stored vault ids (the guest asserts
    // the vaults in the pool's creation order — see amm_swap_exact_in_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", def_a},
        {"tokenOutId", def_b},
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
        {"ammProgramId", amm_program_id},
        {"tokenInId", def_a},
        {"tokenOutId", def_b},
        {"config", config},
        {"poolData", pool_data},
        {"userInputHoldingId", input_holding},
        {"userOutputHoldingId", output_holding},
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

    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty()) {
        AMM_TRACE("swapExactOutput: FAIL no program id (AMM_PROGRAM_BIN unset/unreadable)");
        return {};
    }

    const json config = readConfig(amm_program_id);
    if (config.is_null()) {
        AMM_TRACE("swapExactOutput: FAIL config_id op failed");
        return {};
    }

    // tokenList() moved app-side, so token/holding ids arrive as configured
    // (base58 or hex); normalize to the hex the FFI expects.
    const std::string def_a = normalizeAccountId(def_a_hex);
    const std::string def_b = normalizeAccountId(def_b_hex);
    const std::string input_holding = normalizeAccountId(user_input_holding_hex);
    const std::string output_holding = normalizeAccountId(user_output_holding_hex);

    // Read the pool so the plan can use its stored vault ids (the guest asserts
    // the vaults in the pool's creation order — see amm_swap_exact_out_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", def_a},
        {"tokenOutId", def_b},
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
        {"ammProgramId", amm_program_id},
        {"tokenInId", def_a},
        {"tokenOutId", def_b},
        {"config", config},
        {"poolData", pool_data},
        {"userInputHoldingId", input_holding},
        {"userOutputHoldingId", output_holding},
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

LogosMap AmmModuleImpl::createPoolQuote(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // Pure preview — no program id / chain reads / fee. Normalize the pair to hex (the
    // liquidity UI sources base58 ids from resolveTokens).
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    // amountA/amountB arrive as a JSON number (CLI) or decimal string (UI);
    // coerce to canonical decimal strings (rejects floats — see jsonAmountToDecimal).
    // If an amount field is present but malformed, return bad_amount; otherwise leave it
    // out so the FFI returns amount_required.
    json quoteRequest = {
        {"tokenAId", token_a},
        {"tokenBId", token_b},
    };
    // price is the Q64.64 opening price; used when no amounts are supplied
    // (price-only ⇒ the op returns the minimum opening deposit). Left out if absent.
    std::string price_decimal;
    if (jsonAmountToDecimal(request.value("price", json()), price_decimal))
        quoteRequest["price"] = price_decimal;
    if (request.contains("amountA")) {
        std::string amount_a_decimal;
        if (!jsonAmountToDecimal(request.at("amountA"), amount_a_decimal))
            return error("bad_amount");
        quoteRequest["amountA"] = amount_a_decimal;
    }
    if (request.contains("amountB")) {
        std::string amount_b_decimal;
        if (!jsonAmountToDecimal(request.at("amountB"), amount_b_decimal))
            return error("bad_amount");
        quoteRequest["amountB"] = amount_b_decimal;
    }

    const FfiResult quoteResult = call(amm_create_pool_quote, quoteRequest);
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { actualAmountA, actualAmountB, minimumAmountA,
    // minimumAmountB, expectedLp, lockedLp, price } in the envelope.
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
    if (!jsonAmountToDecimal(request.value("amountA", json()), amount_a_decimal)
        || !jsonAmountToDecimal(request.value("amountB", json()), amount_b_decimal)
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
        {"amountA", amount_a_decimal},
        {"amountB", amount_b_decimal},
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
    if (!jsonAmountToDecimal(request.value("maxAmountA", json()), max_a_decimal)
        || !jsonAmountToDecimal(request.value("maxAmountB", json()), max_b_decimal))
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
    // minimumLp (the LP floor the submit accepts). Require an integer JSON number and reject
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
        {"maxAmountA", max_a_decimal},
        {"maxAmountB", max_b_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { amountA, amountB, expectedLp, minimumLp, price, lpDefinitionId }.
    // (lpDefinitionId is the pool's LP token so the UI can offer existing LP holdings.)
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
    if (!jsonAmountToDecimal(request.value("maxAmountA", json()), max_a_decimal)
        || !jsonAmountToDecimal(request.value("maxAmountB", json()), max_b_decimal)
        || !jsonAmountToDecimal(request.value("minLp", json()), min_lp_decimal)
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
        {"maxAmountA", max_a_decimal},
        {"maxAmountB", max_b_decimal},
        {"minLp", min_lp_decimal},
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
    if (!jsonAmountToDecimal(request.value("lpAmount", json()), lp_amount_decimal))
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
        {"lpAmount", lp_amount_decimal},
        {"slippageBps", slippage_bps},
        {"poolData", pool_data},
    });
    if (!quoteResult.ok)
        return error(quoteResult.error.empty() ? "backend_error" : quoteResult.error);

    // Success: wrap { amountA, amountB, minimumAmountA, minimumAmountB, price }.
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
    if (!jsonAmountToDecimal(request.value("lpAmount", json()), lp_amount_decimal)
        || !jsonAmountToDecimal(request.value("minAmountA", json()), min_a_decimal)
        || !jsonAmountToDecimal(request.value("minAmountB", json()), min_b_decimal)
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
        {"lpAmount", lp_amount_decimal},
        {"minAmountA", min_a_decimal},
        {"minAmountB", min_b_decimal},
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

LogosMap AmmModuleImpl::syncReserves(const LogosMap& request) {
    auto error = [](const std::string& err) {
        return LogosMap{{"status", "error"}, {"error", err}};
    };

    // config_missing == no program id from AMM_PROGRAM_BIN (same as the other submits).
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return error("config_missing");

    // amm_sync_reserves_plan needs the config account for the twap program id the current-tick
    // PDA derives from; a bad/absent config surfaces from the plan.
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return error("backend_error");
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Normalize the pair to hex (transitional). Sync has no user inputs beyond the pair — no
    // holdings, amounts, or deadline, and nothing signs.
    const std::string token_a = normalizeAccountId(jStr(request, "tokenAId"));
    const std::string token_b = normalizeAccountId(jStr(request, "tokenBId"));
    if (token_a.empty() || token_b.empty())
        return error("invalid_token_id");

    // Read the pool so the plan can use its stored vault ids (the guest asserts the provided
    // vaults against them — see amm_sync_reserves_plan).
    const FfiResult poolId = call(amm_pool_id, json{
        {"ammProgramId", amm_program_id},
        {"tokenInId", token_a},
        {"tokenOutId", token_b},
    });
    if (!poolId.ok)
        return error(poolId.error.empty() ? "backend_error" : poolId.error);
    const json pool = readPublicAccount(jStr(poolId.value, "poolId"));
    const std::string pool_data = jStr(pool.value("account", json::object()), "data");

    const FfiResult planResult = call(amm_sync_reserves_plan, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenAId", token_a},
        {"tokenBId", token_b},
        {"poolData", pool_data},
    });
    if (!planResult.ok)
        return error(planResult.error.empty() ? "backend_error" : planResult.error);
    const json plan = planResult.value;

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    AMM_TRACE("syncReserves: SUBMIT programId=" << program_id
              << " instrBytes=" << instruction.size() << " accounts=" << accounts.size());

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    AMM_TRACE("syncReserves: tx reply=" << reply);

    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return error("wallet_submission_failed");

    return LogosMap{{"status", "ok"}, {"error", ""}, {"transactionId", jStr(obj, "tx_hash")}};
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
    const json wallet_accounts = walletAccountReads(wallet_open);

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

LogosList AmmModuleImpl::feeTiers() {
    // Pure enumeration of amm_core::SUPPORTED_FEE_TIERS — no program id, config,
    // or wallet read needed. The FFI wraps the list as { feeTiers: [...] }.
    const FfiResult result = call(amm_fee_tiers, json::object());
    if (!result.ok)
        return LogosList::array();

    LogosList out = LogosList::array();
    const auto it = result.value.find("feeTiers");
    if (it != result.value.end() && it->is_array())
        for (const auto& tier : *it) out.push_back(tier);
    return out;
}

LogosList AmmModuleImpl::resolveTokens(const LogosMap& request, bool wallet_open) {
    const std::string amm_program_id = ammProgramId();
    if (amm_program_id.empty())
        return LogosList::array();

    // The config gives the token_program_id the FFI needs to decode definitions/holdings.
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", amm_program_id}});
    if (!configResult.ok)
        return LogosList::array();
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    // Normalize the app-provided ids (base58 or hex) → hex, de-dup, and read each
    // definition account. The FFI is stateless, so it gets the reads pre-fetched.
    const auto token_ids_it = request.find("tokenIds");
    const json token_ids = (token_ids_it != request.end() && token_ids_it->is_array())
                               ? *token_ids_it
                               : json::array();

    std::vector<std::string> ids_vec;
    ids_vec.reserve(token_ids.size());
    for (const auto& raw : token_ids) {
        if (!raw.is_string()) continue;
        const std::string hex = normalizeAccountId(raw.get<std::string>());
        if (!hex.empty()) ids_vec.push_back(hex);
    }
    std::sort(ids_vec.begin(), ids_vec.end());
    ids_vec.erase(std::unique(ids_vec.begin(), ids_vec.end()), ids_vec.end());

    json ids = json::array();
    json definitions = json::array();
    for (const auto& hex : ids_vec) {
        ids.push_back(hex);
        definitions.push_back(readPublicAccount(hex));
    }

    // Fresh wallet read — the selector wants current holdings/balances.
    const json wallet_accounts = walletAccountReads(wallet_open);

    const FfiResult result = call(amm_resolve_tokens, json{
        {"ammProgramId", amm_program_id},
        {"config", config},
        {"tokenIds", ids},
        {"walletAccounts", wallet_accounts},
        {"tokenDefinitions", definitions},
    });
    if (!result.ok)
        return LogosList::array();

    LogosList out = LogosList::array();
    const auto it = result.value.find("tokens");
    if (it != result.value.end() && it->is_array())
        for (const auto& row : *it) out.push_back(row);
    return out;
}

