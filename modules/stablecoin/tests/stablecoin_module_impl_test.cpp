#include "stablecoin_module_impl.h"

#include <cstdlib>
#include <optional>
#include <string>
#include <utility>

#include <QVariant>
#include <QVariantList>
#include <logos_test.h>
#include <nlohmann/json.hpp>

#include "logos_sdk.h"

namespace {

using json = nlohmann::json;

const std::string PROGRAM_ID_HEX(64, '1');
const std::string ACCUMULATOR_ID_HEX(64, '2');
const std::string PROTOCOL_PARAMETERS_ID_HEX(64, '3');
const std::string REDEMPTION_STATE_ID_HEX(64, '4');
const std::string CLOCK_ID_HEX(64, '7');

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
