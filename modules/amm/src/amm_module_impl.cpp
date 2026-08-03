#include "amm_module_impl.h"

#include <algorithm>
#include <cctype>
#include <chrono>
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
#include "amm_client.h"
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

// Milliseconds since the unix epoch (u64). Used for the plan's `nowMs` and the
// client deadline check — the module runs on the host, not in the zkVM, so wall
// clock is available (unlike a guest).
uint64_t nowMs() {
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::system_clock::now().time_since_epoch())
            .count());
}

// Decimal string -> u64. False (leaving `out` unset) on empty, non-digit, or
// overflow.
bool parseU64(const std::string& s, uint64_t& out) {
    if (s.empty()) return false;
    uint64_t value = 0;
    for (const char c : s) {
        if (c < '0' || c > '9') return false;
        const uint64_t d = static_cast<uint64_t>(c - '0');
        if (value > (~static_cast<uint64_t>(0) - d) / 10) return false;
        value = value * 10 + d;
    }
    out = value;
    return true;
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

// Result of an amm_client JSON op: the `{ ok, value }` envelope decoded.
struct FfiResult {
    bool ok = false;
    json value;
};

// Serialize `request`, hand it to an amm_client op, and decode its
// `{ ok, value, error }` envelope. Mirrors apps/amm/src/AmmClient.cpp. `value`
// is only populated (and `ok` true) when the op reports success with an object.
FfiResult call(char* (*op)(const char*), const json& request) {
    const std::string payload = request.dump();
    char* raw = op(payload.c_str());
    if (raw == nullptr) {
        AMM_TRACE("amm_client op returned null");
        return {};
    }
    const std::string response(raw);
    amm_free(raw);

    const auto doc = json::parse(response, nullptr, /*allow_exceptions=*/false);
    if (!doc.is_object()) {
        AMM_TRACE("amm_client op returned invalid JSON");
        return {};
    }
    if (!doc.value("ok", false)) {
        AMM_TRACE("amm_client op failure: " << doc.value("error", std::string()));
        return {};
    }
    const auto it = doc.find("value");
    if (it == doc.end() || !it->is_object()) {
        AMM_TRACE("amm_client op value is not an object");
        return {};
    }
    return {true, *it};
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
    // Hand the deployed binary to the amm_client program_id op, which decodes it
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

    const FfiResult pairResult = call(amm_swap_pair, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
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
    const json resolved = resolveResult.value;
    if (!resolved.value("exists", false))
        return failed("no_pool");
    return resolved;  // { exists:true, reserveA, reserveB, feeBps }
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

    // amm_swap_plan resolves the pool, reorders holdings to the pool's canonical
    // def order, encodes SwapExactInput, and returns a ready-to-submit plan.
    const FfiResult planResult = call(amm_swap_plan, json{
        {"ammProgramId", net.amm_program_id},
        {"tokenInId", def_a_hex},
        {"tokenOutId", def_b_hex},
        {"config", config},
        {"userInputHoldingId", user_input_holding_hex},
        {"userOutputHoldingId", user_output_holding_hex},
        {"amountIn", amount_in_decimal},
        {"minOut", min_out_decimal},
        {"deadlineMs", deadline_decimal},
    });
    if (!planResult.ok || jStr(planResult.value, "status") != "ready") {
        AMM_TRACE("swapExactInput: FAIL amm_swap_plan not ready");
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

nlohmann::json AmmModuleImpl::buildQuoteInput(const LogosMap& request,
                                              const Network& net,
                                              bool wallet_open,
                                              bool fresh_wallet_accounts,
                                              nlohmann::json* error) {
    if (net.status != "ready") {
        *error = publicError(net.status);
        return json();
    }
    const FfiResult configResult =
        call(amm_config_id, json{{"ammProgramId", net.amm_program_id}});
    if (!configResult.ok) {
        *error = publicError("backend_error");
        return json();
    }
    const json config = readPublicAccount(jStr(configResult.value, "configId"));

    const FfiResult pairResult = call(amm_pair_ids, json{
        {"ammProgramId", net.amm_program_id},
        {"config", config},
        {"tokenAId", request.value("tokenAId", json())},
        {"tokenBId", request.value("tokenBId", json())},
    });
    if (!pairResult.ok) {
        *error = publicError("backend_error");
        return json();
    }
    const json pairManifest = pairResult.value;
    if (jStr(pairManifest, "status") != "ok") {
        *error = publicError(jStr(pairManifest, "code"));
        return json();
    }

    const json walletAccounts = walletAccountReads(wallet_open, fresh_wallet_accounts);
    const json snapshot = {
        {"config", config},
        {"tokenA", readPublicAccount(jStr(pairManifest, "tokenAId"))},
        {"tokenB", readPublicAccount(jStr(pairManifest, "tokenBId"))},
        {"pool", readPublicAccount(jStr(pairManifest, "poolId"))},
        {"vaultA", readPublicAccount(jStr(pairManifest, "vaultAId"))},
        {"vaultB", readPublicAccount(jStr(pairManifest, "vaultBId"))},
        {"lpDefinition", readPublicAccount(jStr(pairManifest, "lpDefinitionId"))},
        {"lpLockHolding", readPublicAccount(jStr(pairManifest, "lpLockHoldingId"))},
        {"currentTick", readPublicAccount(jStr(pairManifest, "currentTickId"))},
        {"clock", readPublicAccount(jStr(pairManifest, "clockId"))},
        {"walletAvailable", wallet_open},
        {"walletAccounts", walletAccounts},
    };
    return {
        {"networkId", net.id},
        {"networkFingerprint", net.fingerprint},
        {"ammProgramId", net.amm_program_id},
        {"request", request},
        {"snapshot", snapshot},
    };
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

LogosMap AmmModuleImpl::quoteNewPosition(const LogosMap& request, bool wallet_open) {
    const Network net = network();
    json error;
    const json input = buildQuoteInput(request, net, wallet_open, /*fresh=*/false, &error);
    if (!error.is_null()) return error;

    const FfiResult result = call(amm_quote, input);
    return result.ok ? result.value : publicError("backend_error");
}

LogosMap AmmModuleImpl::submitNewPosition(const LogosMap& request,
                                          const std::string& quote_hash,
                                          bool wallet_open,
                                          const std::string& fresh_lp_id) {
    if (m_requestPending) return publicError("submit_in_progress");
    if (!wallet_open) return publicError("wallet_unavailable");
    m_requestPending = true;
    struct Guard {
        bool* flag;
        ~Guard() { *flag = false; }
    } guard{&m_requestPending};

    const Network net = network();
    json error;
    const json input = buildQuoteInput(request, net, wallet_open, /*fresh=*/true, &error);
    if (!error.is_null()) return error;

    const FfiResult quoteResult = call(amm_quote, input);
    if (!quoteResult.ok) return publicError("backend_error");
    const json quote = quoteResult.value;
    if (jStr(quote, "quoteHash") != quote_hash) {
        json result = publicError("quote_changed");
        result["quote"] = quote;
        return result;
    }
    if (!quote.value("canSubmit", false)) {
        json result = publicError("quote_not_submittable");
        result["quote"] = quote;
        return result;
    }

    // Fresh LP holding: the app owns wallet-keyset mutation. If the quote needs
    // one and the caller hasn't supplied it, ask for it (no submit) so the
    // backend can create it through its own wallet provider and call again.
    json freshLp;  // null
    if (quote.value("requiresFreshLp", false)) {
        if (fresh_lp_id.empty()) {
            return json{
                {"status", "requires_fresh_lp"},
                {"quote", quote},
            };
        }
        freshLp = readPublicAccount(fresh_lp_id);
    }

    json planInput = input;
    planInput["quoteHash"] = quote_hash;
    planInput["nowMs"] = nowMs();
    if (!freshLp.is_null()) planInput["freshLp"] = freshLp;

    const FfiResult planResult = call(amm_plan, planInput);
    if (!planResult.ok) return publicError("backend_error");
    const json plan = planResult.value;
    if (jStr(plan, "status") != "ready") {
        const std::string code = jStr(plan, "code");
        return publicError(code.empty() ? "wallet_submission_failed" : code);
    }

    uint64_t deadline = 0;
    if (!parseU64(jStr(plan, "deadlineMs"), deadline) || nowMs() >= deadline)
        return publicError("transaction_deadline_expired");

    const std::vector<std::string> accounts = jsonStrVec(plan.value("accountIds", json::array()));
    const std::vector<bool> signers = jsonBoolVec(plan.value("signingRequirements", json::array()));
    const std::vector<uint8_t> instruction = jsonWordsToLeBytes(plan.value("instruction", json::array()));
    const std::string program_id = jStr(plan, "programId");

    const std::string reply = modules().logos_execution_zone.send_generic_public_transaction(
        accounts, signers, instruction, program_id);
    const auto obj = json::parse(reply, nullptr, /*allow_exceptions=*/false);
    if (!obj.is_object() || !obj.value("success", false))
        return publicError("wallet_submission_failed");

    // Native tx hash (64-char hex) -> base58 transaction id via the wallet
    // module (avoids linking libbase58 just for this encode).
    const std::string tx_hash = jStr(obj, "tx_hash");
    const std::string transaction_id =
        modules().logos_execution_zone.account_id_to_base58(tx_hash);
    if (transaction_id.empty()) return publicError("wallet_submission_failed");

    return {
        {"status", "submitted"},
        {"transactionId", transaction_id},
        {"deadlineMs", plan.value("deadlineMs", json())},
    };
}
