#include "test_support.h"

using namespace amm_test;

LOGOS_TEST(config_account_accepts_wallet_decimal_nonce) {
    Fixture fixture;

    const auto config = fixture.module.configAccount();
    LOGOS_ASSERT_EQ(config.at("status").get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(config.at("swapFeeBps").get<unsigned>(), 30u);
}

LOGOS_TEST(oracle_setup_rejects_malformed_duration_before_submission) {
    Fixture fixture;

    LogosMap request = {{"tokenAId", TOKEN_A}, {"tokenBId", TOKEN_B},
                        {"feePayerId", PAYER}};
    for (const char* invalid : {"3000ms", "-1"}) {
        request["windowDurationMs"] = invalid;
        const auto result = fixture.module.createOraclePriceAccount(request);
        LOGOS_ASSERT_EQ(result.at("error").get<std::string>(), std::string("invalid_window"));
    }
    LOGOS_ASSERT_EQ(fixture.modules.lez_core.submissions.size(), 0u);

    request["windowDurationMs"] = "2048";
    const auto valid = fixture.module.createOraclePriceAccount(request);
    LOGOS_ASSERT_EQ(valid.at("status").get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(fixture.modules.lez_core.submissions.size(), 1u);
}
