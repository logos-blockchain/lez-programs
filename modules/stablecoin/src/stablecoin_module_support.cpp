#include "stablecoin_module_support.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <limits>
#include <set>
#include <string>

#include <nlohmann/json.hpp>

namespace stablecoin_module::detail {
namespace {

using json = nlohmann::json;

int hexValue(char value) {
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

bool isHexLength(const std::string& value, std::size_t length) {
    return value.size() == length
        && std::all_of(value.begin(), value.end(), [](char character) {
               return hexValue(character) >= 0;
           });
}

bool isEvenHex(const std::string& value) {
    return value.size() % 2 == 0
        && std::all_of(value.begin(), value.end(), [](char character) {
               return hexValue(character) >= 0;
           });
}

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char character) {
        return static_cast<char>(std::tolower(character));
    });
    return value;
}

std::string trim(const std::string& value) {
    std::size_t begin = 0;
    std::size_t end = value.size();
    while (begin < end && std::isspace(static_cast<unsigned char>(value[begin]))) ++begin;
    while (end > begin && std::isspace(static_cast<unsigned char>(value[end - 1]))) --end;
    return value.substr(begin, end - begin);
}

bool isZeroId(const std::string& value) {
    return value.size() == 64
        && std::all_of(value.begin(), value.end(), [](char character) {
               return character == '0';
           });
}

bool isAllZero(const std::string& value) {
    return !value.empty()
        && std::all_of(value.begin(), value.end(), [](char character) {
               return character == '0';
           });
}

std::string jsonString(const json& object, const char* key) {
    const auto field = object.find(key);
    return field != object.end() && field->is_string()
        ? field->get<std::string>()
        : std::string();
}

}  // namespace

std::string normalizeAccountId(const std::string& value,
                               const Base58Decoder& base58_decoder) {
    std::string normalized = trim(value);
    if (isHexLength(normalized, 64)) {
        normalized = lowercase(std::move(normalized));
        return isZeroId(normalized) ? std::string() : normalized;
    }
    if (normalized.empty()) return {};

    normalized = lowercase(base58_decoder(normalized));
    return isHexLength(normalized, 64) && !isZeroId(normalized)
        ? normalized
        : std::string();
}

bool isValidAccountIdHex(const std::string& value) {
    return isHexLength(value, 64) && !isZeroId(value);
}

nlohmann::json publicAccountRead(const std::string& account_id,
                                 const std::string& raw_response) {
    json read = {{"id", account_id}, {"status", "not_found"}};
    if (raw_response.empty()) return read;

    const json account = json::parse(raw_response, nullptr, false);
    if (!account.is_object()) {
        read["status"] = "backend_error";
        return read;
    }

    std::string owner = lowercase(jsonString(account, "program_owner"));
    std::string balance = lowercase(jsonString(account, "balance"));
    std::string nonce = lowercase(jsonString(account, "nonce"));
    std::string data = lowercase(jsonString(account, "data"));
    if (!isHexLength(owner, 64) || !isHexLength(balance, 32)
        || !isHexLength(nonce, 32) || !isEvenHex(data)) {
        read["status"] = "backend_error";
        return read;
    }

    if (isZeroId(owner) && isAllZero(balance) && isAllZero(nonce) && data.empty()) {
        return read;
    }

    read["status"] = "ok";
    read["account"] = {
        {"program_owner", std::move(owner)},
        {"balance", std::move(balance)},
        {"nonce", std::move(nonce)},
        {"data", std::move(data)},
    };
    return read;
}

std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input) {
    std::vector<std::uint8_t> result;
    if (!input.is_array()) return result;
    result.reserve(input.size() * sizeof(std::uint32_t));
    for (const auto& item : input) {
        if (!item.is_number_unsigned() && !item.is_number_integer()) return {};
        std::uint64_t raw = 0;
        if (item.is_number_unsigned()) {
            raw = item.get<std::uint64_t>();
        } else {
            const std::int64_t signed_raw = item.get<std::int64_t>();
            if (signed_raw < 0) return {};
            raw = static_cast<std::uint64_t>(signed_raw);
        }
        if (raw > std::numeric_limits<std::uint32_t>::max()) return {};
        const auto word = static_cast<std::uint32_t>(raw);
        result.push_back(static_cast<std::uint8_t>(word & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 8) & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 16) & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 24) & 0xff));
    }
    return result;
}

std::string stableFfiError(const std::string& error) {
    static const std::set<std::string> stable = {
        "account_read_failed",
        "backend_error",
        "bad_request",
        "config_missing",
        "invalid_account_id",
        "invalid_clock",
        "invalid_collateral_definition",
        "invalid_market_price_oracle",
        "invalid_numeric_value",
        "invalid_position_data",
        "invalid_program_binary",
        "invalid_program_id",
        "invalid_protocol_parameters_data",
        "invalid_stablecoin_name",
        "oracle_asset_mismatch",
        "position_nonce_mismatch",
        "position_owner_mismatch",
        "position_pda_mismatch",
        "position_vault_mismatch",
        "program_id_mismatch",
        "protocol_parameters_pda_mismatch",
        "stablecoin_program_mismatch",
    };
    return stable.find(error) == stable.end() ? std::string("backend_error") : error;
}

std::string transactionId(const std::string& raw_response) {
    const json reply = json::parse(raw_response, nullptr, false);
    if (!reply.is_object()) return {};
    const auto success = reply.find("success");
    if (success == reply.end() || !success->is_boolean() || !success->get<bool>()) return {};
    std::string transaction_id = lowercase(jsonString(reply, "tx_hash"));
    return isHexLength(transaction_id, 64) ? transaction_id : std::string();
}

}  // namespace stablecoin_module::detail
