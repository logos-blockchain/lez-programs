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

    // LEZ v0.3.0: an account is a nonce plus its program shards. v0.2.5's flat
    // `{program_owner, balance, nonce, data}` is gone with the flat account
    // itself -- ownership is not a property of an address any more, a balance is
    // just the native token program's shard, and "the data" is whichever shard
    // you asked for.
    //
    // The two sides spell it differently, deliberately. `lez_core` emits what its
    // C struct holds -- `{nonce, shards: [{program, data}]}`, an array because
    // `FfiAccount` is a pointer plus a length -- while stablecoin_ffi's
    // `WalletAccount` mirrors `lee_core::Account` itself, `{nonce, data: {shards}}`
    // keyed by program. Re-keying the array into that map is this function's job,
    // and the only place the module has to care that the two agree.
    const std::string nonce = lowercase(jsonString(account, "nonce"));
    const auto shards_field = account.find("shards");
    if (!isHexLength(nonce, 32) || shards_field == account.end()
        || !shards_field->is_array()) {
        read["status"] = "backend_error";
        return read;
    }

    json shards = json::object();
    bool any_state = false;
    for (const auto& entry : *shards_field) {
        if (!entry.is_object()) {
            read["status"] = "backend_error";
            return read;
        }
        const std::string program = lowercase(jsonString(entry, "program"));
        const std::string value = lowercase(jsonString(entry, "data"));
        if (!isHexLength(program, 64) || !isEvenHex(value)) {
            read["status"] = "backend_error";
            return read;
        }
        if (!value.empty()) any_state = true;
        shards[program] = value;
    }

    // An address nobody has written to carries an empty shard map, and a shard
    // with no bytes says the same thing. Both are the `not_found` that v0.2.5
    // reported for an `Account::default()` read.
    if (!any_state) return read;

    read["status"] = "ok";
    read["account"] = {
        {"nonce", nonce},
        {"data", {{"shards", std::move(shards)}}},
    };
    return read;
}

std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input) {
    std::vector<std::uint8_t> result;
    if (!input.is_array()) return result;
    result.reserve(input.size());
    for (const auto& item : input) {
        if (!item.is_number_unsigned() && !item.is_number_integer()) return {};
        std::int64_t raw = item.get<std::int64_t>();
        if (raw < 0 || raw > std::numeric_limits<std::uint8_t>::max()) return {};
        result.push_back(static_cast<std::uint8_t>(raw));
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
        "invalid_program_binary",
        "invalid_program_id",
        "invalid_protocol_parameters_data",
        "invalid_stablecoin_name",
        "oracle_asset_mismatch",
        "program_id_mismatch",
        // New in v0.3.0: a read succeeded but the named program holds no shard at
        // that address -- the successor to a `program_owner` mismatch.
        "program_shard_missing",
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
