#pragma once

#include <cstdint>
#include <functional>
#include <nlohmann/json.hpp>
#include <string>
#include <vector>

namespace oracle_module_support {

using Base58Decoder = std::function<std::string(const std::string&)>;
using IdNormalizer = std::function<std::string(const std::string&)>;

std::string normalizeAccountId(const std::string& value, const Base58Decoder& decoder);
bool isCanonicalAccountIdHex(const std::string& value);

// A failed read never becomes an absent/default account. Successfully decoded
// default snapshots retain every field for Rust's Account::default() comparison.
nlohmann::json publicAccountRead(const std::string& id, const std::string& response,
                                 bool transport_ok = true);

struct FfiResult {
    bool ok = false;
    nlohmann::json value;
    std::string error;
};
FfiResult callFfi(char* (*operation)(const char*), const nlohmann::json& request);
std::string stableFfiError(const std::string& error);
std::string transactionId(const std::string& response);

struct Plan {
    std::string programId;
    std::vector<std::string> accountIds;
    std::vector<bool> signingRequirements;
    std::vector<std::uint8_t> instruction;
};

// Structural validation only: forward every signer flag unchanged. Production
// instruction shapes are chosen by typed Rust planners, not this transport.
bool parsePlan(const nlohmann::json& value, Plan& plan);
std::string validateFeePayer(const Plan& plan, const std::string& payer);
std::string validateWalletInventory(const nlohmann::json& accounts, bool transport_ok,
                                    const Plan& plan, const std::string& payer,
                                    const IdNormalizer& normalize);

}  // namespace oracle_module_support
