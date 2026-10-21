#include "oracle_module_support.h"

#include <algorithm>
#include <cctype>
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
        "program_header_unavailable",
        "program_binary_mismatch",
        "invalid_account_id",
        "invalid_numeric_value",
        "invalid_window",
        "account_read_failed",
        "invalid_clock",
        "source_not_signable",
        "invalid_fee_payer",
        "fee_payer_not_signable",
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
    for (const auto& byte : value["instruction"]) {
        if (byte.is_number_unsigned()) {
            if (byte.get<std::uint64_t>() > 255) return false;
        } else if (byte.is_number_integer()) {
            const auto number = byte.get<std::int64_t>();
            if (number < 0 || number > 255) return false;
        } else {
            return false;
        }
        parsed.instruction.push_back(byte.get<std::uint8_t>());
    }
    plan = std::move(parsed);
    return true;
}

std::string validateFeePayer(const Plan& plan, const std::string& payer) {
    if (!isCanonicalAccountIdHex(payer)) return "invalid_fee_payer";
    for (std::size_t i = 0; i < plan.accountIds.size(); ++i) {
        if (plan.accountIds[i] == payer && !plan.signingRequirements[i]) return "invalid_fee_payer";
    }
    return {};
}

std::string validateWalletInventory(const json& accounts, bool transport_ok, const Plan& plan,
                                    const std::string& payer, const IdNormalizer& normalize) {
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
    return signable.count(payer) == 1 ? std::string() : "fee_payer_not_signable";
}

}  // namespace oracle_module_support
