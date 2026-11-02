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
