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

bool hasStrings(const json& value, std::initializer_list<const char*> keys) {
    if (!value.is_object()) return false;
    for (const auto* key : keys) {
        const auto field = value.find(key);
        if (field == value.end() || !field->is_string()) return false;
    }
    return true;
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
    std::string program;
    if (!configured.empty()) {
        program = normalizeAccountId(configured);
        if (program.empty()) {
            error = "invalid_program_id";
            return {};
        }
    }
    json request = json::object();
    if (!program.empty()) request["oracleProgramId"] = program;
    const char* binary_path = std::getenv("TWAP_ORACLE_PROGRAM_BIN");
    if (binary_path != nullptr && *binary_path != '\0') {
        const std::string binary = binaryHex(binary_path);
        if (binary.empty()) {
            error = "invalid_program_binary";
            return {};
        }
        request["elf"] = binary;
    }
    if (request.empty()) {
        error = "config_missing";
        return {};
    }
    const auto result = support::callFfi(oracle_program_info, request);
    if (!result.ok) {
        error = support::stableFfiError(result.error);
        return {};
    }
    // The program id is the ImageID: both spellings must agree, and a configured
    // id must survive resolution unchanged.
    const std::string resolved = stringField(result.value, "programIdHex");
    if (!validIdentity(result.value, "programId") || !validIdentity(result.value, "clockId") ||
        stringField(result.value, "imageIdHex") != resolved ||
        (!program.empty() && resolved != program)) {
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

LogosMap OracleModuleImpl::submitPlan(const json& value, const json& identities) {
    return guarded([&]() -> LogosMap {
        support::Plan plan;
        if (!support::parsePlan(value, plan) || !identities.is_object())
            return failure("backend_error");
        logos::CallError call_error;
        const json accounts = modules().lez_core.list_accounts(&call_error);
        const std::string inventory_error = support::validateWalletInventory(
            accounts, call_error.ok(), plan,
            [this](const std::string& id) { return normalizeAccountId(id); });
        if (!inventory_error.empty()) return failure(inventory_error);
        const std::string response = modules().lez_core.send_generic_public_transaction(
            plan.accountIds, plan.signingRequirements, plan.instruction, plan.programId,
            &call_error);
        const std::string transaction =
            call_error.ok() ? support::transactionId(response) : std::string();
        if (transaction.empty()) return failure("wallet_submission_failed");
        LogosMap result = success(identities);
        result["transactionId"] = transaction;
        return result;
    });
}

LogosMap OracleModuleImpl::planAndSubmit(Planner planner, json request,
                                         std::initializer_list<AccountRequirement> requirements) {
    std::string error;
    const json info = resolveProgram(error);
    if (!info.is_object()) return failure(error);
    const std::string program = info["programIdHex"].get<std::string>();
    request["oracleProgramId"] = program;

    const auto result = support::callFfi(planner, request);
    if (!result.ok) return failure(result.error);
    const auto plan = result.value.find("plan");
    const auto identities = result.value.find("identities");
    if (plan == result.value.end() || !plan->is_object() || identities == result.value.end() ||
        !identities->is_object() || stringField(*plan, "programId") != program)
        return failure("backend_error");
    for (const auto& [key, value] : identities->items()) {
        const bool hex_twin = key.size() > 3 && key.compare(key.size() - 3, 3, "Hex") == 0;
        if (!hex_twin && !validIdentity(*identities, key)) return failure("backend_error");
    }

    // The program rejects these too; checking first gives a stable error code
    // instead of a submitted transaction that fails on-chain.
    for (const auto& requirement : requirements) {
        const std::string id = stringField(*identities, std::string(requirement.identity) + "Hex");
        if (id.empty()) return failure("backend_error");
        const auto state = support::accountState(readPublicAccount(id), program);
        if (state == support::AccountState::ReadFailed) return failure("account_read_failed");
        if (!requirement.initialized && state != support::AccountState::Uninitialized)
            return failure("already_initialized");
        // A foreign-owned account at the PDA is not this feed's state either.
        if (requirement.initialized && state != support::AccountState::ProgramOwned)
            return failure("account_not_initialized");
    }
    return submitPlan(*plan, *identities);
}

LogosMap OracleModuleImpl::createCurrentTickAccount(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "initialPrice"})) return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        return planAndSubmit(oracle_create_current_tick_account_plan,
                             {{"priceSourceId", source}, {"initialPrice", input["initialPrice"]}},
                             {{"currentTickAccountId", false}});
    });
}

LogosMap OracleModuleImpl::createPriceObservations(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "initialTick", "windowDurationMs"}))
            return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        return planAndSubmit(oracle_create_price_observations_plan,
                             {{"priceSourceId", source},
                              {"initialTick", input["initialTick"]},
                              {"windowDurationMs", input["windowDurationMs"]}},
                             {{"priceObservationsId", false}});
    });
}

LogosMap OracleModuleImpl::createOraclePriceAccount(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "baseAssetId", "quoteAssetId", "initialPrice",
                                "windowDurationMs"}))
            return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        const std::string base = normalizeAccountId(input["baseAssetId"].get<std::string>());
        const std::string quote = normalizeAccountId(input["quoteAssetId"].get<std::string>());
        if (source.empty() || base.empty() || quote.empty()) return failure("invalid_account_id");
        return planAndSubmit(oracle_create_oracle_price_account_plan,
                             {{"priceSourceId", source},
                              {"baseAssetId", base},
                              {"quoteAssetId", quote},
                              {"initialPrice", input["initialPrice"]},
                              {"windowDurationMs", input["windowDurationMs"]}},
                             {{"priceAccountId", false}});
    });
}

LogosMap OracleModuleImpl::updateCurrentTick(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "price"})) return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        return planAndSubmit(oracle_update_current_tick_plan,
                             {{"priceSourceId", source}, {"price", input["price"]}},
                             {{"currentTickAccountId", true}});
    });
}

LogosMap OracleModuleImpl::recordTick(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "windowDurationMs"})) return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        return planAndSubmit(
            oracle_record_tick_plan,
            {{"priceSourceId", source}, {"windowDurationMs", input["windowDurationMs"]}},
            {{"priceObservationsId", true}, {"currentTickAccountId", true}});
    });
}

LogosMap OracleModuleImpl::publishPrice(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceSourceId", "windowDurationMs"})) return failure("bad_request");
        const std::string source = normalizeAccountId(input["priceSourceId"].get<std::string>());
        if (source.empty()) return failure("invalid_account_id");
        return planAndSubmit(
            oracle_publish_price_plan,
            {{"priceSourceId", source}, {"windowDurationMs", input["windowDurationMs"]}},
            {{"priceObservationsId", true},
             {"priceAccountId", true},
             {"currentTickAccountId", true}});
    });
}

LogosMap OracleModuleImpl::priceAccount(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceAccountId", "expectedOwnerId"})) return failure("bad_request");
        const std::string account_id =
            normalizeAccountId(input["priceAccountId"].get<std::string>());
        const std::string owner = normalizeAccountId(input["expectedOwnerId"].get<std::string>());
        if (account_id.empty() || owner.empty()) return failure("invalid_account_id");
        // Explicit account and owner ids: no program resolution, and the read
        // is always the provider's current snapshot, never caller-supplied.
        const json read = readPublicAccount(account_id);
        const auto result =
            support::callFfi(oracle_price_account,
                             {{"priceAccountId", account_id},
                              {"expectedOwnerId", owner},
                              {"read", read}});
        if (!result.ok) return failure(result.error);
        return success(result.value);
    });
}

LogosMap OracleModuleImpl::queryPrice(const LogosMap& request) {
    return guarded([&]() -> LogosMap {
        const json input = request;
        if (!hasStrings(input, {"priceAccountId", "expectedOwnerId", "baseAssetId",
                                "quoteAssetId", "expectedSourceId", "maxAgeMs"}))
            return failure("bad_request");
        const std::string account_id =
            normalizeAccountId(input["priceAccountId"].get<std::string>());
        const std::string owner = normalizeAccountId(input["expectedOwnerId"].get<std::string>());
        const std::string base = normalizeAccountId(input["baseAssetId"].get<std::string>());
        const std::string quote = normalizeAccountId(input["quoteAssetId"].get<std::string>());
        const std::string source =
            normalizeAccountId(input["expectedSourceId"].get<std::string>());
        if (account_id.empty() || owner.empty() || base.empty() || quote.empty() ||
            source.empty())
            return failure("invalid_account_id");
        // The canonical clock is resolved and fetched internally on every call;
        // reads never depend on the selected TWAP deployment.
        const auto address = support::callFfi(oracle_clock_address, json::object());
        if (!address.ok || !validIdentity(address.value, "clockId"))
            return failure("backend_error");
        const json read = readPublicAccount(account_id);
        const json clock_read = readPublicAccount(stringField(address.value, "clockIdHex"));
        const auto result = support::callFfi(
            oracle_query_price,
            {{"priceAccountId", account_id},
             {"expectedOwnerId", owner},
             {"baseAssetId", base},
             {"quoteAssetId", quote},
             {"expectedSourceId", source},
             {"maxAgeMs", input["maxAgeMs"]},
             {"read", read},
             {"clockRead", clock_read}});
        if (!result.ok) return failure(result.error);
        return success(result.value);
    });
}
