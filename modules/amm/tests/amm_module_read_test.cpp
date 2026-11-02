#include "mock_support.h"

#include <algorithm>
#include <limits>

using namespace amm_test;

LOGOS_TEST(config_selection_normalizes_ids_and_restores_environment_fallback) {
    MockFixture f;
    assertOk(f.module.configAccount());
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("ammProgramId"), json(PROGRAM));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("config").at("id"), json(CONFIG));

    assertOk(f.module.setAmmProgramId({{"ammProgramId", std::string(64, 'A')}}));
    assertOk(f.module.setConfigId({{"configId", std::string(64, 'B')}}));
    f.modules.lez_core.accounts[TOKEN_B] = snapshot().dump();
    assertOk(f.module.configAccount());
    auto received = ffi::lastRequest("amm_config_account");
    LOGOS_ASSERT_EQ(received.at("ammProgramId"), json(TOKEN_A));
    LOGOS_ASSERT_EQ(received.at("config").at("id"), json(TOKEN_B));

    assertError(f.module.setAmmProgramId({{"ammProgramId", "invalid!"}}), "invalid_account_id");
    assertError(f.module.setConfigId({{"configId", "invalid!"}}), "invalid_account_id");
    assertOk(f.module.configAccount());
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("ammProgramId"), json(TOKEN_A));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("config").at("id"), json(TOKEN_B));

    assertOk(f.module.setAmmProgramId({{"ammProgramId", ""}}));
    assertOk(f.module.setConfigId({{"configId", ""}}));
    assertOk(f.module.configAccount());
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("ammProgramId"), json(PROGRAM));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account").at("config").at("id"), json(CONFIG));
    LOGOS_ASSERT_EQ(ffi::calls("amm_config_account"), ffi::frees());
}

LOGOS_TEST(config_and_pool_read_failures_keep_distinct_errors) {
    MockFixture f;
    f.modules.lez_core.accounts[CONFIG] = "not-json";
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "config_read_failed");
    LOGOS_ASSERT_EQ(ffi::calls("amm_swap_pair"), 0);
    ffi::success("amm_config_account", {{"status", "error"}, {"error", "config_unavailable"}});
    assertError(f.module.configAccount(), "config_unavailable");
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account")["config"]["status"],
                    json("read_failed"));
    f.modules.lez_core.accounts[CONFIG] = snapshot(configData()).dump();
    ffi::success("amm_swap_pair", {{"status", "error"}, {"code", "config_unavailable"}});
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "amm_not_initialized");
    ffi::success("amm_swap_pair", {{"status", "error"}, {"code", "same_token_pair"}});
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_A), "same_token_pair");
    ScopedEnv no_program("AMM_PROGRAM_ID", nullptr);
    assertError(f.module.configAccount(), "config_missing");
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "no_program_bin");
}

LOGOS_TEST(missing_configuration_returns_stable_errors_without_wallet_reads) {
    MockFixture f;
    {
        ScopedEnv no_program("AMM_PROGRAM_ID", nullptr);
        assertError(f.module.configAccount(), "config_missing");
        assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 100, 50), "config_missing");
        assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 100, 50), "config_missing");
        assertError(f.module.addLiquidityQuote(request()), "config_missing");
        assertError(f.module.removeLiquidityQuote(request()), "config_missing");
        LOGOS_ASSERT_TRUE(f.module.tokenHoldings(true).empty());
        LOGOS_ASSERT_TRUE(f.module.resolveTokens({{"tokenIds", {TOKEN_A}}}, true).empty());
    }
    {
        ScopedEnv no_config("AMM_CONFIG_ID", nullptr);
        assertError(f.module.configAccount(), "backend_error");
        assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "bad_config");
        assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 100, 50), "config_missing");
        assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 100, 50), "config_missing");
        assertError(f.module.addLiquidityQuote(request()), "config_missing");
        assertError(f.module.removeLiquidityQuote(request()), "config_missing");
        LOGOS_ASSERT_TRUE(f.module.tokenHoldings(true).empty());
        LOGOS_ASSERT_TRUE(f.module.resolveTokens({{"tokenIds", {TOKEN_A}}}, true).empty());
    }
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
    LOGOS_ASSERT_EQ(f.modules.lez_core.inventory_reads, 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 0);
}

LOGOS_TEST(malformed_account_fields_are_not_forwarded_as_valid_reads) {
    MockFixture f;
    for (const auto& field : {"program_owner", "balance", "nonce", "data"}) {
        auto malformed = snapshot(configData());
        malformed[field] = std::string(field) == "data" ? "0" : "not-hex";
        f.modules.lez_core.accounts[CONFIG] = malformed.dump();
        assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "config_read_failed");
        ffi::success("amm_config_account", {{"status", "error"}, {"error", "config_unavailable"}});
        assertError(f.module.configAccount(), "config_unavailable");
        LOGOS_ASSERT_EQ(ffi::lastRequest("amm_config_account")["config"]["status"],
                        json("read_failed"));
    }
    LOGOS_ASSERT_EQ(ffi::calls("amm_swap_pair"), 0);
}

LOGOS_TEST(pool_resolution_orients_stored_fields_to_requested_token_order) {
    MockFixture f;
    ffi::success("amm_resolve_pool", {{"status", "ok"},
                                      {"error", ""},
                                      {"defAHex", TOKEN_B},
                                      {"defBHex", TOKEN_A},
                                      {"reserveA", "22"},
                                      {"reserveB", "11"},
                                      {"vaultAId", "vault-b"},
                                      {"vaultBId", "vault-a"},
                                      {"poolId", POOL}});
    const auto pool = f.module.resolvePoolAccount("token-a-base58", "token-b-base58");
    assertOk(pool);
    LOGOS_ASSERT_EQ(pool.at("defAHex"), json(TOKEN_A));
    LOGOS_ASSERT_EQ(pool.at("reserveA"), json("11"));
    LOGOS_ASSERT_EQ(pool.at("reserveB"), json("22"));
    LOGOS_ASSERT_EQ(pool.at("vaultAId"), json("vault-a"));
    LOGOS_ASSERT_EQ(pool.at("vaultBId"), json("vault-b"));
    const auto pair = ffi::lastRequest("amm_swap_pair");
    LOGOS_ASSERT_EQ(pair.at("tokenInId"), json(TOKEN_A));
    LOGOS_ASSERT_EQ(pair.at("tokenOutId"), json(TOKEN_B));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_resolve_pool")["pool"]["account"]["data"],
                    json(poolData()));

    ffi::success("amm_resolve_pool", {{"status", "error"}, {"error", "no_pool"}, {"poolId", POOL}});
    const auto missing = f.module.resolvePoolAccount(TOKEN_A, TOKEN_B);
    assertError(missing, "no_pool");
    LOGOS_ASSERT_EQ(missing.at("poolId"), json(POOL));
}

LOGOS_TEST(wallet_holdings_and_token_resolution_filter_and_deduplicate_reads) {
    MockFixture f;
    f.modules.lez_core.inventory = json::array({{{"account_id", HOLDING_A}, {"is_public", true}},
                                                {{"account_id", HOLDING_B}, {"is_public", false}}});
    f.modules.lez_core.accounts[HOLDING_A] = snapshot("0102").dump();
    f.modules.lez_core.accounts[TOKEN_A] = snapshot("aaaa").dump();
    f.modules.lez_core.accounts[TOKEN_B] = snapshot("bbbb").dump();
    ffi::success("amm_token_holdings", {{"holdings", json::array({{{"accountId", HOLDING_A}}})}});
    ffi::success("amm_resolve_tokens", {{"tokens", json::array({{{"definitionIdHex", TOKEN_A}}})}});
    ffi::success("amm_fee_tiers", {{"feeTiers", {1, 5, 30, 100}}});

    LOGOS_ASSERT_EQ(f.module.feeTiers(), json::array({1, 5, 30, 100}));
    LOGOS_ASSERT_EQ(f.modules.lez_core.reads.size(), 0U);
    LOGOS_ASSERT_EQ(f.module.tokenHoldings(true).size(), 1U);
    auto holdings = ffi::lastRequest("amm_token_holdings")["walletAccounts"];
    LOGOS_ASSERT_EQ(holdings.size(), 1U);
    LOGOS_ASSERT_EQ(holdings[0]["id"], json(HOLDING_A));
    LOGOS_ASSERT_TRUE(std::find(f.modules.lez_core.reads.begin(), f.modules.lez_core.reads.end(),
                                HOLDING_B) == f.modules.lez_core.reads.end());
    f.module.tokenHoldings(false);
    LOGOS_ASSERT_TRUE(ffi::lastRequest("amm_token_holdings")["walletAccounts"].empty());
    LOGOS_ASSERT_EQ(f.modules.lez_core.inventory_reads, 1);

    const auto rows = f.module.resolveTokens(
        {{"tokenIds", json::array({"token-a-base58", TOKEN_A, TOKEN_B, 12, "invalid!"})}}, false);
    LOGOS_ASSERT_EQ(rows.size(), 1U);
    const auto resolved = ffi::lastRequest("amm_resolve_tokens");
    LOGOS_ASSERT_EQ(resolved["tokenIds"], json::array({TOKEN_A, TOKEN_B}));
    LOGOS_ASSERT_EQ(resolved["tokenDefinitions"].size(), 2U);
    LOGOS_ASSERT_TRUE(resolved["walletAccounts"].empty());
    LOGOS_ASSERT_EQ(f.modules.lez_core.inventory_reads, 1);
    ffi::failure("amm_fee_tiers", "backend_error");
    LOGOS_ASSERT_TRUE(f.module.feeTiers().empty());
}

LOGOS_TEST(quotes_forward_exact_amounts_and_pool_data) {
    MockFixture f;
    const std::string huge = "340282366920938463463374607431768211455";
    ffi::success("amm_create_pool_quote", {{"expectedLp", "50"}});
    ffi::success("amm_swap_exact_in_quote", {{"expectedOut", "20"}});
    ffi::success("amm_swap_exact_out_quote", {{"requiredIn", "30"}});
    ffi::success("amm_add_liquidity_quote", {{"expectedLp", "40"}});
    ffi::success("amm_remove_liquidity_quote", {{"amountA", "10"}});
    auto r = request();
    r["amountA"] = " \"" + huge + "\" ";
    assertOk(f.module.createPoolQuote(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_create_pool_quote")["amountA"], json(huge));
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
    assertOk(f.module.swapExactInQuote("token-a-base58", TOKEN_B, huge, 50));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_swap_exact_in_quote")["amountIn"], json(huge));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_swap_exact_in_quote")["poolData"], json(poolData()));
    assertOk(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 100, 50));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_swap_exact_out_quote")["amountOut"], json("100"));
    r["maxAmountA"] = huge;
    r["slippageBps"] = 75;
    assertOk(f.module.addLiquidityQuote(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_add_liquidity_quote")["maxAmountA"], json(huge));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_add_liquidity_quote")["slippageBps"], json(75));
    assertOk(f.module.removeLiquidityQuote(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_remove_liquidity_quote")["lpAmount"], json("20"));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_remove_liquidity_quote")["slippageBps"], json(75));
}

LOGOS_TEST(quote_pool_id_failures_do_not_call_pricers) {
    MockFixture f;
    f.modules.lez_core.accounts[CONFIG] = "";
    ffi::failure("amm_pool_id", "account read failed");
    assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 100, 50), "account read failed");
    assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 100, 50), "account read failed");
    assertError(f.module.addLiquidityQuote(request()), "account read failed");
    assertError(f.module.removeLiquidityQuote(request()), "account read failed");
    LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 4);
    LOGOS_ASSERT_EQ(ffi::calls("amm_swap_exact_in_quote"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_swap_exact_out_quote"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_add_liquidity_quote"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_remove_liquidity_quote"), 0);
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(unsigned_json_amount_is_forwarded_without_loss) {
    MockFixture f;
    ffi::success("amm_create_pool_quote", {{"expectedLp", "50"}});
    auto r = request();
    r["amountA"] = std::numeric_limits<std::uint64_t>::max();
    assertOk(f.module.createPoolQuote(r));
    LOGOS_ASSERT_EQ(ffi::lastRequest("amm_create_pool_quote")["amountA"],
                    json("18446744073709551615"));
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
}

LOGOS_TEST(quote_validation_and_domain_errors_are_stable) {
    MockFixture f;
    for (const json& amount : {json(-1), json(1.5), json("12e3"), json(nullptr), json("-")}) {
        assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, amount, 1), "bad_amount");
        assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, amount, 1), "bad_amount");
    }
    for (const auto slippage : {-1LL, 10000LL, std::numeric_limits<long long>::max()}) {
        assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10, slippage), "invalid_slippage");
        assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 10, slippage), "invalid_slippage");
    }
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
    LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 0);
    auto r = request();
    r["amountA"] = 1.5;
    assertError(f.module.createPoolQuote(r), "bad_amount");
    r = request();
    r["slippageBps"] = 1.5;
    assertError(f.module.addLiquidityQuote(r), "invalid_slippage");
    assertError(f.module.removeLiquidityQuote(r), "invalid_slippage");
    LOGOS_ASSERT_EQ(ffi::calls("amm_add_liquidity_quote"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_remove_liquidity_quote"), 0);
    ffi::failure("amm_swap_exact_in_quote", "no_pool");
    assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10, 1), "no_pool");
    ffi::failure("amm_create_pool_quote", "amount_too_low");
    assertError(f.module.createPoolQuote(request()), "amount_too_low");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}
