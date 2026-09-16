#include "stablecoin_module_impl.h"

#include <cstdlib>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "logos_sdk.h"

namespace {

using json = nlohmann::json;

const std::string PROGRAM_ID_HEX(64, '1');
const std::string ACCUMULATOR_ID_HEX(64, '2');
const std::string PROTOCOL_PARAMETERS_ID_HEX(64, '3');
const std::string REDEMPTION_STATE_ID_HEX(64, '4');
const std::string ORACLE_ID_HEX(64, '8');
const std::string CLOCK_ID_HEX(64, '7');
const std::string CALLER_ID_HEX(64, '9');
const std::string OTHER_CALLER_ID_HEX(64, 'a');
const std::string TRANSACTION_ID_HEX(64, 'b');

class ScopedEnvironment {
public:
    ScopedEnvironment(std::string name, const char* value)
        : name_(std::move(name)) {
        if (const char* previous = std::getenv(name_.c_str()); previous != nullptr) {
            previous_ = previous;
        }
        if (value == nullptr) {
            unsetenv(name_.c_str());
        } else {
            setenv(name_.c_str(), value, 1);
        }
    }

    ~ScopedEnvironment() {
        if (previous_.has_value()) {
            setenv(name_.c_str(), previous_->c_str(), 1);
        } else {
            unsetenv(name_.c_str());
        }
    }

    ScopedEnvironment(const ScopedEnvironment&) = delete;
    ScopedEnvironment& operator=(const ScopedEnvironment&) = delete;

private:
    std::string name_;
    std::optional<std::string> previous_;
};

json programInfoValue() {
    return {
        {"programId", "program-id"},
        {"programIdHex", PROGRAM_ID_HEX},
        {"protocolParametersId", "protocol-parameters-id"},
        {"protocolParametersIdHex", PROTOCOL_PARAMETERS_ID_HEX},
        {"stabilityFeeAccumulatorId", "stability-fee-accumulator-id"},
        {"stabilityFeeAccumulatorIdHex", ACCUMULATOR_ID_HEX},
        {"redemptionPriceStateId", "redemption-price-state-id"},
        {"redemptionPriceStateIdHex", REDEMPTION_STATE_ID_HEX},
        {"stablecoinDefinitionId", "stablecoin-definition-id"},
        {"stablecoinDefinitionIdHex", std::string(64, '5')},
        {"stablecoinMasterHoldingId", "stablecoin-master-holding-id"},
        {"stablecoinMasterHoldingIdHex", std::string(64, '6')},
        {"clockId", "clock-id"},
        {"clockIdHex", CLOCK_ID_HEX},
    };
}

std::string successEnvelope(const json& value) {
    return json{{"ok", true}, {"value", value}}.dump();
}

std::string failureEnvelope(const std::string& error) {
    return json{{"ok", false}, {"error", error}}.dump();
}

std::string initializedAccount() {
    return json{
        {"program_owner", PROGRAM_ID_HEX},
        {"balance", std::string(32, '0')},
        {"nonce", std::string(32, '0')},
        {"data", "00"},
    }.dump();
}

QVariantList walletAccounts(const std::string& account_id) {
    QVariantMap account;
    account.insert("account_id", QString::fromStdString(account_id));
    account.insert("is_public", true);
    return QVariantList{QVariant(account)};
}

std::vector<std::string> accrueAccounts() {
    return {
        CALLER_ID_HEX,
        PROTOCOL_PARAMETERS_ID_HEX,
        ACCUMULATOR_ID_HEX,
        CLOCK_ID_HEX,
    };
}

std::vector<std::string> updateAccounts() {
    return {
        CALLER_ID_HEX,
        PROTOCOL_PARAMETERS_ID_HEX,
        REDEMPTION_STATE_ID_HEX,
        ORACLE_ID_HEX,
        CLOCK_ID_HEX,
    };
}

std::vector<std::string> refreshAccounts() {
    return {
        CALLER_ID_HEX,
        PROTOCOL_PARAMETERS_ID_HEX,
        ACCUMULATOR_ID_HEX,
        REDEMPTION_STATE_ID_HEX,
        ORACLE_ID_HEX,
        CLOCK_ID_HEX,
    };
}

json submissionPlan(const std::vector<std::string>& account_ids,
                    std::uint32_t instruction_word) {
    std::vector<bool> signing_requirements(account_ids.size(), false);
    signing_requirements.front() = true;
    return {
        {"programId", PROGRAM_ID_HEX},
        {"accountIds", account_ids},
        {"signingRequirements", signing_requirements},
        {"instruction", json::array({instruction_word})},
    };
}

QVariantList submissionArguments(const std::vector<std::string>& account_ids,
                                 std::uint32_t instruction_word) {
    QStringList qt_account_ids;
    QVariantList signing_requirements;
    for (std::size_t index = 0; index < account_ids.size(); ++index) {
        qt_account_ids.push_back(QString::fromStdString(account_ids[index]));
        signing_requirements.push_back(index == 0);
    }
    const QByteArray instruction(
        1,
        static_cast<char>(instruction_word));
    QByteArray instruction_le = instruction;
    instruction_le.append(3, '\0');
    return {
        QVariant(qt_account_ids),
        QVariant(signing_requirements),
        QVariant(instruction_le),
        QVariant(QString::fromStdString(PROGRAM_ID_HEX)),
    };
}

std::string successfulTransaction() {
    return json{{"success", true}, {"tx_hash", TRANSACTION_ID_HEX}}.dump();
}

void attachModules(StablecoinModuleImpl& module, LogosModules& modules) {
    module._logosCoreSetLogosModulesPtr_(&modules);
}

void assertError(const LogosMap& response, const std::string& error) {
    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("error"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), error);
}

}  // namespace

LOGOS_TEST(stability_fee_accumulator_reads_once_and_returns_exact_snapshot) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json decoded = {
        {"accountId", "E4tfkjjkPz2g1G3bpgkXQx4M7e7g4Lr2Mr8bxvAsSmzE"},
        {"accountIdHex", ACCUMULATOR_ID_HEX},
        {"accumulatedRateAtLastAccrual", "340282366920938463463374607431768211455"},
        {"lastAccruedAt", "18446744073709551615"},
    };
    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = successEnvelope(decoded);
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_decode_stability_fee_accumulator")
        .returns(decoder_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.stabilityFeeAccumulator();

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
    LOGOS_ASSERT_EQ(response["stabilityFeeAccumulator"], decoded);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "get_account_public",
        QVariantList{QVariant(QString::fromStdString(ACCUMULATOR_ID_HEX))}));
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_stability_fee_accumulator"), 1);
}

LOGOS_TEST(stability_fee_accumulator_maps_missing_account_to_not_initialized) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("");

    const LogosMap response = module.stabilityFeeAccumulator();

    assertError(response, "not_initialized");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_stability_fee_accumulator"), 0);
}

LOGOS_TEST(stability_fee_accumulator_rejects_malformed_account_response) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("not-json");

    const LogosMap response = module.stabilityFeeAccumulator();

    assertError(response, "account_read_failed");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_stability_fee_accumulator"), 0);
}

LOGOS_TEST(stability_fee_accumulator_preserves_stable_decoder_errors) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = failureEnvelope(
        "stability_fee_accumulator_pda_mismatch");
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_decode_stability_fee_accumulator")
        .returns(decoder_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.stabilityFeeAccumulator();

    assertError(response, "stability_fee_accumulator_pda_mismatch");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_stability_fee_accumulator"), 1);
}

LOGOS_TEST(redemption_price_state_reads_once_and_returns_exact_snapshot) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json decoded = {
        {"accountId", "redemption-price-state-id"},
        {"accountIdHex", REDEMPTION_STATE_ID_HEX},
        {"redemptionPriceAtLastUpdate", "340282366920938463463374607431768211455"},
        {"redemptionRatePerMillisecond", "340282366920938463463374607431768211455"},
        {"controllerIntegralTerm", "-170141183460469231731687303715884105728"},
        {"lastUpdatedAt", "18446744073709551615"},
    };
    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = successEnvelope(decoded);
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_decode_redemption_price_state")
        .returns(decoder_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.redemptionPriceState();

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
    LOGOS_ASSERT_EQ(response["redemptionPriceState"], decoded);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "get_account_public",
        QVariantList{QVariant(QString::fromStdString(REDEMPTION_STATE_ID_HEX))}));
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_redemption_price_state"), 1);
}

LOGOS_TEST(redemption_price_state_maps_missing_account_to_not_initialized) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("");

    const LogosMap response = module.redemptionPriceState();

    assertError(response, "not_initialized");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_redemption_price_state"), 0);
}

LOGOS_TEST(redemption_price_state_rejects_malformed_account_response) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("not-json");

    const LogosMap response = module.redemptionPriceState();

    assertError(response, "account_read_failed");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_redemption_price_state"), 0);
}

LOGOS_TEST(redemption_price_state_preserves_stable_decoder_errors) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = failureEnvelope(
        "redemption_price_state_pda_mismatch");
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_decode_redemption_price_state")
        .returns(decoder_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.redemptionPriceState();

    assertError(response, "redemption_price_state_pda_mismatch");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_redemption_price_state"), 1);
}

LOGOS_TEST(current_global_state_reads_all_sources_and_preserves_exact_projection) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json projected = {
        {"accumulatedRateAtLastAccrual", "340282366920938463463374607431768211455"},
        {"lastAccruedAt", "18446744073709551611"},
        {"redemptionPriceAtLastUpdate", "340282366920938463463374607431768211454"},
        {"lastUpdatedAt", "18446744073709551612"},
        {"currentAccumulatedRate", "340282366920938463463374607431768211453"},
        {"currentRedemptionPrice", "340282366920938463463374607431768211452"},
        {"projectedAt", "18446744073709551615"},
    };
    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string projection_response = successEnvelope(projected);
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_current_global_state").returns(projection_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.currentGlobalState();

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
    LOGOS_ASSERT_EQ(response["currentGlobalState"], projected);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 4);
    for (const auto& account_id : {
             PROTOCOL_PARAMETERS_ID_HEX,
             ACCUMULATOR_ID_HEX,
             REDEMPTION_STATE_ID_HEX,
             CLOCK_ID_HEX,
         }) {
        LOGOS_ASSERT_TRUE(context.moduleCalledWith(
            "lez_core",
            "get_account_public",
            QVariantList{QVariant(QString::fromStdString(account_id))}));
    }
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_current_global_state"), 1);
}

LOGOS_TEST(current_global_state_maps_missing_globals_to_not_initialized) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("");

    const LogosMap response = module.currentGlobalState();

    assertError(response, "not_initialized");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 4);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_current_global_state"), 0);
}

LOGOS_TEST(current_global_state_maps_malformed_reads_to_account_read_failed) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockModule("lez_core", "get_account_public").returns("not-json");

    const LogosMap response = module.currentGlobalState();

    assertError(response, "account_read_failed");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 4);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_current_global_state"), 0);
}

LOGOS_TEST(current_global_state_preserves_stable_projection_errors) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string projection_response = failureEnvelope("invalid_clock");
    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_current_global_state").returns(projection_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.currentGlobalState();

    assertError(response, "invalid_clock");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 4);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_current_global_state"), 1);
}

LOGOS_TEST(redemption_rate_update_quote_reads_configured_sources_and_returns_flat_quote) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json decoded_parameters = {{"marketPriceOracleIdHex", ORACLE_ID_HEX}};
    const json quote = {
        {"canSubmit", true},
        {"code", "ready"},
        {"currentRedemptionPrice", "1000000000000000000000000000"},
        {"marketPrice", "900000000000000000000000000"},
        {"elapsedMilliseconds", "300000"},
        {"nextRedemptionRatePerMillisecond", "1000010000000000000000000000"},
        {"nextControllerIntegralTerm", "42"},
        {"clampMetadata", {
            {"integralMinimum", "-1000000000000000000000000000000000"},
            {"integralMaximum", "1000000000000000000000000000000000"},
            {"rateDeltaMinimum", "-10000000000000000000000"},
            {"rateDeltaMaximum", "10000000000000000000000"},
        }},
        {"errors", json::array()},
        {"warnings", json::array()},
    };
    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = successEnvelope(decoded_parameters);
    const std::string quote_response = successEnvelope(quote);
    context.mockCFunction("stablecoin_program_info")
        .returns(program_info_response);
    context.mockCFunction("stablecoin_decode_protocol_parameters")
        .returns(decoder_response);
    context.mockCFunction("stablecoin_redemption_rate_update_quote")
        .returns(quote_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.redemptionRateUpdateQuote();

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
    for (auto field = quote.begin(); field != quote.end(); ++field) {
        LOGOS_ASSERT_EQ(response[field.key()], field.value());
    }
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 4);
    for (const auto& account_id : {
             PROTOCOL_PARAMETERS_ID_HEX,
             REDEMPTION_STATE_ID_HEX,
             ORACLE_ID_HEX,
             CLOCK_ID_HEX,
         }) {
        LOGOS_ASSERT_TRUE(context.moduleCalledWith(
            "lez_core",
            "get_account_public",
            QVariantList{QVariant(QString::fromStdString(account_id))}));
    }
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_decode_protocol_parameters"), 1);
    LOGOS_ASSERT_EQ(
        context.cFunctionCallCount("stablecoin_redemption_rate_update_quote"), 1);
    LOGOS_ASSERT_EQ(
        context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
}

LOGOS_TEST(redemption_rate_update_quote_keeps_soft_blockers_as_success) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json decoded_parameters = {{"marketPriceOracleIdHex", ORACLE_ID_HEX}};
    const json blocker = {
        {"code", "oracle_price_zero"},
        {"recoverable", true},
        {"blockingFields", json::array()},
        {"details", {{"marketPrice", "0"}}},
    };
    const json quote = {
        {"canSubmit", false},
        {"code", "blocked"},
        {"currentRedemptionPrice", "100"},
        {"marketPrice", "0"},
        {"elapsedMilliseconds", "9"},
        {"nextRedemptionRatePerMillisecond", nullptr},
        {"nextControllerIntegralTerm", nullptr},
        {"clampMetadata", json::object()},
        {"errors", json::array({blocker})},
        {"warnings", json::array()},
    };
    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoder_response = successEnvelope(decoded_parameters);
    const std::string quote_response = successEnvelope(quote);
    context.mockCFunction("stablecoin_program_info")
        .returns(program_info_response);
    context.mockCFunction("stablecoin_decode_protocol_parameters")
        .returns(decoder_response);
    context.mockCFunction("stablecoin_redemption_rate_update_quote")
        .returns(quote_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.redemptionRateUpdateQuote();

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["canSubmit"].get<bool>(), false);
    LOGOS_ASSERT_TRUE(response["nextRedemptionRatePerMillisecond"].is_null());
    LOGOS_ASSERT_TRUE(response["nextControllerIntegralTerm"].is_null());
    LOGOS_ASSERT_EQ(
        context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
}

LOGOS_TEST(redemption_rate_update_quote_maps_missing_globals_and_hard_ffi_errors) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        context.mockCFunction("stablecoin_program_info")
            .returns(program_info_response);
        context.mockModule("lez_core", "get_account_public").returns("");

        assertError(module.redemptionRateUpdateQuote(), "not_initialized");
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
        LOGOS_ASSERT_EQ(
            context.cFunctionCallCount("stablecoin_decode_protocol_parameters"), 0);
    }

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const json decoded_parameters = {{"marketPriceOracleIdHex", ORACLE_ID_HEX}};
        const std::string program_info_response = successEnvelope(programInfoValue());
        const std::string decoder_response = successEnvelope(decoded_parameters);
        const std::string quote_response =
            failureEnvelope("market_price_oracle_mismatch");
        context.mockCFunction("stablecoin_program_info")
            .returns(program_info_response);
        context.mockCFunction("stablecoin_decode_protocol_parameters")
            .returns(decoder_response);
        context.mockCFunction("stablecoin_redemption_rate_update_quote")
            .returns(quote_response);
        context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

        assertError(module.redemptionRateUpdateQuote(), "market_price_oracle_mismatch");
        LOGOS_ASSERT_EQ(
            context.cFunctionCallCount("stablecoin_redemption_rate_update_quote"), 1);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }
}

LOGOS_TEST(poke_methods_validate_caller_wallet_ownership_before_reads) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);

        assertError(module.accrueStabilityFee("not-an-account"), "invalid_account_id");
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 0);
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(walletAccounts(OTHER_CALLER_ID_HEX)));

        assertError(module.accrueStabilityFee(CALLER_ID_HEX), "account_read_failed");
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 1);
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(QVariantList{
                QVariant(QString::fromStdString(
                    UniversalLezCore::transportErrorSentinel())),
            }));

        assertError(module.accrueStabilityFee(CALLER_ID_HEX), "backend_error");
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 1);
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }
}

LOGOS_TEST(poke_methods_submit_exact_plans_while_protocol_is_frozen) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const std::string program_info_response = successEnvelope(programInfoValue());
    const std::string decoded_parameters_response = successEnvelope({
        {"marketPriceOracleIdHex", ORACLE_ID_HEX},
        {"isFrozen", true},
    });
    const std::string accrue_plan_response =
        successEnvelope(submissionPlan(accrueAccounts(), 1));
    const std::string update_plan_response =
        successEnvelope(submissionPlan(updateAccounts(), 2));
    const std::string refresh_plan_response =
        successEnvelope(submissionPlan(refreshAccounts(), 3));
    const std::string account_response = initializedAccount();
    const std::string transaction_response = successfulTransaction();

    context.mockCFunction("stablecoin_program_info").returns(program_info_response);
    context.mockCFunction("stablecoin_decode_protocol_parameters")
        .returns(decoded_parameters_response);
    context.mockCFunction("stablecoin_accrue_stability_fee_plan")
        .returns(accrue_plan_response);
    context.mockCFunction("stablecoin_update_redemption_rate_plan")
        .returns(update_plan_response);
    context.mockCFunction("stablecoin_refresh_globals_plan")
        .returns(refresh_plan_response);
    context.mockModule("lez_core", "list_accounts")
        .returnsVariant(QVariant(walletAccounts(CALLER_ID_HEX)));
    context.mockModule("lez_core", "get_account_public").returns(account_response);
    context.mockModule("lez_core", "send_generic_public_transaction")
        .returns(transaction_response);

    const LogosMap accrued = module.accrueStabilityFee(CALLER_ID_HEX);
    const LogosMap updated = module.updateRedemptionRate(CALLER_ID_HEX);
    const LogosMap refreshed = module.refreshGlobals(CALLER_ID_HEX);

    for (const LogosMap* response : {&accrued, &updated, &refreshed}) {
        LOGOS_ASSERT_EQ((*response)["status"].get<std::string>(), std::string("ok"));
        LOGOS_ASSERT_EQ((*response)["error"].get<std::string>(), std::string());
        LOGOS_ASSERT_EQ(
            (*response)["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    }
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 3);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 12);
    LOGOS_ASSERT_EQ(
        context.moduleCallCount("lez_core", "send_generic_public_transaction"), 3);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(accrueAccounts(), 1)));
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(updateAccounts(), 2)));
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(refreshAccounts(), 3)));
}

LOGOS_TEST(update_redemption_rate_blocks_each_ordered_quote_gate_without_submit) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);

    for (const std::string& blocker : {
             std::string("oracle_stale"),
             std::string("oracle_price_zero"),
             std::string("rate_update_too_soon"),
         }) {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);

        const std::string program_info_response = successEnvelope(programInfoValue());
        const std::string decoded_parameters_response =
            successEnvelope({{"marketPriceOracleIdHex", ORACLE_ID_HEX}});
        const std::string plan_response = failureEnvelope(blocker);
        const std::string account_response = initializedAccount();
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockCFunction("stablecoin_decode_protocol_parameters")
            .returns(decoded_parameters_response);
        context.mockCFunction("stablecoin_update_redemption_rate_plan")
            .returns(plan_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(walletAccounts(CALLER_ID_HEX)));
        context.mockModule("lez_core", "get_account_public").returns(account_response);

        assertError(module.updateRedemptionRate(CALLER_ID_HEX), blocker);
        LOGOS_ASSERT_EQ(
            context.cFunctionCallCount("stablecoin_update_redemption_rate_plan"), 1);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }
}

LOGOS_TEST(poke_methods_map_missing_globals_and_oracle_mismatch) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(walletAccounts(CALLER_ID_HEX)));
        context.mockModule("lez_core", "get_account_public").returns("");

        assertError(module.accrueStabilityFee(CALLER_ID_HEX), "not_initialized");
        LOGOS_ASSERT_EQ(
            context.cFunctionCallCount("stablecoin_accrue_stability_fee_plan"), 0);
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }

    {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const std::string program_info_response = successEnvelope(programInfoValue());
        const std::string decoded_parameters_response =
            successEnvelope({{"marketPriceOracleIdHex", ORACLE_ID_HEX}});
        const std::string plan_response =
            failureEnvelope("market_price_oracle_mismatch");
        const std::string account_response = initializedAccount();
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockCFunction("stablecoin_decode_protocol_parameters")
            .returns(decoded_parameters_response);
        context.mockCFunction("stablecoin_refresh_globals_plan").returns(plan_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(walletAccounts(CALLER_ID_HEX)));
        context.mockModule("lez_core", "get_account_public").returns(account_response);

        assertError(
            module.refreshGlobals(CALLER_ID_HEX), "market_price_oracle_mismatch");
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);
    }
}

LOGOS_TEST(poke_submission_maps_wallet_rejection_and_transport_failure) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);

    for (const std::string& wallet_response : {
             json{{"success", false}, {"tx_hash", TRANSACTION_ID_HEX}}.dump(),
             UniversalLezCore::transportErrorSentinel(),
         }) {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);

        const std::string program_info_response = successEnvelope(programInfoValue());
        const std::string plan_response =
            successEnvelope(submissionPlan(accrueAccounts(), 1));
        const std::string account_response = initializedAccount();
        context.mockCFunction("stablecoin_program_info").returns(program_info_response);
        context.mockCFunction("stablecoin_accrue_stability_fee_plan")
            .returns(plan_response);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(walletAccounts(CALLER_ID_HEX)));
        context.mockModule("lez_core", "get_account_public").returns(account_response);
        context.mockModule("lez_core", "send_generic_public_transaction")
            .returns(wallet_response);

        assertError(
            module.accrueStabilityFee(CALLER_ID_HEX), "wallet_submission_failed");
        LOGOS_ASSERT_EQ(
            context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
    }
}
