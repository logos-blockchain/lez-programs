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
        {"protocolParametersIdHex", std::string(64, '3')},
        {"stabilityFeeAccumulatorId", "stability-fee-accumulator-id"},
        {"stabilityFeeAccumulatorIdHex", ACCUMULATOR_ID_HEX},
        {"redemptionPriceStateId", "redemption-price-state-id"},
        {"redemptionPriceStateIdHex", std::string(64, '4')},
        {"stablecoinDefinitionId", "stablecoin-definition-id"},
        {"stablecoinDefinitionIdHex", std::string(64, '5')},
        {"stablecoinMasterHoldingId", "stablecoin-master-holding-id"},
        {"stablecoinMasterHoldingIdHex", std::string(64, '6')},
        {"clockId", "clock-id"},
        {"clockIdHex", std::string(64, '7')},
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
