#include "oracle_module_support.h"

#include <logos_test.h>

#include <limits>

namespace {
using json = nlohmann::json;
namespace support = oracle_module_support;
const std::string ID(64, '1');
const std::string SOURCE(64, '2');
const std::string CLOCK(64, '3');
const std::string PAYER(64, '4');

json snapshot() {
    return {{"program_owner", std::string(64, '0')},
            {"balance", std::string(32, '0')},
            {"nonce", std::string(32, '0')},
            {"data", ""}};
}
json planValue() {
    return {{"programId", ID},
            {"accountIds", {ID, SOURCE, CLOCK}},
            {"signingRequirements", {false, true, false}},
            {"instruction", {0, 127, 128, 255}}};
}
}  // namespace

LOGOS_TEST(default_account_is_successful_full_snapshot) {
    const auto read = support::publicAccountRead(ID, snapshot().dump());
    LOGOS_ASSERT_EQ(read["status"], json("ok"));
    LOGOS_ASSERT_EQ(read["account"], snapshot());
    LOGOS_ASSERT_EQ(read["id"], json(ID));
    auto initialized = snapshot();
    initialized["data"] = "ABff";
    LOGOS_ASSERT_EQ(support::publicAccountRead(ID, initialized.dump())["account"]["data"],
                    json("abff"));
}

LOGOS_TEST(read_failures_never_become_not_found) {
    for (const auto& response : {std::string(), std::string("{"), std::string("null"),
                                 std::string("[]"), std::string("{}")}) {
        const auto read = support::publicAccountRead(ID, response);
        LOGOS_ASSERT_EQ(read["status"], json("account_read_failed"));
        LOGOS_ASSERT_TRUE(!read.contains("account"));
    }
    LOGOS_ASSERT_EQ(support::publicAccountRead(ID, snapshot().dump(), false)["status"],
                    json("account_read_failed"));
    for (const auto* field : {"program_owner", "balance", "nonce", "data"}) {
        auto value = snapshot();
        value.erase(field);
        LOGOS_ASSERT_EQ(support::publicAccountRead(ID, value.dump())["status"],
                        json("account_read_failed"));
        for (const auto& malformed : {json("z0"), json("0"), json(0), json(nullptr), json(false)}) {
            value = snapshot();
            value[field] = malformed;
            LOGOS_ASSERT_EQ(support::publicAccountRead(ID, value.dump())["status"],
                            json("account_read_failed"));
        }
    }
}

LOGOS_TEST(normalize_ids_and_closed_error_allowlist) {
    const auto decoder = [](const std::string& text) {
        return text == "source" ? SOURCE : std::string();
    };
    LOGOS_ASSERT_EQ(support::normalizeAccountId(std::string(64, 'A'), decoder),
                    std::string(64, 'a'));
    LOGOS_ASSERT_EQ(support::normalizeAccountId("source", decoder), SOURCE);
    for (const auto& invalid :
         {std::string(), std::string(64, '0'), std::string("unknown"), std::string(63, '1')}) {
        LOGOS_ASSERT_TRUE(support::normalizeAccountId(invalid, decoder).empty());
    }
    for (const auto* code :
         {"bad_request", "config_missing", "invalid_program_id", "invalid_program_binary",
          "program_header_unavailable", "program_binary_mismatch", "invalid_account_id",
          "invalid_numeric_value", "invalid_window", "account_read_failed", "invalid_clock",
          "source_not_signable", "invalid_fee_payer", "fee_payer_not_signable",
          "wallet_submission_failed", "backend_error"}) {
        LOGOS_ASSERT_EQ(support::stableFfiError(code), std::string(code));
    }
    for (const auto* code : {"", "not_found", "future_domain_error", "panic: details"}) {
        LOGOS_ASSERT_EQ(support::stableFfiError(code), std::string("backend_error"));
    }
}

LOGOS_TEST(null_ffi_reply_is_rejected) {
    const auto result =
        support::callFfi([](const char*) -> char* { return nullptr; }, json::object());
    LOGOS_ASSERT_TRUE(!result.ok);
}

LOGOS_TEST(plans_require_exact_byte_arrays_and_matching_account_flags) {
    support::Plan plan;
    LOGOS_ASSERT_TRUE(support::parsePlan(planValue(), plan));
    LOGOS_ASSERT_TRUE(plan.instruction == std::vector<std::uint8_t>({0, 127, 128, 255}));
    for (const auto& invalid : {json(-1), json(256), json(1.0), json(true), json("1"),
                                json(std::numeric_limits<std::uint64_t>::max())}) {
        auto value = planValue();
        value["instruction"] = json::array({invalid});
        LOGOS_ASSERT_TRUE(!support::parsePlan(value, plan));
    }
    for (const auto* field : {"programId", "accountIds", "signingRequirements", "instruction"}) {
        auto value = planValue();
        value.erase(field);
        LOGOS_ASSERT_TRUE(!support::parsePlan(value, plan));
    }
    for (const auto& flags :
         {json::array(), json::array({false, true}), json::array({false, 1, false})}) {
        auto value = planValue();
        value["signingRequirements"] = flags;
        LOGOS_ASSERT_TRUE(!support::parsePlan(value, plan));
    }
    auto empty = planValue();
    empty["instruction"] = json::array();
    LOGOS_ASSERT_TRUE(!support::parsePlan(empty, plan));
    auto bad_id = planValue();
    bad_id["accountIds"][1] = "bad-id";
    LOGOS_ASSERT_TRUE(!support::parsePlan(bad_id, plan));
}

LOGOS_TEST(fee_payer_aliases_and_wallet_inventory_are_independent_of_balance) {
    support::Plan plan;
    LOGOS_ASSERT_TRUE(support::parsePlan(planValue(), plan));
    LOGOS_ASSERT_TRUE(support::validateFeePayer(plan, SOURCE).empty());
    LOGOS_ASSERT_TRUE(support::validateFeePayer(plan, PAYER).empty());
    for (const auto& invalid : {ID, CLOCK, std::string(), std::string("bad")}) {
        LOGOS_ASSERT_EQ(support::validateFeePayer(plan, invalid), std::string("invalid_fee_payer"));
    }
    const auto normalize = [](const std::string& id) { return id; };
    const json wallet = json::array({{{"account_id", SOURCE}, {"is_public", true}},
                                     {{"account_id", PAYER}, {"is_public", true}}});
    LOGOS_ASSERT_TRUE(
        support::validateWalletInventory(wallet, true, plan, PAYER, normalize).empty());
    LOGOS_ASSERT_EQ(support::validateWalletInventory(wallet, false, plan, PAYER, normalize),
                    std::string("backend_error"));
    LOGOS_ASSERT_EQ(support::validateWalletInventory(json::object(), true, plan, PAYER, normalize),
                    std::string("backend_error"));
    for (const auto& malformed :
         {json("bad"), json::object(), json{{"account_id", SOURCE}, {"is_public", "true"}}}) {
        LOGOS_ASSERT_EQ(support::validateWalletInventory(json::array({malformed}), true, plan,
                                                         PAYER, normalize),
                        std::string("backend_error"));
    }
    auto private_source = wallet;
    private_source[0]["is_public"] = false;
    LOGOS_ASSERT_EQ(support::validateWalletInventory(private_source, true, plan, PAYER, normalize),
                    std::string("source_not_signable"));
    auto private_payer = wallet;
    private_payer[1]["is_public"] = false;
    LOGOS_ASSERT_EQ(support::validateWalletInventory(private_payer, true, plan, PAYER, normalize),
                    std::string("fee_payer_not_signable"));
}

LOGOS_TEST(transaction_hash_must_be_lowercase_hex_and_reply_must_succeed) {
    const json valid = {{"success", true}, {"tx_hash", std::string(64, 'a')}};
    LOGOS_ASSERT_EQ(support::transactionId(valid.dump()), std::string(64, 'a'));
    for (const auto& hash : {json(std::string(64, 'A')), json(std::string(63, 'a')),
                             json(std::string(64, 'g')), json(nullptr)}) {
        auto reply = valid;
        reply["tx_hash"] = hash;
        LOGOS_ASSERT_TRUE(support::transactionId(reply.dump()).empty());
    }
    for (const auto& state : {json(false), json(1), json("true"), json(nullptr)}) {
        auto reply = valid;
        reply["success"] = state;
        LOGOS_ASSERT_TRUE(support::transactionId(reply.dump()).empty());
    }
    LOGOS_ASSERT_TRUE(support::transactionId("not-json").empty());
}
