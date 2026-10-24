#include "oracle_module_support.h"

#include <algorithm>
#include <cctype>
#include <limits>
#include <memory>
#include <set>
#include <utility>

extern "C" {
#include "oracle_ffi.h"
}

namespace oracle_module_support {
namespace {

using json = nlohmann::json;

bool isHex(const std::string& value) {
    return std::all_of(value.begin(), value.end(), [](unsigned char c) {
        return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
    });
}

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return value;
}

bool hasString(const json& value, const char* key) {
    const auto field = value.find(key);
    return field != value.end() && field->is_string();
}

}  // namespace

bool isCanonicalAccountIdHex(const std::string& value) {
    return value.size() == 64 && isHex(value) && value != std::string(64, '0');
}

std::string normalizeAccountId(const std::string& value, const Base58Decoder& decoder) {
    const auto begin = value.find_first_not_of(" \t\r\n");
    if (begin == std::string::npos) return {};
    const auto end = value.find_last_not_of(" \t\r\n");
    const std::string trimmed = value.substr(begin, end - begin + 1);
    std::string normalized = lowercase(trimmed);
    if (normalized.size() != 64 || !isHex(normalized)) {
        normalized = lowercase(decoder(trimmed));
    }
    return isCanonicalAccountIdHex(normalized) ? normalized : std::string();
}

json publicAccountRead(const std::string& id, const std::string& response, bool transport_ok) {
    json read = {{"id", id}, {"status", "account_read_failed"}};
    if (!transport_ok || response.empty()) return read;
    const json parsed = json::parse(response, nullptr, false);
    if (!parsed.is_object()) return read;
    json account = json::object();
    for (const auto* field : {"program_owner", "balance", "nonce", "data"}) {
        if (!hasString(parsed, field)) return read;
        const std::string value = lowercase(parsed[field].get<std::string>());
        if (!isHex(value) || value.size() % 2 != 0) return read;
        account[field] = value;
    }
    if (account["program_owner"].get<std::string>().size() != 64 ||
        account["balance"].get<std::string>().size() != 32 ||
        account["nonce"].get<std::string>().size() != 32)
        return read;
    read["status"] = "ok";
    read["account"] = std::move(account);
    return read;
}

AccountState accountState(const json& read, const std::string& program_id_hex) {
    if (!read.is_object() || !hasString(read, "status") || read["status"] != "ok" ||
        !read.contains("account") || !read["account"].is_object())
        return AccountState::ReadFailed;
    const json& account = read["account"];
    for (const auto* field : {"program_owner", "balance", "nonce", "data"}) {
        if (!hasString(account, field)) return AccountState::ReadFailed;
    }
    const std::string owner = account["program_owner"].get<std::string>();
    const bool is_default = owner == std::string(64, '0') &&
                            account["balance"] == std::string(32, '0') &&
                            account["nonce"] == std::string(32, '0') && account["data"] == "";
    if (is_default) return AccountState::Uninitialized;
    return owner == program_id_hex ? AccountState::ProgramOwned : AccountState::OtherOwned;
}

FfiResult callFfi(char* (*operation)(const char*), const json& request) {
    const std::string payload = request.dump();
    const std::unique_ptr<char, decltype(&oracle_free)> response(operation(payload.c_str()),
                                                                 &oracle_free);
    if (!response) return {};
    const json document = json::parse(response.get(), nullptr, false);
    if (!document.is_object() || !document.contains("ok") || !document["ok"].is_boolean())
        return {};
    if (!document["ok"].get<bool>()) {
        return {
            false,
            {},
            hasString(document, "error") ? document["error"].get<std::string>() : "backend_error"};
    }
    if (!document.contains("value") || !document["value"].is_object()) return {};
    return {true, document["value"], {}};
}

std::string stableFfiError(const std::string& error) {
    static const std::set<std::string> codes = {
        "bad_request",
        "config_missing",
        "invalid_program_id",
        "invalid_program_binary",
        "program_binary_mismatch",
        "invalid_account_id",
        "invalid_numeric_value",
        "invalid_window",
        "invalid_price",
        "account_read_failed",
        "already_initialized",
        "account_not_initialized",
        "invalid_account_data",
        "not_found",
        "oracle_asset_mismatch",
        "oracle_future_timestamp",
        "oracle_owner_mismatch",
        "oracle_price_zero",
        "oracle_source_mismatch",
        "oracle_stale",
        "oracle_timestamp_zero",
        "price_account_id_mismatch",
        "invalid_clock",
        "source_not_signable",
        "wallet_submission_failed",
        "backend_error",
    };
    return codes.count(error) == 1 ? error : "backend_error";
}

std::string transactionId(const std::string& response) {
    const json value = json::parse(response, nullptr, false);
    if (!value.is_object() || !value.contains("success") || !value["success"].is_boolean() ||
        !value["success"].get<bool>() || !hasString(value, "tx_hash"))
        return {};
    const std::string hash = value["tx_hash"].get<std::string>();
    return hash.size() == 64 && isHex(hash) ? hash : std::string();
}

bool parsePlan(const json& value, Plan& plan) {
    if (!value.is_object() || !hasString(value, "programId") || !value.contains("accountIds") ||
        !value["accountIds"].is_array() || !value.contains("signingRequirements") ||
        !value["signingRequirements"].is_array() || !value.contains("instruction") ||
        !value["instruction"].is_array())
        return false;
    Plan parsed;
    parsed.programId = value["programId"].get<std::string>();
    const auto& accounts = value["accountIds"];
    const auto& signers = value["signingRequirements"];
    if (!isCanonicalAccountIdHex(parsed.programId) ||
        (accounts.size() != 3 && accounts.size() != 4) || accounts.size() != signers.size() ||
        value["instruction"].empty())
        return false;
    for (const auto& account : accounts) {
        if (!account.is_string() || !isCanonicalAccountIdHex(account.get<std::string>()))
            return false;
        parsed.accountIds.push_back(account.get<std::string>());
    }
    for (const auto& signer : signers) {
        if (!signer.is_boolean()) return false;
        parsed.signingRequirements.push_back(signer.get<bool>());
    }
    for (const auto& word : value["instruction"]) {
        std::uint64_t raw = 0;
        if (word.is_number_unsigned()) {
            raw = word.get<std::uint64_t>();
        } else if (word.is_number_integer()) {
            const auto number = word.get<std::int64_t>();
            if (number < 0) return false;
            raw = static_cast<std::uint64_t>(number);
        } else {
            return false;
        }
        if (raw > std::numeric_limits<std::uint32_t>::max()) return false;
        for (int shift = 0; shift < 32; shift += 8)
            parsed.instruction.push_back(static_cast<std::uint8_t>(raw >> shift));
    }
    plan = std::move(parsed);
    return true;
}

std::string validateWalletInventory(const json& accounts, bool transport_ok, const Plan& plan,
                                    const IdNormalizer& normalize) {
    if (!transport_ok || !accounts.is_array()) return "backend_error";
    std::set<std::string> signable;
    for (const auto& account : accounts) {
        if (!account.is_object() || !hasString(account, "account_id") ||
            !account.contains("is_public") || !account["is_public"].is_boolean())
            return "backend_error";
        if (account["is_public"].get<bool>()) {
            const std::string id = normalize(account["account_id"].get<std::string>());
            if (id.empty()) return "backend_error";
            signable.insert(id);
        }
    }
    for (std::size_t i = 0; i < plan.accountIds.size(); ++i) {
        if (plan.signingRequirements[i] && signable.count(plan.accountIds[i]) == 0)
            return "source_not_signable";
    }
    return {};
}

}  // namespace oracle_module_support
