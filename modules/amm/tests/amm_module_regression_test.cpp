#include "mock_support.h"

#include <cstdint>
#include <initializer_list>
#include <limits>
#include <string>
#include <utility>
#include <vector>

using namespace amm_test;
namespace {
std::vector<std::uint8_t> encoded(std::uint8_t tag,
                                  std::initializer_list<std::pair<std::uint64_t, int>> fields) {
    std::vector<std::uint8_t> bytes{tag};
    for (auto [value, size] : fields) {
        for (int i = 0; i < size; ++i) {
            bytes.push_back(static_cast<std::uint8_t>(value & 0xff));
            value >>= 8;
        }
    }
    return bytes;
}
}  // namespace

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

LOGOS_TEST(oracle_setup_forwards_full_width_windows_and_refuses_existing_targets) {
    MockFixture f;
    const auto p = plan({CONFIG, POOL, OTHER, LP, TOKEN_A}, {false, false, false, false, false},
                        encoded(2, {{std::numeric_limits<std::uint64_t>::max(), 8}}));
    ffi::success("amm_create_price_observations_plan", p);
    auto r = request();
    r["windowDurationMs"] = "18446744073709551615";
    assertOk(f.module.createPriceObservations(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_create_price_observations_plan")["windowDurationMs"],
                    json(std::numeric_limits<std::uint64_t>::max()));
    f.modules.lez_core.accounts[LP] = snapshot().dump();
    assertError(f.module.createPriceObservations(r), "already_exists");
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
}

LOGOS_TEST(oracle_setup_accepts_numeric_windows_and_checks_price_account_target) {
    MockFixture f;
    const auto p =
        plan({CONFIG, POOL, OTHER, TOKEN_A}, {false, false, false, false}, encoded(3, {{2048, 8}}));
    ffi::success("amm_create_oracle_price_account_plan", p);
    auto r = request();
    r["windowDurationMs"] = std::uint64_t(2048);
    assertOk(f.module.createOraclePriceAccount(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_create_oracle_price_account_plan")["windowDurationMs"],
                    json(2048));
    assertSubmission(f.modules.lez_core.submissions.back(), p);
    f.modules.lez_core.accounts[OTHER] = snapshot().dump();
    assertError(f.module.createOraclePriceAccount(r), "already_exists");
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
}

LOGOS_TEST(foreign_owned_pool_data_is_not_forwarded_for_quotes_or_plans) {
    MockFixture f;
    f.modules.lez_core.accounts[POOL] = snapshot(poolData(), OTHER).dump();
    for (const auto* name : {"amm_swap_exact_in_quote", "amm_swap_exact_out_quote",
                             "amm_add_liquidity_quote", "amm_remove_liquidity_quote",
                             "amm_swap_exact_in_plan", "amm_swap_exact_out_plan",
                             "amm_add_liquidity_plan", "amm_remove_liquidity_plan",
                             "amm_sync_reserves_plan"})
        ffi::failure(name, "no_pool");
    assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10, 1), "no_pool");
    assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 10, 1), "no_pool");
    assertError(f.module.addLiquidityQuote(request()), "no_pool");
    assertError(f.module.removeLiquidityQuote(request()), "no_pool");
    LOGOS_ASSERT_TRUE(f.module.swapExactInput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B,
                                             10, 1, 1000, PAYER).empty());
    LOGOS_ASSERT_TRUE(f.module.swapExactOutput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B,
                                              10, 20, 1000, PAYER).empty());
    assertError(f.module.addLiquidity(request()), "no_pool");
    assertError(f.module.removeLiquidity(request()), "no_pool");
    assertError(f.module.syncReserves(request()), "no_pool");
    for (const auto* name : {"amm_swap_exact_in_quote", "amm_swap_exact_out_quote",
                             "amm_add_liquidity_quote", "amm_remove_liquidity_quote",
                             "amm_swap_exact_in_plan", "amm_swap_exact_out_plan",
                             "amm_add_liquidity_plan", "amm_remove_liquidity_plan",
                             "amm_sync_reserves_plan"}) {
        LOGOS_ASSERT_EQ(ffi::calls(name), 1);
        LOGOS_ASSERT_EQ(ffi::lastRequest(name).at("poolData"), json(""));
    }
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}
