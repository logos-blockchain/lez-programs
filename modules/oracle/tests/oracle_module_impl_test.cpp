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
extern std::string oracleLastPlanRequest;

namespace {
using json = nlohmann::json;
const std::string PROGRAM(64, '1');
const std::string SOURCE(64, '2');
const std::string CLOCK(64, '3');
const std::string TICK(64, '4');
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
// Derived feed accounts returned by the mocked planners.
const std::string TICK_ACCOUNT(64, '6');
const std::string TICK_ACCOUNT58 = "7tj9biW3KRJ7EEWmVUGigHiouCTXhV2dzcyvwma7Cyu7";
const std::string OBSERVATIONS(64, '7');
const std::string OBSERVATIONS58 = "93MB2qRDNVLxbmmPuYpLdAqn3u2x9ZhaVZK5wELHueP8";
const std::string PRICE_ACCOUNT(64, '8');
const std::string PRICE_ACCOUNT58 = "AByCTxLPRZPoyK22KdMxa3xkCbcNbeNWzVeEvh6UcJs9";

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
            {"imageIdHex", program}};
}

json accountSnapshot() {
    return {{"program_owner", std::string(64, '0')},
            {"balance", std::string(32, '0')},
            {"nonce", std::string(32, '0')},
            {"data", ""}};
}

// risc0 serde instruction words; lez_core receives them flattened little-endian.
const json INSTRUCTION_WORDS = {0, 1, 0x80000000u, 0xffffffffu};
const char INSTRUCTION_BYTES_HEX[] = "000000000100000000000080ffffffff";

json plan(const std::vector<bool>& signers) {
    std::vector<std::string> accounts = {PRICE, SOURCE, CLOCK};
    if (signers.size() == 4) accounts = {PRICE, SOURCE, TICK, CLOCK};
    return {{"programId", PROGRAM},
            {"accountIds", accounts},
            {"signingRequirements", signers},
            {"instruction", INSTRUCTION_WORDS}};
}

void assertError(const LogosMap& value, const char* code) {
    LOGOS_ASSERT_EQ(value.at("status"), json("error"));
    LOGOS_ASSERT_EQ(value.at("error"), json(code));
}

std::string writeBinary(QTemporaryFile& file) {
    LOGOS_ASSERT_TRUE(file.open());
    file.write("binary");
    file.flush();
    return file.fileName().toStdString();
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
                                                              {OTHER58, OTHER_PROGRAM},
                                                              {TICK_ACCOUNT58, TICK_ACCOUNT},
                                                              {OBSERVATIONS58, OBSERVATIONS},
                                                              {PRICE_ACCOUNT58, PRICE_ACCOUNT}}) {
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
    // Mocks lez_core's read of one account; a default snapshot is "absent".
    void account(const std::string& id, const json& snapshot) {
        MockStore::instance()
            .when("lez_core", "get_account_public")
            .withArgs(QVariantList{QString::fromStdString(id)})
            .thenReturn(QString::fromStdString(snapshot.dump()));
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
    static LogosMap submit(OracleModuleImpl& module, const nlohmann::json& plan) {
        return module.submitPlan(plan, {{"priceAccountId", PRICE58}, {"priceAccountIdHex", PRICE}});
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
    {
        Environment absent("TWAP_ORACLE_PROGRAM_ID", nullptr);
        Environment invalid_binary("TWAP_ORACLE_PROGRAM_BIN", "/missing-binary");
        assertError(f.module.programInfo(), "invalid_program_binary");
    }
    LOGOS_ASSERT_EQ(f.context.cFunctionCallCount("oracle_program_info"), 0);
}

LOGOS_TEST(binary_supplies_or_verifies_the_image_id_without_chain_reads) {
    Fixture f;
    QTemporaryFile binary;
    const std::string path = writeBinary(binary);
    Environment configured_binary("TWAP_ORACLE_PROGRAM_BIN", path.c_str());

    LOGOS_ASSERT_EQ(f.module.programInfo().at("imageIdHex"), json(PROGRAM));
    LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest),
                    (json{{"oracleProgramId", PROGRAM}, {"elf", "62696e617279"}}));

    const std::string mismatch = R"({"ok":false,"error":"program_binary_mismatch"})";
    f.context.mockCFunction("oracle_program_info").returns(mismatch);
    assertError(f.module.programInfo(), "program_binary_mismatch");

    {
        // A binary alone derives the program id.
        Environment absent("TWAP_ORACLE_PROGRAM_ID", nullptr);
        f.setInfo(programInfo(OTHER_PROGRAM, OTHER58));
        LOGOS_ASSERT_EQ(f.module.programInfo().at("programIdHex"), json(OTHER_PROGRAM));
        LOGOS_ASSERT_EQ(json::parse(oracleLastProgramRequest), (json{{"elf", "62696e617279"}}));
    }
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 0);
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
          std::string(R"({"ok":false,"error":"unknown"})"),
          std::string(R"({"ok":false,"error":"program_header_unavailable"})")}) {
        f.context.mockCFunction("oracle_program_info").returns(response);
        assertError(f.module.programInfo(), "backend_error");
    }
    auto wrong_id = programInfo();
    wrong_id["programIdHex"] = OTHER_PROGRAM;
    f.setInfo(wrong_id);
    assertError(f.module.programInfo(), "backend_error");
    auto split_identity = programInfo();
    split_identity["imageIdHex"] = HASH;
    f.setInfo(split_identity);
    assertError(f.module.programInfo(), "backend_error");
    for (const auto* code : {"invalid_program_id", "invalid_program_binary", "program_binary_mismatch"}) {
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

LOGOS_TEST(submission_forwards_instruction_bytes_and_any_signer_position) {
    for (const auto& flags : std::vector<std::vector<bool>>{{false, true, false},
                                                            {false, false, false},
                                                            {true, false, false},
                                                            {false, false, false, false}}) {
        Fixture f;
        const auto value = plan(flags);
        // Only the instruction's actual signers are in the wallet.
        std::vector<std::string> inventory;
        QStringList expected_accounts;
        QVariantList expected_flags;
        for (std::size_t i = 0; i < flags.size(); ++i) {
            const std::string id = value["accountIds"][i].get<std::string>();
            expected_accounts.push_back(QString::fromStdString(id));
            expected_flags.push_back(static_cast<bool>(flags[i]));
            if (flags[i]) inventory.push_back(id);
        }
        f.inventory(inventory);
        f.submission();
        const auto result = OracleModuleTestAccess::submit(f.module, value);
        LOGOS_ASSERT_EQ(result.at("status"), json("ok"));
        LOGOS_ASSERT_EQ(result.at("error"), json(""));
        LOGOS_ASSERT_EQ(result.at("transactionId"), json(HASH));
        LOGOS_ASSERT_EQ(result.at("priceAccountId"), json(PRICE58));
        const auto expected =
            QVariantList{QVariant(expected_accounts), QVariant(expected_flags),
                         QVariant(QByteArray::fromHex(INSTRUCTION_BYTES_HEX)),
                         QString::fromStdString(PROGRAM)};
        LOGOS_ASSERT_TRUE(
            f.context.moduleCalledWith("lez_core", "send_generic_public_transaction", expected));
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 1);
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"), 0);
    }
}

LOGOS_TEST(missing_source_and_wallet_failure_prevent_submission) {
    Fixture f;
    f.inventory({PRICE});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false})),
                "source_not_signable");
    f.inventory({});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false})),
                "source_not_signable");
    f.context.mockModule("lez_core", "list_accounts")
        .returnsVariant(
            QVariantList{QString::fromStdString(UniversalLezCore::transportErrorSentinel())});
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, false, false})),
                "backend_error");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
}

LOGOS_TEST(private_wallet_accounts_cannot_authorize_the_source) {
    Fixture f;
    const QVariantList accounts = {
        QVariantMap{{"account_id", QString::fromStdString(SOURCE)}, {"is_public", false}},
    };
    f.context.mockModule("lez_core", "list_accounts").returnsVariant(accounts);
    assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false})),
                "source_not_signable");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
}

LOGOS_TEST(malformed_plans_and_failed_submissions_have_stable_errors) {
    Fixture f;
    f.inventory({SOURCE});
    auto bad = plan({false, true, false});
    bad["instruction"] = json::array({std::uint64_t{0x100000000}});
    assertError(OracleModuleTestAccess::submit(f.module, bad), "backend_error");
    LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "list_accounts"), 0);
    for (const auto& reply : {std::string(), std::string("not-json"),
                              std::string(R"({"success":false,"error":"insufficient_balance"})"),
                              json{{"success", true}, {"tx_hash", std::string(64, 'A')}}.dump(),
                              UniversalLezCore::transportErrorSentinel()}) {
        f.submission(reply);
        assertError(OracleModuleTestAccess::submit(f.module, plan({false, true, false})),
                    "wallet_submission_failed");
    }
}

namespace {
json ownedSnapshot(const std::string& owner = PROGRAM) {
    auto snapshot = accountSnapshot();
    snapshot["program_owner"] = owner;
    snapshot["data"] = "abcd";
    return snapshot;
}

json identity(const std::string& base58, const std::string& hex) {
    return {{"id", base58}, {"hex", hex}};
}

// One row per transaction method: its planner mock, its public request, the
// accounts its plan touches, and which of those must already exist.
struct Operation {
    const char* name;
    const char* planner;
    LogosMap (OracleModuleImpl::*method)(const LogosMap&);
    json request;
    std::vector<bool> signers;
    std::vector<std::pair<std::string, json>> identities;
    std::vector<std::pair<std::string, bool>> requirements;
};

std::vector<Operation> operations() {
    const auto tick = identity(TICK_ACCOUNT58, TICK_ACCOUNT);
    const auto observations = identity(OBSERVATIONS58, OBSERVATIONS);
    const auto price = identity(PRICE_ACCOUNT58, PRICE_ACCOUNT);
    return {
        {"createCurrentTickAccount", "oracle_create_current_tick_account_plan",
         &OracleModuleImpl::createCurrentTickAccount,
         {{"priceSourceId", SOURCE58}, {"initialPrice", "18446744073709551616"}},
         {false, true, false},
         {{"currentTickAccountId", tick}},
         {{TICK_ACCOUNT, false}}},
        {"createPriceObservations", "oracle_create_price_observations_plan",
         &OracleModuleImpl::createPriceObservations,
         {{"priceSourceId", SOURCE58}, {"initialTick", "-42"}, {"windowDurationMs", "86400000"}},
         {false, true, false},
         {{"priceObservationsId", observations}},
         {{OBSERVATIONS, false}}},
        {"createOraclePriceAccount", "oracle_create_oracle_price_account_plan",
         &OracleModuleImpl::createOraclePriceAccount,
         {{"priceSourceId", SOURCE58},
          {"baseAssetId", PRICE58},
          {"quoteAssetId", OTHER58},
          {"initialPrice", "18446744073709551616"},
          {"windowDurationMs", "86400000"}},
         {false, true, false},
         {{"priceAccountId", price}},
         {{PRICE_ACCOUNT, false}}},
        {"updateCurrentTick", "oracle_update_current_tick_plan",
         &OracleModuleImpl::updateCurrentTick,
         {{"priceSourceId", SOURCE58}, {"price", "0"}},
         {false, true, false},
         {{"currentTickAccountId", tick}},
         {{TICK_ACCOUNT, true}}},
        {"recordTick", "oracle_record_tick_plan", &OracleModuleImpl::recordTick,
         {{"priceSourceId", SOURCE58}, {"windowDurationMs", "86400000"}},
         {false, false, false},
         {{"priceObservationsId", observations}, {"currentTickAccountId", tick}},
         {{OBSERVATIONS, true}, {TICK_ACCOUNT, true}}},
        {"publishPrice", "oracle_publish_price_plan", &OracleModuleImpl::publishPrice,
         {{"priceSourceId", SOURCE58}, {"windowDurationMs", "86400000"}},
         {false, false, false, false},
         {{"priceObservationsId", observations},
          {"priceAccountId", price},
          {"currentTickAccountId", tick}},
         {{OBSERVATIONS, true}, {PRICE_ACCOUNT, true}, {TICK_ACCOUNT, true}}},
    };
}

json planReply(const Operation& op, const std::string& program = PROGRAM) {
    std::vector<std::string> accounts;
    for (const auto& [key, value] : op.identities) accounts.push_back(value["hex"]);
    while (accounts.size() + 1 < op.signers.size()) accounts.push_back(SOURCE);
    accounts.push_back(CLOCK);
    json identities = {{"priceSourceId", SOURCE58},
                       {"priceSourceIdHex", SOURCE},
                       {"clockId", CLOCK58},
                       {"clockIdHex", CLOCK}};
    for (const auto& [key, value] : op.identities) {
        identities[key] = value["id"];
        identities[key + "Hex"] = value["hex"];
    }
    return {{"ok", true},
            {"value",
             {{"plan",
               {{"programId", program},
                {"accountIds", accounts},
                {"signingRequirements", op.signers},
                {"instruction", INSTRUCTION_WORDS}}},
              {"identities", identities}}}};
}

// Every requirement satisfied: creation targets absent, maintained accounts
// owned by the oracle program, and the source (when it signs) in the wallet.
void satisfy(Fixture& f, const Operation& op) {
    f.context.mockCFunction(op.planner).returns(planReply(op).dump());
    for (const auto& [id, initialized] : op.requirements)
        f.account(id, initialized ? ownedSnapshot() : accountSnapshot());
    f.inventory({SOURCE});
    f.submission();
}

LogosMap invoke(Fixture& f, const Operation& op) {
    return (f.module.*op.method)(op.request.get<LogosMap>());
}
}  // namespace

LOGOS_TEST(transaction_methods_plan_check_accounts_and_submit) {
    for (const auto& op : operations()) {
        Fixture f;
        satisfy(f, op);
        const auto result = invoke(f, op);
        LOGOS_ASSERT_EQ(result.at("status"), json("ok"));
        LOGOS_ASSERT_EQ(result.at("transactionId"), json(HASH));
        LOGOS_ASSERT_EQ(result.at("priceSourceId"), json(SOURCE58));
        for (const auto& [key, value] : op.identities) {
            LOGOS_ASSERT_EQ(result.at(key), value["id"]);
            LOGOS_ASSERT_EQ(result.at(key + "Hex"), value["hex"]);
        }
        // The planner gets the resolved program and normalized ids, with
        // numeric arguments passed through as the caller's decimal strings.
        json expected = op.request;
        expected["oracleProgramId"] = PROGRAM;
        expected["priceSourceId"] = SOURCE;
        if (expected.contains("baseAssetId")) expected["baseAssetId"] = PRICE;
        if (expected.contains("quoteAssetId")) expected["quoteAssetId"] = OTHER_PROGRAM;
        LOGOS_ASSERT_EQ(json::parse(oracleLastPlanRequest), expected);
        LOGOS_ASSERT_EQ(f.context.cFunctionCallCount(op.planner), 1);
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "get_account_public"),
                        static_cast<int>(op.requirements.size()));
        LOGOS_ASSERT_EQ(
            f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
    }
}

LOGOS_TEST(creation_refuses_existing_accounts_and_maintenance_requires_feed_state) {
    for (const auto& op : operations()) {
        for (const auto& [id, initialized] : op.requirements) {
            Fixture f;
            satisfy(f, op);
            if (initialized) {
                f.account(id, accountSnapshot());
                assertError(invoke(f, op), "account_not_initialized");
                f.account(id, ownedSnapshot(OTHER_PROGRAM));
                assertError(invoke(f, op), "account_not_initialized");
            } else {
                f.account(id, ownedSnapshot());
                assertError(invoke(f, op), "already_initialized");
                auto funded = accountSnapshot();
                funded["balance"] = "01" + std::string(30, '0');
                f.account(id, funded);
                assertError(invoke(f, op), "already_initialized");
            }
            f.context.mockModule("lez_core", "get_account_public").returns("");
            assertError(invoke(f, op), "account_read_failed");
            LOGOS_ASSERT_EQ(
                f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
        }
    }
}

LOGOS_TEST(source_writes_need_the_source_in_the_wallet_but_maintenance_does_not) {
    for (const auto& op : operations()) {
        Fixture f;
        satisfy(f, op);
        f.inventory({});
        const bool source_signs = op.signers.size() == 3 && op.signers[1];
        const auto result = invoke(f, op);
        if (source_signs) {
            assertError(result, "source_not_signable");
        } else {
            LOGOS_ASSERT_EQ(result.at("status"), json("ok"));
        }
        LOGOS_ASSERT_EQ(f.context.moduleCallCount("lez_core", "send_generic_public_transaction"),
                        source_signs ? 0 : 1);
    }
}

LOGOS_TEST(transaction_inputs_and_planner_failures_have_stable_errors) {
    for (const auto& op : operations()) {
        Fixture f;
        satisfy(f, op);
        for (const auto& [key, value] : op.request.items()) {
            auto missing = op.request;
            missing.erase(key);
            assertError((f.module.*op.method)(missing.get<LogosMap>()), "bad_request");
            auto numeric = op.request;
            numeric[key] = 1;
            assertError((f.module.*op.method)(numeric.get<LogosMap>()), "bad_request");
        }
        auto bad_source = op.request;
        bad_source["priceSourceId"] = "invalid!";
        assertError((f.module.*op.method)(bad_source.get<LogosMap>()), "invalid_account_id");
        LOGOS_ASSERT_EQ(f.context.cFunctionCallCount(op.planner), 0);

        for (const auto* code : {"invalid_window", "invalid_numeric_value", "invalid_price",
                                 "invalid_program_id", "invalid_account_id"}) {
            f.context.mockCFunction(op.planner).returns(json{{"ok", false}, {"error", code}}.dump());
            assertError(invoke(f, op), code);
        }
        f.context.mockCFunction(op.planner).returns(R"({"ok":false,"error":"guest panic"})");
        assertError(invoke(f, op), "backend_error");
        // A plan for another program, or malformed identities, never submits.
        f.context.mockCFunction(op.planner).returns(planReply(op, OTHER_PROGRAM).dump());
        assertError(invoke(f, op), "backend_error");
        auto broken = planReply(op);
        broken["value"]["identities"]["priceSourceIdHex"] = OTHER_PROGRAM;
        f.context.mockCFunction(op.planner).returns(broken.dump());
        assertError(invoke(f, op), "backend_error");
        auto missing_plan = planReply(op);
        missing_plan["value"].erase("plan");
        f.context.mockCFunction(op.planner).returns(missing_plan.dump());
        assertError(invoke(f, op), "backend_error");
        LOGOS_ASSERT_EQ(
            f.context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }
    for (const auto& op : operations()) {
        Fixture f;
        Environment absent("TWAP_ORACLE_PROGRAM_ID", nullptr);
        assertError(invoke(f, op), "config_missing");
        LOGOS_ASSERT_EQ(f.context.cFunctionCallCount(op.planner), 0);
    }
}

LOGOS_TEST(create_oracle_price_account_normalizes_asset_ids) {
    const auto op = operations()[2];
    Fixture f;
    satisfy(f, op);
    for (const auto* key : {"baseAssetId", "quoteAssetId"}) {
        auto request = op.request;
        request[key] = "invalid!";
        assertError((f.module.*op.method)(request.get<LogosMap>()), "invalid_account_id");
    }
    auto hex = op.request;
    hex["baseAssetId"] = std::string(64, '5');
    LOGOS_ASSERT_EQ((f.module.*op.method)(hex.get<LogosMap>()).at("status"), json("ok"));
    LOGOS_ASSERT_EQ(json::parse(oracleLastPlanRequest)["baseAssetId"], json(PRICE));
}
