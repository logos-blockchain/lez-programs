#include "stablecoin_module_support.h"

#include <cstdint>
#include <limits>
#include <string>
#include <vector>

#include <logos_test.h>
#include <nlohmann/json.hpp>

namespace {

std::string repeated(char value, std::size_t count) {
    return std::string(count, value);
}

/// An account as `lez_core` hands it over: a nonce plus an array of program
/// shards (an array because `FfiAccount` is a shard pointer plus a length).
nlohmann::json validAccount() {
    return {
        {"nonce", repeated('C', 32)},
        {"shards", nlohmann::json::array({
                       {{"program", repeated('A', 64)}, {"data", "00ff"}},
                   })},
    };
}

}  // namespace

LOGOS_TEST(account_id_normalization_accepts_hex_and_delegates_base58) {
    const auto decoder = [](const std::string& value) {
        return value == "base58-id" ? repeated('D', 64) : std::string();
    };

    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::normalizeAccountId(
            "  AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA  ",
            decoder),
        repeated('a', 64));
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::normalizeAccountId("base58-id", decoder),
        repeated('d', 64));
    LOGOS_ASSERT_TRUE(
        stablecoin_module::detail::normalizeAccountId("invalid", decoder).empty());
    LOGOS_ASSERT_TRUE(
        stablecoin_module::detail::normalizeAccountId(repeated('0', 64), decoder).empty());
}

LOGOS_TEST(account_id_hex_validation_rejects_zero_and_malformed_values) {
    LOGOS_ASSERT_TRUE(
        stablecoin_module::detail::isValidAccountIdHex(repeated('A', 64)));
    LOGOS_ASSERT_TRUE(
        !stablecoin_module::detail::isValidAccountIdHex(repeated('0', 64)));
    LOGOS_ASSERT_TRUE(
        !stablecoin_module::detail::isValidAccountIdHex(repeated('g', 64)));
    LOGOS_ASSERT_TRUE(
        !stablecoin_module::detail::isValidAccountIdHex(repeated('1', 63)));
}

LOGOS_TEST(public_account_response_validation_distinguishes_state_and_corruption) {
    const std::string account_id = repeated('1', 64);
    const auto valid = stablecoin_module::detail::publicAccountRead(
        account_id,
        validAccount().dump());
    LOGOS_ASSERT_EQ(valid["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(valid["account"]["nonce"].get<std::string>(), repeated('c', 32));
    // The shard map is keyed by the program that wrote it, lowercased like every
    // other id the module hands to the Rust decoders.
    LOGOS_ASSERT_EQ(
        valid["account"]["data"]["shards"][repeated('a', 64)].get<std::string>(),
        std::string("00ff"));

    // v0.2.5 spelled an absent account `Account::default()`. There is no such
    // value now: an address nobody has written to has no shards.
    const nlohmann::json absent = {
        {"nonce", repeated('0', 32)},
        {"shards", nlohmann::json::array()},
    };
    const auto missing = stablecoin_module::detail::publicAccountRead(account_id, absent.dump());
    LOGOS_ASSERT_EQ(missing["status"].get<std::string>(), std::string("not_found"));

    // A shard that is present but empty says the same thing.
    const nlohmann::json empty_shard = {
        {"nonce", repeated('0', 32)},
        {"shards", nlohmann::json::array({
                       {{"program", repeated('A', 64)}, {"data", ""}},
                   })},
    };
    const auto empty =
        stablecoin_module::detail::publicAccountRead(account_id, empty_shard.dump());
    LOGOS_ASSERT_EQ(empty["status"].get<std::string>(), std::string("not_found"));

    auto malformed = validAccount();
    malformed["nonce"] = "short";
    const auto invalid =
        stablecoin_module::detail::publicAccountRead(account_id, malformed.dump());
    LOGOS_ASSERT_EQ(invalid["status"].get<std::string>(), std::string("backend_error"));

    // A shard whose program is not an account id is corruption, not absence.
    auto bad_key = validAccount();
    bad_key["shards"][0]["program"] = "not-a-program-id";
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::publicAccountRead(account_id, bad_key.dump())["status"]
            .get<std::string>(),
        std::string("backend_error"));

    // The v0.2.5 flat account has no shards array at all, so an unbumped lez_core
    // reads as corruption rather than silently as an empty account.
    const nlohmann::json flat = {
        {"program_owner", repeated('A', 64)},
        {"balance", repeated('0', 32)},
        {"nonce", repeated('C', 32)},
        {"data", "00ff"},
    };
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::publicAccountRead(account_id, flat.dump())["status"]
            .get<std::string>(),
        std::string("backend_error"));
}

LOGOS_TEST(wallet_submission_response_requires_success_and_full_hash) {
    const std::string hash = repeated('A', 64);
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::transactionId(
            nlohmann::json{{"success", true}, {"tx_hash", hash}}.dump()),
        repeated('a', 64));
    LOGOS_ASSERT_TRUE(stablecoin_module::detail::transactionId(
                          nlohmann::json{{"success", false}, {"tx_hash", hash}}.dump())
                          .empty());
    LOGOS_ASSERT_TRUE(stablecoin_module::detail::transactionId(
                          nlohmann::json{{"success", true}, {"tx_hash", "short"}}.dump())
                          .empty());
    LOGOS_ASSERT_TRUE(stablecoin_module::detail::transactionId("not-json").empty());
}

LOGOS_TEST(ffi_error_mapping_preserves_only_public_codes) {
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::stableFfiError("invalid_numeric_value"),
        std::string("invalid_numeric_value"));
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::stableFfiError("internal parse detail"),
        std::string("backend_error"));
    LOGOS_ASSERT_EQ(
        stablecoin_module::detail::stableFfiError(""),
        std::string("backend_error"));
}

LOGOS_TEST(instruction_words_are_validated_and_encoded_little_endian) {
    const nlohmann::json input = nlohmann::json::array(
        {std::uint64_t{0}, std::uint64_t{1},
         std::uint64_t{std::numeric_limits<std::uint8_t>::max()}});
    const std::vector<std::uint8_t> expected = {0x00, 0x01, 0xff};
    const auto actual = stablecoin_module::detail::jsonInstructionLeBytes(input);
    LOGOS_ASSERT_EQ(actual.size(), expected.size());
    for (std::size_t index = 0; index < expected.size(); ++index) {
        LOGOS_ASSERT_EQ(actual[index], expected[index]);
    }

    LOGOS_ASSERT_TRUE(stablecoin_module::detail::jsonInstructionLeBytes(
                          nlohmann::json::array({std::int64_t{-1}}))
                          .empty());
    LOGOS_ASSERT_TRUE(stablecoin_module::detail::jsonInstructionLeBytes(
                          nlohmann::json::array({1.5}))
                          .empty());
    LOGOS_ASSERT_TRUE(stablecoin_module::detail::jsonInstructionLeBytes(
                          nlohmann::json::array({std::uint64_t{256}}))
                          .empty());
}
