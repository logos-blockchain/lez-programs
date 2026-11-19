#include "amm_module_impl.h"
#include "logos_sdk.h"

#include <logos_test.h>

namespace {

void mockConfigWallet(LogosModules& dependencies, AmmModuleImpl& module) {
    module._logosCoreSetLogosModulesPtr_(&dependencies);
    const std::string program(64, 'a');
    module.setAmmProgramId({{"ammProgramId", program}});
    module.setConfigId({{"configId", std::string(64, 'b')}});

    // Borsh Config tag, token and oracle program ids, authority, and two LE u128 fees.
    const std::string config = "00" + std::string(64, '1') + std::string(64, '2')
        + std::string(64, '3') + "1e" + std::string(30, '0') + std::string(32, '0');
    dependencies.lez_core.account = {
        {"nonce", "1"},
        {"actor_states", LogosList::array({{{"program", program}, {"data", config}}})},
    };
}

}  // namespace

LOGOS_TEST(config_account_accepts_wallet_decimal_nonce) {
    LogosModules dependencies;
    AmmModuleImpl module;
    mockConfigWallet(dependencies, module);

    const auto config = module.configAccount();
    LOGOS_ASSERT_EQ(config.at("status").get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(config.at("swapFeeBps").get<unsigned>(), 30u);
}

LOGOS_TEST(oracle_setup_rejects_malformed_duration_before_submission) {
    LogosModules dependencies;
    AmmModuleImpl module;
    mockConfigWallet(dependencies, module);

    LogosMap request = {{"tokenAId", std::string(64, '4')}, {"tokenBId", std::string(64, '5')},
                        {"feePayerId", std::string(64, '6')}};
    for (const char* invalid : {"3000ms", "-1"}) {
        request["windowDurationMs"] = invalid;
        const auto result = module.createOraclePriceAccount(request);
        LOGOS_ASSERT_EQ(result.at("error").get<std::string>(), std::string("invalid_window"));
    }
    LOGOS_ASSERT_EQ(dependencies.lez_core.submissions, 0);

    request["windowDurationMs"] = "2048";
    const auto valid = module.createOraclePriceAccount(request);
    LOGOS_ASSERT_EQ(valid.at("status").get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(dependencies.lez_core.submissions, 1);
}
