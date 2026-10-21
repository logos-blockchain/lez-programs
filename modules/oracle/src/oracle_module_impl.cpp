#include "oracle_module_impl.h"

#include <cstdlib>
#include <fstream>
#include <iterator>
#include <nlohmann/json.hpp>
#include <vector>

#include "oracle_module_support.h"

// Generated glue stays out of the universal implementation header.
#include "logos_sdk.h"
extern "C" {
#include "oracle_ffi.h"
}

namespace {
using json = nlohmann::json;
namespace support = oracle_module_support;

LogosMap failure(const std::string& code) {
    return {{"status", "error"}, {"error", support::stableFfiError(code)}};
}

LogosMap success(const json& value = json::object()) {
    LogosMap result = value.get<LogosMap>();
    result["status"] = "ok";
    result["error"] = "";
    return result;
}

template <typename Operation>
LogosMap guarded(Operation&& operation) {
    try {
        return operation();
    } catch (...) {
        return failure("backend_error");
    }
}

std::string stringField(const json& value, const std::string& key) {
    const auto field = value.find(key);
    return field != value.end() && field->is_string() ? field->get<std::string>() : std::string();
}

std::string binaryHex(const char* path) {
    std::ifstream file(path, std::ios::binary);
    if (!file) return {};
    const std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(file)), {});
    if (file.bad()) return {};
    constexpr char digits[] = "0123456789abcdef";
    std::string result;
    result.reserve(bytes.size() * 2);
    for (const auto byte : bytes) {
        result.push_back(digits[byte >> 4]);
        result.push_back(digits[byte & 15]);
    }
    return result;
}
}  // namespace

std::string OracleModuleImpl::normalizeAccountId(const std::string& id) {
    return support::normalizeAccountId(id, [this](const std::string& base58) {
        return modules().lez_core.account_id_from_base58(base58);
    });
}

LogosMap OracleModuleImpl::setOracleProgramId(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json value = request;
        if (!value.contains("oracleProgramId") || !value["oracleProgramId"].is_string())
            return failure("bad_request");
        const std::string selected = value["oracleProgramId"].get<std::string>();
        const std::string normalized =
            selected.empty() ? std::string() : normalizeAccountId(selected);
        if (!selected.empty() && normalized.empty()) return failure("invalid_program_id");
        const std::lock_guard<std::mutex> lock(selectionMutex_);
        oracleProgramId_ = normalized;
        return success();
    });
}

json OracleModuleImpl::readPublicAccount(const std::string& id) {
    logos::CallError error;
    const std::string response = modules().lez_core.get_account_public(id, &error);
    return support::publicAccountRead(id, response, error.ok());
}

bool OracleModuleImpl::validIdentity(const json& value, const std::string& key) {
    const std::string hex = stringField(value, key + "Hex");
    const std::string base58 = stringField(value, key);
    static const std::string alphabet =
        "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    return support::isCanonicalAccountIdHex(hex) && !base58.empty() && base58.size() <= 44 &&
           base58.find_first_not_of(alphabet) == std::string::npos &&
           normalizeAccountId(base58) == hex;
}

json OracleModuleImpl::resolveProgram(std::string& error) {
    std::string configured;
    {
        const std::lock_guard<std::mutex> lock(selectionMutex_);
        configured = oracleProgramId_;
    }
    if (configured.empty()) {
        const char* environment = std::getenv("TWAP_ORACLE_PROGRAM_ID");
        if (environment != nullptr) configured = environment;
    }
    if (configured.empty()) {
        error = "config_missing";
        return {};
    }
    const std::string program = normalizeAccountId(configured);
    if (program.empty()) {
        error = "invalid_program_id";
        return {};
    }
    json request = {{"oracleProgramId", program}};
    const char* binary_path = std::getenv("TWAP_ORACLE_PROGRAM_BIN");
    const bool binary_configured = binary_path != nullptr && *binary_path != '\0';
    if (binary_configured) {
        const std::string binary = binaryHex(binary_path);
        if (binary.empty()) {
            error = "invalid_program_binary";
            return {};
        }
        request["elf"] = binary;
        request["programHeader"] = readPublicAccount(program);
    }
    const auto result = support::callFfi(oracle_program_info, request);
    if (!result.ok) {
        error = support::stableFfiError(result.error);
        return {};
    }
    const auto image = result.value.find("imageIdHex");
    if (!validIdentity(result.value, "programId") || !validIdentity(result.value, "clockId") ||
        stringField(result.value, "programIdHex") != program || image == result.value.end() ||
        !image->is_string() ||
        (binary_configured ? !support::isCanonicalAccountIdHex(image->get<std::string>())
                           : !image->get<std::string>().empty())) {
        error = "backend_error";
        return {};
    }
    return result.value;
}

LogosMap OracleModuleImpl::programInfo() {
    return guarded([&]() -> LogosMap {
        std::string error;
        const json info = resolveProgram(error);
        return info.is_object() ? success(info) : failure(error);
    });
}

LogosMap OracleModuleImpl::feedAddresses(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!input.contains("priceSourceId") || !input["priceSourceId"].is_string() ||
            !input.contains("windowDurationMs") || !input["windowDurationMs"].is_string())
            return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        std::string error;
        const json info = resolveProgram(error);
        if (!info.is_object()) return failure(error);
        const auto result = support::callFfi(oracle_feed_addresses,
                                             {
                                                 {"oracleProgramId", info["programIdHex"]},
                                                 {"priceSourceId", source},
                                                 {"windowDurationMs", input["windowDurationMs"]},
                                             });
        if (!result.ok) return failure(result.error);
        for (const auto* key :
             {"currentTickAccountId", "priceObservationsId", "priceAccountId", "clockId"}) {
            if (!validIdentity(result.value, key)) return failure("backend_error");
        }
        if (result.value["clockIdHex"] != info["clockIdHex"]) return failure("backend_error");
        return success(result.value);
    });
}

LogosMap OracleModuleImpl::submitPlan(const json& value, const std::string& fee_payer,
                                      const json& identities) {
    return guarded([&]() -> LogosMap {
        support::Plan plan;
        if (!support::parsePlan(value, plan) || !identities.is_object())
            return failure("backend_error");
        const std::string payer = normalizeAccountId(fee_payer);
        const std::string payer_error = support::validateFeePayer(plan, payer);
        if (!payer_error.empty()) return failure(payer_error);
        logos::CallError call_error;
        const json accounts = modules().lez_core.list_accounts(&call_error);
        const std::string inventory_error = support::validateWalletInventory(
            accounts, call_error.ok(), plan, payer,
            [this](const std::string& id) { return normalizeAccountId(id); });
        if (!inventory_error.empty()) return failure(inventory_error);
        const std::string response = modules().lez_core.send_generic_public_transaction(
            plan.accountIds, plan.signingRequirements, plan.instruction, plan.programId, payer,
            &call_error);
        const std::string transaction =
            call_error.ok() ? support::transactionId(response) : std::string();
        if (transaction.empty()) return failure("wallet_submission_failed");
        LogosMap result = success(identities);
        result["transactionId"] = transaction;
        return result;
    });
}
