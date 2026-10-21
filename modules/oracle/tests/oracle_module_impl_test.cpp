#include "oracle_module_impl.h"

#include <logos_test.h>
#include <mock_store.h>

#include <QTemporaryFile>
#include <cstdlib>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>

#include "logos_sdk.h"

extern std::string oracleLastProgramRequest;
extern std::string oracleLastFeedRequest;

namespace {
using json = nlohmann::json;
const std::string PROGRAM(64, '1');
const std::string SOURCE(64, '2');
const std::string CLOCK(64, '3');
const std::string PAYER(64, '4');
const std::string PRICE(64, '5');
const std::string OTHER_PROGRAM(64, 'a');
const std::string HASH(64, 'b');

// Real base58 spellings of the matching 32-byte fixture IDs. The dependency
// mock provides the same normalization contract as lez_core.
const std::string PROGRAM58 = "29d2S7vB453rNYFdR5Ycwt7y9haRT5fwVwL9zTmBhfV2";
const std::string CLOCK58 = "4Ss5JMkXAD9Z7cktFEdrqeMuT6jGMF1pVozTyPHZ6zT4";
const std::string SOURCE58 = "3JF3sEqM796hk5WFqA6EtmEwJQ9quALszsfJyvXNQKy3";
const std::string PRICE58 = "6k78AbasGMFFrhG95Pj6jQbqkVt7FQMhVgemxJovWKR6";
const std::string OTHER58 = "CVDFLCAjXhVWiPXH9nTCTpCgVzmDVoiPzNJYuccr1dqB";

class Environment {
public:
    Environment(const char* key, const char* value) : key_(key) {
        if (const char* previous = std::getenv(key)) previous_ = previous;
        if (value)
            setenv(key, value, 1);
        else
            unsetenv(key);
    }
    ~Environment() {
        if (previous_)
            setenv(key_.c_str(), previous_->c_str(), 1);
        else
            unsetenv(key_.c_str());
    }
    Environment(const Environment&) = delete;
    Environment& operator=(const Environment&) = delete;

private:
    std::string key_;
    std::optional<std::string> previous_;
};

json programInfo(const std::string& program = PROGRAM, const std::string& base58 = PROGRAM58) {
    return {{"programId", base58},
            {"programIdHex", program},
            {"clockId", CLOCK58},
            {"clockIdHex", CLOCK},
            {"imageIdHex", ""}};
}

json accountSnapshot() {
    return {{"program_owner", std::string(64, '0')},
            {"balance", std::string(32, '0')},
            {"nonce", std::string(32, '0')},
            {"data", ""}};
}

json plan(const std::vector<bool>& signers) {
    std::vector<std::string> accounts = {PRICE, SOURCE, CLOCK};
    if (signers.size() == 4) accounts = {PRICE, SOURCE, PAYER, CLOCK};
    return {{"programId", PROGRAM},
            {"accountIds", accounts},
            {"signingRequirements", signers},
            {"instruction", {0, 127, 128, 255}}};
}

void assertError(const LogosMap& value, const char* code) {
    LOGOS_ASSERT_EQ(value.at("status"), json("error"));
    LOGOS_ASSERT_EQ(value.at("error"), json(code));
}

struct Fixture {
    Environment program_env{"TWAP_ORACLE_PROGRAM_ID", PROGRAM.c_str()};
    Environment binary_env{"TWAP_ORACLE_PROGRAM_BIN", nullptr};
    LogosTestContext context{"oracle_module"};
    LogosModules modules{context.api()};
    OracleModuleImpl module;
    std::string program_response;
    std::string feed_response;

    Fixture() {
        module._logosCoreSetLogosModulesPtr_(&modules);
        for (const auto& [base58, hex] :
             std::vector<std::pair<std::string, std::string>>{{PROGRAM58, PROGRAM},
                                                              {CLOCK58, CLOCK},
                                                              {SOURCE58, SOURCE},
                                                              {PRICE58, PRICE},
                                                              {OTHER58, OTHER_PROGRAM}}) {
            MockStore::instance()
                .when("lez_core", "account_id_from_base58")
                .withArgs(QVariantList{QString::fromStdString(base58)})
                .thenReturn(QString::fromStdString(hex));
        }
        setInfo(programInfo());
    }
    void setInfo(const json& value) {
        program_response = json{{"ok", true}, {"value", value}}.dump();
        context.mockCFunction("oracle_program_info").returns(program_response);
    }
    void inventory(const std::vector<std::string>& ids) {
        QVariantList accounts;
        for (const auto& id : ids)
            accounts.push_back(
                QVariantMap{{"account_id", QString::fromStdString(id)}, {"is_public", true}});
        context.mockModule("lez_core", "list_accounts").returnsVariant(accounts);
    }
    void submission(const std::string& response = json{
                        {"success", true},
                        {"tx_hash", HASH}}.dump()) {
        context.mockModule("lez_core", "send_generic_public_transaction").returns(response);
    }
};
}  // namespace

// Private seam: never part of the generated universal method inventory.
struct OracleModuleTestAccess {
    static nlohmann::json read(OracleModuleImpl& module, const std::string& id) {
        return module.readPublicAccount(id);
    }
    static LogosMap submit(OracleModuleImpl& module, const nlohmann::json& plan,
                           const std::string& payer) {
        return module.submitPlan(plan, payer,
                                 {{"priceAccountId", PRICE58}, {"priceAccountIdHex", PRICE}});
    }
};

LOGOS_TEST(program_selection_normalizes_preserves_and_clears_override) {
    Fixture f;
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(PROGRAM));
    LOGOS_ASSERT_EQ(f.module.setOracleProgramId({{"oracleProgramId", OTHER58}}).at("status"),
                    json("ok"));
    f.setInfo(programInfo(OTHER_PROGRAM, OTHER58));
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(OTHER_PROGRAM));
    LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest)["oracleProgramId"], json(OTHER_PROGRAM));
    assertError(f.module.setOracleProgramId({{"oracleProgramId", "invalid!"}}),
                "invalid_program_id");
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(OTHER_PROGRAM));
    assertError(f.module.setOracleProgramId({{"oracleProgramId", 1}}), "bad_request");
    LOGOS_ASSERT_EQ(
        f.module.setOracleProgramId({{"oracleProgramId", std::string(64, 'A')}}).at("status"),
        json("ok"));
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(OTHER_PROGRAM));
    LOGOS_ASSERT_EQ(f.module.setOracleProgramId({{"oracleProgramId", ""}}).at("status"),
                    json("ok"));
    f.setInfo(programInfo());
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(PROGRAM));
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 0);
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 0);
}

LOGOS_TEST(program_discovery_observes_environment_changes_on_same_instance) {
    Fixture f;
    LOGOS_ASSERT_EQ(f.module.programInfo().at("status"), json("ok"));
    {
        Environment changed("TWAP_ORACLE_PROGRAM_ID", OTHER58.c_str());
        f.setInfo(programInfo(OTHER_PROGRAM, OTHER58));
        LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(OTHER_PROGRAM));
    }
    f.setInfo(programInfo());
    LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(PROGRAM));
    LOGOS_ASSERT_EQ(f.context.cFunctionCallCount("oracle_program_info"), 3);
    LOGOS_ASSERT_EQ(f.context.cFunctionCallCount("oracle_free"), 3);
}

LOGOS_TEST(program_config_and_binary_failures_are_stable) {
    Fixture f;
    {
        Environment absent("TWAP_ORACLE_PROGRAM_ID", nullptr);
        Environment binary_only("TWAP_ORACLE_PROGRAM_BIN", "/missing-binary");
        assertError(f.module.programInfo(), "config_missing");
    }
    {
        Environment invalid("TWAP_ORACLE_PROGRAM_ID", "invalid!");
        assertError(f.module.programInfo(), "invalid_program_id");
    }
    {
        Environment invalid_binary("TWAP_ORACLE_PROGRAM_BIN", "/missing-binary");
        assertError(f.module.programInfo(), "invalid_program_binary");
    }
    LOGOS_ASSERT_EQ(f.context.cFunctionCallCount("oracle_program_info"), 0);
}

LOGOS_TEST(binary_verification_reads_live_header_on_every_call) {
    Fixture f;
    QTemporaryFile binary;
    LOGOS_ASSERT_TRUE(binary.open());
    binary.write("binary");
    binary.flush();
    const std::string path = binary.fileName().toStdString();
    Environment configured_binary("TWAP_ORACLE_PROGRAM_BIN", path.c_str());
    auto info = programInfo();
    info["imageIdHex"] = HASH;
    f.setInfo(info);
    auto first = accountSnapshot();
    first["data"] = "aabb";
    f.context.mockModule("lez_core", "get_account_public").returns(first.dump());
    LOGOS_ASSERT_EQ(f.module.programInfo().at("imageIdHex"), json(HASH));
    LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest)["programHeader"]["account"], first);
    LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest)["elf"], json("62696e617279"));
    auto second = first;
    second["data"] = "ccdd";
    f.context.mockModule("lez_core", "get_account_public").returns(second.dump());
    const std::string mismatch = R"({"ok":false,"error":"program_binary_mismatch"})";
    f.context.mockCFunction("oracle_program_info").returns(mismatch);
    assertError(f.module.programInfo(), "program_binary_mismatch");
    LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest)["programHeader"]["account"], second);
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 2);
}

LOGOS_TEST(account_read_seam_preserves_default_and_transport_failures) {
    Fixture f;
    f.context.mockModule("lez_core", "get_account_public").returns(accountSnapshot().dump());
    const auto read = OracleModuleTestAccess::read(f.module, PRICE);
    LOGOS_ASSERT_EQ(read["status"], json("ok"));
    LOGOS_ASSERT_EQ(read["account"], accountSnapshot());
    for (const auto& response : {std::string(), std::string("{"), std::string("{}"),
                                 UniversalLezCore::transportErrorSentinel()}) {
        f.context.mockModule("lez_core", "get_account_public").returns(response);
        LOGOS_ASSERT_EQ(OracleModuleTestAccess::read(f.module, PRICE)["status"],
                        json("account_read_failed"));
    }
}

LOGOS_TEST(malformed_ffi_replies_and_unknown_codes_fail_closed) {
    Fixture f;
    for (const auto& response :
         {std::string("{"), std::string("[]"), std::string("{}"),
          std::string(R"({"ok":1,"value":{}})"), std::string(R"({"ok":true,"value":[]})"),
          std::string(R"({"ok":true,"value":{}})"),
          std::string(R"({"ok":false,"error":"unknown"})")}) {
        f.context.mockCFunction("oracle_program_info").returns(response);
        assertError(f.module.programInfo(), "backend_error");
    }
    auto wrong_id = programInfo();
    wrong_id["programIdHex"] = OTHER_PROGRAM;
    f.setInfo(wrong_id);
    assertError(f.module.programInfo(), "backend_error");
    for (const auto* code :
         {"program_header_unavailable", "invalid_program_binary", "program_binary_mismatch"}) {
        const std::string reply = json{{"ok", false}, {"error", code}}.dump();
        f.context.mockCFunction("oracle_program_info").returns(reply);
        assertError(f.module.programInfo(), code);
    }
}

LOGOS_TEST(feed_discovery_sends_exact_decimal_strings_and_canonical_hex) {
    Fixture f;
    const json addresses = {{"currentTickAccountId", SOURCE58},
                            {"currentTickAccountIdHex", SOURCE},
                            {"priceObservationsId", PROGRAM58},
                            {"priceObservationsIdHex", PROGRAM},
                            {"priceAccountId", PRICE58},
                            {"priceAccountIdHex", PRICE},
                            {"clockId", CLOCK58},
                            {"clockIdHex", CLOCK}};
    f.feed_response = json{{"ok", true}, {"value", addresses}}.dump();
    f.context.mockCFunction("oracle_feed_addresses").returns(f.feed_response);
    const auto result = f.module.feedAddresses(
        {{"priceSourceId", SOURCE58}, {"windowDurationMs", "18446744073709551615"}});
    LOGOS_ASSERT_EQ(result.at("priceAccountIdHex"), json(PRICE));
    LOGOS_ASSERT_EQ(json::parse(oracleLastFeedRequest),
                    (json{{"oracleProgramId", PROGRAM},
                          {"priceSourceId", SOURCE},
                          {"windowDurationMs", "18446744073709551615"}}));
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 0);
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 0);
    assertError(f.module.feedAddresses({{"priceSourceId", SOURCE}, {"windowDurationMs", 2048}}),
                "bad_request");
    assertError(
        f.module.feedAddresses({{"priceSourceId", "invalid!"}, {"windowDurationMs", "2048"}}),
        "invalid_account_id");
    for (const auto* code : {"invalid_window", "invalid_numeric_value"}) {
        f.feed_response = json{{"ok", false}, {"error", code}}.dump();
        f.context.mockCFunction("oracle_feed_addresses").returns(f.feed_response);
        assertError(f.module.feedAddresses({{"priceSourceId", SOURCE}, {"windowDurationMs", "1"}}),
                    code);
    }
    auto invalid = addresses;
    invalid["priceAccountIdHex"] = "bad";
    f.feed_response = json{{"ok", true}, {"value", invalid}}.dump();
    f.context.mockCFunction("oracle_feed_addresses").returns(f.feed_response);
    assertError(f.module.feedAddresses({{"priceSourceId", SOURCE}, {"windowDurationMs", "2048"}}),
                "backend_error");
}

LOGOS_TEST(submission_forwards_bytes_and_any_signer_position_with_separate_payer) {
    for (const auto& flags : std::vector<std::vector<bool>>{{false, true, false},
                                                            {false, false, false},
                                                            {true, false, false},
                                                            {false, false, false, false}}) {
        Fixture f;
        const auto value = plan(flags);
        // Only actual instruction signers plus a separate payer are in the wallet.
        const std::string payer = OTHER_PROGRAM;
        std::vector<std::string> inventory{payer};
        QStringList expected_accounts;
        QVariantList expected_flags;
        for (std::size_t i = 0; i < flags.size(); ++i) {
            const std::string id = value["accountIds"][i].get<std::string>();
            expected_accounts.push_back(QString::fromStdString(id));
            expected_flags.push_back(flags[i]);
            if (flags[i]) inventory.push_back(id);
        }
        f.inventory(inventory);
        f.submission();
        const auto result = OracleModuleTestAccess::submit(f.module, value, payer);
        LOGOS_ASSERT_EQ(result.at("status"), json("ok"));
        LOGOS_ASSERT_EQ(result.at("error"), json(""));
        LOGOS_ASSERT_EQ(result.at("transactionId"), json(HASH));
        LOGOS_ASSERT_EQ(result.at("priceAccountId"), json(PRICE58));
        const auto expected =
            QVariantList{QVariant(expected_accounts), QVariant(expected_flags),
                         QVariant(QByteArray::fromHex("007f80ff")), QString::fromStdString(PROGRAM),
                         QString::fromStdString(payer)};
        LOGOS_ASSERT_TRUE(
            f.context.moduleCalledWith("lez_core", "send_generic_public_transaction", expected));
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 1);
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 0);
    }
}

LOGOS_TEST(source_can_pay_but_nonsigning_participants_cannot) {
    Fixture f;
    f.inventory({SOURCE});
    f.submission();
    LOGOS_ASSERT_EQ(
        OracleModuleTestAccess::submit(f.module, plan({false, true, false}), SOURCE).at("status"),
        json("ok"));
    for (const auto& payer : {PRICE, CLOCK, std::string(), std::string("invalid!")}) {
        assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false}), payer),
                    "invalid_fee_payer");
    }
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, false, false}), SOURCE),
                "invalid_fee_payer");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
}

LOGOS_TEST(missing_source_payer_and_wallet_failure_prevent_submission) {
    Fixture f;
    f.inventory({PAYER});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false}), PAYER),
                "source_not_signable");
    f.inventory({SOURCE});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false}), PAYER),
                "fee_payer_not_signable");
    f.inventory({});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, false, false}), PAYER),
                "fee_payer_not_signable");
    f.context.mockModule("lez_core", "list_accounts")
        .returnsVariant(
            QVariantList{QString::fromStdString(UniversalLezCore::transportErrorSentinel())});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, false, false}), PAYER),
                "backend_error");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
}

LOGOS_TEST(private_wallet_accounts_cannot_authorize_source_or_payer) {
    for (const bool source_public : {false, true}) {
        Fixture f;
        const QVariantList accounts = {
            QVariantMap{{"account_id", QString::fromStdString(SOURCE)},
                        {"is_public", source_public}},
            QVariantMap{{"account_id", QString::fromStdString(PAYER)},
                        {"is_public", !source_public}},
        };
        f.context.mockModule("lez_core", "list_accounts").returnsVariant(accounts);
        assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false}), PAYER),
                    source_public ? "fee_payer_not_signable" : "source_not_signable");
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"),
                        0);
    }
}

LOGOS_TEST(malformed_plans_and_failed_submissions_have_stable_errors) {
    Fixture f;
    f.inventory({SOURCE, PAYER});
    auto bad = plan({false, true, false});
    bad["instruction"] = json::array({256});
    assertError(OracleModuleTestAccess::submit(f.module, bad, PAYER), "backend_error");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 0);
    for (const auto& reply : {std::string(), std::string("not-json"),
                              std::string(R"({"success":false,"error":"insufficient_balance"})"),
                              json{{"success", true}, {"tx_hash", std::string(64, 'A')}}.dump(),
                              UniversalLezCore::transportErrorSentinel()}) {
        f.submission(reply);
        assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false}), PAYER),
                    "wallet_submission_failed");
    }
}
