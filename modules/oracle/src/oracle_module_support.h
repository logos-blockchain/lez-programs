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

enum class AccountState { ReadFailed, Uninitialized, ProgramOwned, OtherOwned };
// Classifies a publicAccountRead result. Uninitialized means Account::default()
// exactly, which is what the program requires of every account it creates.
AccountState accountState(const nlohmann::json& read, const std::string& program_id_hex);

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
    // risc0 serde words from Rust, flattened little-endian for lez_core.
    std::vector<std::uint8_t> instruction;
};

// Structural validation only: forward every signer flag unchanged. Production
// instruction shapes are chosen by typed Rust planners, not this transport.
bool parsePlan(const nlohmann::json& value, Plan& plan);
// Every signer the plan requires must be a public account the wallet holds.
std::string validateWalletInventory(const nlohmann::json& accounts, bool transport_ok,
                                    const Plan& plan, const IdNormalizer& normalize);

}  // namespace oracle_module_support
