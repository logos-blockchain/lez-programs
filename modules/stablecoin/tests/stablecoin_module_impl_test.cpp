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
const std::string OWNER_ID_HEX(64, '2');
const std::string POSITION_ID_HEX(64, '3');
const std::string VAULT_ID_HEX(64, '4');
const std::string MAX_U64 = "18446744073709551615";
const std::string MAX_U128 = "340282366920938463463374607431768211455";

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
        {"protocolParametersIdHex", std::string(64, '5')},
        {"stabilityFeeAccumulatorId", "stability-fee-accumulator-id"},
        {"stabilityFeeAccumulatorIdHex", std::string(64, '6')},
        {"redemptionPriceStateId", "redemption-price-state-id"},
        {"redemptionPriceStateIdHex", std::string(64, '7')},
        {"stablecoinDefinitionId", "stablecoin-definition-id"},
        {"stablecoinDefinitionIdHex", std::string(64, '8')},
        {"stablecoinMasterHoldingId", "stablecoin-master-holding-id"},
        {"stablecoinMasterHoldingIdHex", std::string(64, '9')},
        {"clockId", "clock-id"},
        {"clockIdHex", std::string(64, 'a')},
    };
}

json positionIdentityValue() {
    return {
        {"ownerId", "owner-id"},
        {"ownerIdHex", OWNER_ID_HEX},
        {"positionNonce", MAX_U64},
        {"positionId", "position-id"},
        {"positionIdHex", POSITION_ID_HEX},
        {"vaultId", "vault-id"},
        {"vaultIdHex", VAULT_ID_HEX},
    };
}

json decodedPositionValue() {
    json value = positionIdentityValue();
    value["collateralAmount"] = MAX_U128;
    value["normalizedDebtAmount"] = MAX_U128;
    value["openedAt"] = MAX_U64;
    return value;
}

std::string successEnvelope(const json& value) {
    return json{{"ok", true}, {"value", value}}.dump();
}

std::string failureEnvelope(const std::string& error) {
    return json{{"ok", false}, {"error", error}}.dump();
}

const std::string PROGRAM_INFO_RESPONSE = successEnvelope(programInfoValue());
const std::string POSITION_INFO_RESPONSE = successEnvelope(positionIdentityValue());
const std::string DECODED_POSITION_RESPONSE = successEnvelope(decodedPositionValue());
const std::string INVALID_NONCE_RESPONSE = failureEnvelope("invalid_numeric_value");
const std::string VAULT_MISMATCH_RESPONSE = failureEnvelope("position_vault_mismatch");

std::string initializedAccount() {
    return json{
        {"program_owner", PROGRAM_ID_HEX},
        {"balance", std::string(32, '0')},
        {"nonce", std::string(32, '0')},
        {"data", "00"},
    }.dump();
}

std::string missingAccount() {
    return json{
        {"program_owner", std::string(64, '0')},
        {"balance", std::string(32, '0')},
        {"nonce", std::string(32, '0')},
        {"data", ""},
    }.dump();
}

void attachModules(StablecoinModuleImpl& module, LogosModules& modules) {
    module._logosCoreSetLogosModulesPtr_(&modules);
}

void configureProgramInfo(LogosTestContext& context) {
    context.mockCFunction("stablecoin_program_info").returns(PROGRAM_INFO_RESPONSE);
}

void assertError(const LogosMap& response, const std::string& error) {
    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("error"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), error);
}

}  // namespace

LOGOS_TEST(position_account_reads_once_and_returns_exact_snapshot) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json identity = positionIdentityValue();
    const json decoded = decodedPositionValue();
    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(POSITION_INFO_RESPONSE);
    context.mockCFunction("stablecoin_decode_position").returns(DECODED_POSITION_RESPONSE);
    context.mockModule("lez_core", "account_id_from_base58").returns(OWNER_ID_HEX);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.positionAccount({
        {"ownerId", "owner-base58"},
        {"positionNonce", MAX_U64},
    });

    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
    LOGOS_ASSERT_EQ(response["position"], decoded);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "get_account_public",
        QVariantList{QVariant(QString::fromStdString(POSITION_ID_HEX))}));
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_position_info"), 1);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_position"), 1);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "account_id_from_base58"), 1);
}

LOGOS_TEST(position_account_returns_derived_ids_when_position_is_absent) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const json identity = positionIdentityValue();
    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(POSITION_INFO_RESPONSE);
    context.mockModule("lez_core", "get_account_public").returns(missingAccount());

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", MAX_U64},
    });

    assertError(response, "not_found");
    LOGOS_ASSERT_EQ(response["position"], identity);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_position"), 0);
}

LOGOS_TEST(position_account_rejects_malformed_wallet_response_without_decoding) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(POSITION_INFO_RESPONSE);
    context.mockModule("lez_core", "get_account_public").returns("not-json");

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", MAX_U64},
    });

    assertError(response, "account_read_failed");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 1);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_position"), 0);
}

LOGOS_TEST(position_account_preserves_stable_decoder_errors) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(POSITION_INFO_RESPONSE);
    context.mockCFunction("stablecoin_decode_position").returns(VAULT_MISMATCH_RESPONSE);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", MAX_U64},
    });

    assertError(response, "position_vault_mismatch");
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_position"), 1);
}

LOGOS_TEST(position_account_rejects_non_string_request_fields_before_io) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", 1},
    });

    assertError(response, "bad_request");
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_program_info"), 0);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_position_info"), 0);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
}

LOGOS_TEST(position_account_rejects_non_decimal_nonce_before_account_io) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    configureProgramInfo(context);

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", "1e3"},
    });

    assertError(response, "invalid_numeric_value");
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_program_info"), 1);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_position_info"), 0);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
}

LOGOS_TEST(position_account_propagates_invalid_nonce_without_reading) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(INVALID_NONCE_RESPONSE);

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", "18446744073709551616"},
    });

    assertError(response, "invalid_numeric_value");
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 0);
    LOGOS_ASSERT_EQ(context.cFunctionCallCount("stablecoin_decode_position"), 0);
}

LOGOS_TEST(position_account_rejects_inconsistent_decoder_identity) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    json decoded = decodedPositionValue();
    decoded["vaultIdHex"] = std::string(64, 'b');
    const std::string decoded_response = successEnvelope(decoded);
    configureProgramInfo(context);
    context.mockCFunction("stablecoin_position_info").returns(POSITION_INFO_RESPONSE);
    context.mockCFunction("stablecoin_decode_position").returns(decoded_response);
    context.mockModule("lez_core", "get_account_public").returns(initializedAccount());

    const LogosMap response = module.positionAccount({
        {"ownerId", OWNER_ID_HEX},
        {"positionNonce", MAX_U64},
    });

    assertError(response, "backend_error");
}
