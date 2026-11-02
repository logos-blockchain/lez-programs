#pragma once

#include "amm_module_impl.h"
#include "logos_sdk.h"

#include <cstdlib>
#include <optional>
#include <string>
#include <utility>

#include <logos_test.h>
#include <nlohmann/json.hpp>

namespace amm_test {
using json = nlohmann::json;

inline const std::string PROGRAM(64, '1');
inline const std::string CONFIG(64, '2');
inline const std::string TOKEN_A(64, 'a');
inline const std::string TOKEN_B(64, 'b');
inline const std::string POOL = "fd2ff15a495e881c889a9e4e50dd5937bc751eb99ae40a80abb1388b9324eb58";
inline const std::string HOLDING_A(64, '4');
inline const std::string HOLDING_B(64, '5');
inline const std::string LP(64, '6');
inline const std::string PAYER(64, '7');
inline const std::string SIGNER(64, '8');
inline const std::string OTHER(64, '9');
inline constexpr const char* TX = "test-transaction";

class ScopedEnv {
   public:
    ScopedEnv(const char* key, const char* value) : key_(key) {
        if (const char* old = std::getenv(key)) old_ = old;
        if (value)
            setenv(key, value, 1);
        else
            unsetenv(key);
    }
    ~ScopedEnv() {
        if (old_)
            setenv(key_.c_str(), old_->c_str(), 1);
        else
            unsetenv(key_.c_str());
    }
    ScopedEnv(const ScopedEnv&) = delete;
    ScopedEnv& operator=(const ScopedEnv&) = delete;

   private:
    std::string key_;
    std::optional<std::string> old_;
};

inline json snapshot(const std::string& data = "abcd", const std::string& owner = PROGRAM) {
    return {{"program_owner", owner},
            {"balance", std::string(32, '0')},
            {"nonce", std::string(32, '0')},
            {"data", data}};
}

inline std::string littleHex(std::uint64_t value, int bytes) {
    constexpr char digits[] = "0123456789abcdef";
    std::string result;
    for (int i = 0; i < bytes; ++i) {
        const auto byte = static_cast<unsigned char>(value & 0xff);
        result += digits[byte >> 4];
        result += digits[byte & 15];
        value >>= 8;
    }
    return result;
}

// Borsh-encoded account data, with the same fields used by the real FFI fixture.
inline std::string configData() {
    return HOLDING_A + HOLDING_B + SIGNER + littleHex(30, 16) + littleHex(2000, 16);
}
inline std::string poolData() {
    return TOKEN_A + TOKEN_B + std::string(64, 'c') + std::string(64, 'd') + std::string(64, '0') +
           littleHex(1000000, 16) + littleHex(1000000, 16) + littleHex(2000000, 16);
}

struct Fixture {
    ScopedEnv program_env{"AMM_PROGRAM_ID", PROGRAM.c_str()};
    ScopedEnv config_env{"AMM_CONFIG_ID", CONFIG.c_str()};
    ScopedEnv binary_env{"AMM_PROGRAM_BIN", nullptr};
    LogosModules modules;
    AmmModuleImpl module;

    Fixture() {
        module._logosCoreSetLogosModulesPtr_(&modules);
        modules.lez_core.accounts[CONFIG] = snapshot(configData()).dump();
        modules.lez_core.accounts[POOL] = snapshot(poolData()).dump();
        modules.lez_core.base58["token-a-base58"] = TOKEN_A;
        modules.lez_core.base58["token-b-base58"] = TOKEN_B;
        modules.lez_core.base58["payer-base58"] = PAYER;
        modules.lez_core.base58["signer-base58"] = SIGNER;
        modules.lez_core.base58["config-base58"] = CONFIG;
    }
};

inline json request() {
    return {
        {"tokenAId", TOKEN_A},        {"tokenBId", TOKEN_B},     {"holdingAId", HOLDING_A},
        {"holdingBId", HOLDING_B},    {"lpHoldingId", LP},       {"feePayerId", PAYER},
        {"amountA", "4000000"},       {"amountB", "9000000"},    {"maxAmountA", "4000000"},
        {"maxAmountB", "9000000"},    {"minLp", "10"},           {"lpAmount", "20"},
        {"minAmountA", "1"},          {"minAmountB", "2"},       {"deadlineMs", "1700000000000"},
        {"windowDurationMs", "2048"}, {"newAuthorityId", OTHER}, {"tokenDefinitionId", TOKEN_A},
        {"destinationId", HOLDING_A}, {"amount", "42"}};
}

inline void assertError(const json& result, const char* error) {
    LOGOS_ASSERT_EQ(result.at("status"), json("error"));
    LOGOS_ASSERT_EQ(result.at("error"), json(error));
}

inline void assertOk(const json& result) {
    LOGOS_ASSERT_EQ(result.at("status"), json("ok"));
    LOGOS_ASSERT_EQ(result.at("error"), json(""));
}
}  // namespace amm_test
