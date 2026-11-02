#include "mock_support.h"

#include <limits>

using namespace amm_test;

LOGOS_TEST(exact_output_rejects_invalid_slippage_before_account_reads) {
    MockFixture f;
    for (const auto slippage : {-1LL, 10000LL, std::numeric_limits<long long>::max()}) {
        assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 10, slippage),
                    "invalid_slippage");
    }
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
    LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_swap_exact_out_quote"), 0);
}

LOGOS_TEST(oracle_setup_rejects_invalid_windows_without_planning_or_submission) {
    for (const json& value : {json(-1), json("-1"), json("+1"), json("123abc"), json(""),
                              json("18446744073709551616"), json(0), json(1.5), json(nullptr)}) {
        MockFixture f;
        auto r = request();
        r["windowDurationMs"] = value;
        assertError(f.module.createPriceObservations(r), "invalid_window");
        assertError(f.module.createOraclePriceAccount(r), "invalid_window");
        LOGOS_ASSERT_EQ(ffi::calls("amm_create_price_observations_plan"), 0);
        LOGOS_ASSERT_EQ(ffi::calls("amm_create_oracle_price_account_plan"), 0);
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    }
}
