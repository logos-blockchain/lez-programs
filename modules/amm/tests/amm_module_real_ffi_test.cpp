#include "test_support.h"

#include <cstdint>
#include <vector>

using namespace amm_test;
namespace {
const std::string REAL_POOL = POOL;
const std::string VAULT_A(64, 'd');
const std::string VAULT_B(64, 'e');
const std::string LP_DEFINITION(64, 'f');
const std::string TICK = "bc326ed8c2dcd95b939dca07bd24efd0024d3140d9253ceaadac2de94d6c3b6b";
const std::string CLOCK = "2f4c455a2f436c6f636b50726f6772616d4163636f756e742f30303030303031";

void appendLittle(std::vector<std::uint8_t>& out, std::uint64_t value, int bytes) {
    for (int i = 0; i < bytes; ++i) {
        out.push_back(static_cast<std::uint8_t>(value & 0xff));
        value >>= 8;
    }
}
struct RealFixture : Fixture {
    RealFixture() {
        // Borsh AmmConfig and PoolDefinition fixtures matching the Rust core layouts.
        const std::string config =
            HOLDING_A + HOLDING_B + SIGNER + littleHex(30, 16) + littleHex(2000, 16);
        const std::string pool = TOKEN_A + TOKEN_B + VAULT_A + VAULT_B + LP_DEFINITION +
                                 littleHex(1000000, 16) + littleHex(1000000, 16) +
                                 littleHex(2000000, 16);
        modules.lez_core.accounts[CONFIG] = snapshot(config).dump();
        modules.lez_core.accounts[REAL_POOL] = snapshot(pool).dump();
    }
};
}  // namespace

LOGOS_TEST(real_ffi_prices_creation_and_enumerates_supported_fees_without_chain_reads) {
    RealFixture f;
    LOGOS_ASSERT_EQ(f.module.feeTiers(), json::array({1, 5, 30, 100}));
    const auto quote = f.module.createPoolQuote(request());
    assertOk(quote);
    LOGOS_ASSERT_EQ(quote.at("actualAmountA"), json("4000000"));
    LOGOS_ASSERT_EQ(quote.at("actualAmountB"), json("9000000"));
    LOGOS_ASSERT_EQ(quote.at("expectedLp"), json("5999000"));
    LOGOS_ASSERT_EQ(quote.at("lockedLp"), json("1000"));
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
    auto same = request();
    same["tokenBId"] = TOKEN_A;
    assertError(f.module.createPoolQuote(same), "same_token_pair");
}

LOGOS_TEST(real_ffi_price_only_pool_quote_requires_no_accounts) {
    RealFixture f;
    json r = {{"tokenAId", TOKEN_A},
              {"tokenBId", TOKEN_B},
              {"price", "18446744073709551616"}};  // 1.0 in Q64.64.
    const auto quote = f.module.createPoolQuote(r);
    assertOk(quote);
    LOGOS_ASSERT_EQ(quote.at("actualAmountA"), json("1001"));
    LOGOS_ASSERT_EQ(quote.at("actualAmountB"), json("1001"));
    LOGOS_ASSERT_EQ(quote.at("minimumAmountA"), json("1001"));
    LOGOS_ASSERT_EQ(quote.at("minimumAmountB"), json("1001"));
    LOGOS_ASSERT_EQ(quote.at("expectedLp"), json("1"));
    LOGOS_ASSERT_EQ(quote.at("lockedLp"), json("1000"));
    r.erase("price");
    assertError(f.module.createPoolQuote(r), "amount_required");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
}

LOGOS_TEST(real_ffi_decodes_config_and_orients_pool_and_swap_quote) {
    RealFixture f;
    const auto config = f.module.configAccount();
    assertOk(config);
    LOGOS_ASSERT_EQ(config.at("swapFeeBps"), json(30));
    LOGOS_ASSERT_EQ(config.at("protocolFeeBps"), json(2000));
    const auto forward = f.module.resolvePoolAccount(TOKEN_A, TOKEN_B);
    assertOk(forward);
    LOGOS_ASSERT_EQ(forward.at("reserveA"), json("1000000"));
    LOGOS_ASSERT_EQ(forward.at("reserveB"), json("2000000"));
    LOGOS_ASSERT_EQ(forward.at("feeBps"), json(30));
    const auto reverse = f.module.resolvePoolAccount(TOKEN_B, TOKEN_A);
    assertOk(reverse);
    LOGOS_ASSERT_EQ(reverse.at("defAHex"), json(TOKEN_B));
    LOGOS_ASSERT_EQ(reverse.at("reserveA"), json("2000000"));
    LOGOS_ASSERT_EQ(reverse.at("reserveB"), json("1000000"));
    LOGOS_ASSERT_EQ(reverse.at("vaultAId"), forward.at("vaultBId"));
    const auto quote = f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10000, 50);
    assertOk(quote);
    LOGOS_ASSERT_EQ(quote.at("expectedOut"), json("19743"));
    LOGOS_ASSERT_EQ(quote.at("minReceived"), json("19644"));
    f.modules.lez_core.accounts.erase(REAL_POOL);
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "no_pool");
    assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10000, 50), "no_pool");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_quotes_exact_output_and_liquidity_in_both_token_orders) {
    RealFixture f;
    const auto swap = f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 10000, 50);
    assertOk(swap);
    LOGOS_ASSERT_EQ(swap.at("requiredIn"), json("5042"));
    LOGOS_ASSERT_EQ(swap.at("maxIn"), json("5068"));
    assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 2000000, 50),
                "output_exceeds_liquidity");

    auto r = request();
    r["maxAmountA"] = "10000";
    r["maxAmountB"] = "30000";
    r["lpAmount"] = "10000";
    r["slippageBps"] = 50;
    auto add = f.module.addLiquidityQuote(r);
    auto remove = f.module.removeLiquidityQuote(r);
    assertOk(add);
    assertOk(remove);
    LOGOS_ASSERT_EQ(add.at("amountA"), json("10000"));
    LOGOS_ASSERT_EQ(add.at("amountB"), json("20000"));
    LOGOS_ASSERT_EQ(add.at("expectedLp"), json("10000"));
    LOGOS_ASSERT_EQ(add.at("minimumLp"), json("9950"));
    LOGOS_ASSERT_EQ(remove.at("amountA"), json("10000"));
    LOGOS_ASSERT_EQ(remove.at("amountB"), json("20000"));
    LOGOS_ASSERT_EQ(remove.at("minimumAmountA"), json("9950"));
    LOGOS_ASSERT_EQ(remove.at("minimumAmountB"), json("19900"));

    r["tokenAId"] = TOKEN_B;
    r["tokenBId"] = TOKEN_A;
    r["maxAmountA"] = "30000";
    r["maxAmountB"] = "10000";
    add = f.module.addLiquidityQuote(r);
    remove = f.module.removeLiquidityQuote(r);
    assertOk(add);
    assertOk(remove);
    LOGOS_ASSERT_EQ(add.at("amountA"), json("20000"));
    LOGOS_ASSERT_EQ(add.at("amountB"), json("10000"));
    LOGOS_ASSERT_EQ(remove.at("minimumAmountA"), json("19900"));
    LOGOS_ASSERT_EQ(remove.at("minimumAmountB"), json("9950"));
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_create_plan_preserves_canonical_amounts_signers_and_bytes) {
    RealFixture f;
    auto r = request();
    r["deadlineMs"] = "1000";
    assertError(f.module.createPool(r), "already_exists");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    f.modules.lez_core.accounts.erase(REAL_POOL);
    const auto result = f.module.createPool(r);
    assertOk(result);
    LOGOS_ASSERT_EQ(result.at("transactionId"), json(TX));
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
    const auto& tx = f.modules.lez_core.submissions.front();
    const std::vector<std::string> accounts = {
        CONFIG,
        REAL_POOL,
        "859d26f6c34ecdd3b11bf75007666ab28eb31bf96d37aa317f85706ea6073727",
        "c98b5c9cfa944bb53e3db546d83fbb9d0cecd0ca179ff01cbdbf76c318059fc9",
        "6a316529f57c0295d2e34909a15b446ec570444b310f401f002800985bdf9b6f",
        "ec2be3fc9dd82a59400338170cbf65b187fcf946380efc574c725003b667d7c9",
        HOLDING_B,
        HOLDING_A,
        LP,
        "bc326ed8c2dcd95b939dca07bd24efd0024d3140d9253ceaadac2de94d6c3b6b",
        "2f4c455a2f436c6f636b50726f6772616d4163636f756e742f30303030303031"};
    const std::vector<bool> signers = {false, false, false, false, false, false,
                                       true,  true,  true,  false, false};
    std::vector<std::uint8_t> instruction{4};
    appendLittle(instruction, 9000000, 16);
    appendLittle(instruction, 4000000, 16);
    appendLittle(instruction, 1000, 8);
    LOGOS_ASSERT_EQ(json(tx.accounts), json(accounts));
    LOGOS_ASSERT_EQ(json(tx.signers), json(signers));
    LOGOS_ASSERT_EQ(json(tx.instruction), json(instruction));
    LOGOS_ASSERT_EQ(tx.program, PROGRAM);
    LOGOS_ASSERT_EQ(tx.payer, PAYER);
}

LOGOS_TEST(real_ffi_swap_submits_full_width_amount_and_stored_vaults) {
    RealFixture f;
    const auto hash = f.module.swapExactInput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B,
                                              "18446744073709551617", 123, "1700000000000", PAYER);
    LOGOS_ASSERT_EQ(hash, std::string(TX));
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
    const auto& tx = f.modules.lez_core.submissions.front();
    const std::vector<std::string> accounts = {
        CONFIG,
        REAL_POOL,
        VAULT_A,
        VAULT_B,
        HOLDING_A,
        HOLDING_B,
        "bc326ed8c2dcd95b939dca07bd24efd0024d3140d9253ceaadac2de94d6c3b6b",
        "2f4c455a2f436c6f636b50726f6772616d4163636f756e742f30303030303031",
        "39ea338a46959c1b07bcedcd06b83119eef7ad3656dfc0f33ec3ea99f1a6cbda"};
    const std::vector<bool> signers = {false, false, false, false, true,
                                       false, false, false, false};
    std::vector<std::uint8_t> instruction{7};
    appendLittle(instruction, 1, 16);
    instruction[9] = 1;  // 2^64 + 1, encoded as a little-endian u128.
    appendLittle(instruction, 123, 16);
    appendLittle(instruction, 1700000000000ULL, 8);
    LOGOS_ASSERT_EQ(json(tx.accounts), json(accounts));
    LOGOS_ASSERT_EQ(json(tx.signers), json(signers));
    LOGOS_ASSERT_EQ(json(tx.instruction), json(instruction));
    LOGOS_ASSERT_EQ(tx.program, PROGRAM);
    LOGOS_ASSERT_EQ(tx.payer, PAYER);
}

LOGOS_TEST(real_ffi_rejects_foreign_owned_pool_for_reads_quotes_and_submits) {
    RealFixture f;
    auto pool = json::parse(f.modules.lez_core.accounts.at(REAL_POOL));
    pool["program_owner"] = OTHER;
    f.modules.lez_core.accounts[REAL_POOL] = pool.dump();
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "bad_config");
    assertError(f.module.swapExactInQuote(TOKEN_A, TOKEN_B, 10000, 50), "no_pool");
    assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 10000, 50), "no_pool");
    auto r = request();
    assertError(f.module.addLiquidityQuote(r), "no_pool");
    assertError(f.module.removeLiquidityQuote(r), "no_pool");
    LOGOS_ASSERT_TRUE(
        f.module.swapExactInput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, 10000, 1, 1000, PAYER)
            .empty());
    LOGOS_ASSERT_TRUE(
        f.module.swapExactOutput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, 10000, 20000, 1000, PAYER)
            .empty());
    assertError(f.module.addLiquidity(r), "no_pool");
    assertError(f.module.removeLiquidity(r), "no_pool");
    assertError(f.module.syncReserves(r), "no_pool");
    assertError(f.module.createPool(r), "already_exists");
    assertError(f.module.createPriceObservations(r), "no_pool");
    assertError(f.module.createOraclePriceAccount(r), "no_pool");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_pool_creation_requires_an_unoccupied_default_account) {
    RealFixture f;
    const auto empty = snapshot("", std::string(64, '0'));
    for (const auto& field : {"program_owner", "balance", "nonce", "data"}) {
        auto occupied = empty;
        if (std::string(field) == "program_owner")
            occupied[field] = OTHER;
        else if (std::string(field) == "data")
            occupied[field] = "00";
        else
            occupied[field] = "01" + std::string(30, '0');
        f.modules.lez_core.accounts[REAL_POOL] = occupied.dump();
        assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "bad_config");
        assertError(f.module.createPool(request()), "already_exists");
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    }
    f.modules.lez_core.accounts.erase(REAL_POOL);
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "no_pool");
    assertOk(f.module.createPool(request()));
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
}

LOGOS_TEST(real_ffi_oracle_setup_requires_an_amm_owned_pool) {
    RealFixture f;
    f.modules.lez_core.accounts.erase(REAL_POOL);
    assertError(f.module.createPriceObservations(request()), "no_pool");
    assertError(f.module.createOraclePriceAccount(request()), "no_pool");
    f.modules.lez_core.accounts[REAL_POOL] = snapshot(poolData(), OTHER).dump();
    assertError(f.module.createPriceObservations(request()), "no_pool");
    assertError(f.module.createOraclePriceAccount(request()), "no_pool");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_pool_read_failure_is_not_reported_as_a_vacant_pool) {
    RealFixture f;
    f.modules.lez_core.accounts[REAL_POOL] = "";
    assertError(f.module.resolvePoolAccount(TOKEN_A, TOKEN_B), "pool_read_failed");
    assertError(f.module.createPool(request()), "backend_error");
    assertError(f.module.createPriceObservations(request()), "backend_error");
    assertError(f.module.createOraclePriceAccount(request()), "backend_error");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_liquidity_submits_follow_stored_pool_orientation) {
    RealFixture f;
    auto r = request();
    r["tokenAId"] = TOKEN_B;
    r["tokenBId"] = TOKEN_A;
    r["holdingAId"] = HOLDING_B;
    r["holdingBId"] = HOLDING_A;
    r["maxAmountA"] = "30000";
    r["maxAmountB"] = "10000";
    r["lpAmount"] = "10000";
    r["minAmountA"] = "19900";
    r["minAmountB"] = "9950";
    r["deadlineMs"] = "1000";
    assertOk(f.module.addLiquidity(r));
    assertOk(f.module.removeLiquidity(r));
    assertOk(f.module.syncReserves(r));
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 3U);

    const std::vector<std::string> accounts = {
        CONFIG, REAL_POOL, VAULT_A, VAULT_B, LP_DEFINITION, HOLDING_A, HOLDING_B, LP, TICK, CLOCK};
    const auto& add = f.modules.lez_core.submissions[0];
    const auto& remove = f.modules.lez_core.submissions[1];
    const auto& sync = f.modules.lez_core.submissions[2];
    LOGOS_ASSERT_EQ(json(add.accounts), json(accounts));
    LOGOS_ASSERT_EQ(json(remove.accounts), json(accounts));
    LOGOS_ASSERT_EQ(json(add.signers), json(std::vector<bool>{false, false, false, false, false,
                                                              true, true, true, false, false}));
    LOGOS_ASSERT_EQ(json(remove.signers),
                    json(std::vector<bool>{false, false, false, false, false, false, false, true,
                                           false, false}));
    std::vector<std::uint8_t> add_bytes{5};
    appendLittle(add_bytes, 10, 16);
    appendLittle(add_bytes, 10000, 16);
    appendLittle(add_bytes, 30000, 16);
    appendLittle(add_bytes, 1000, 8);
    LOGOS_ASSERT_EQ(json(add.instruction), json(add_bytes));
    std::vector<std::uint8_t> remove_bytes{6};
    appendLittle(remove_bytes, 10000, 16);
    appendLittle(remove_bytes, 9950, 16);
    appendLittle(remove_bytes, 19900, 16);
    appendLittle(remove_bytes, 1000, 8);
    LOGOS_ASSERT_EQ(json(remove.instruction), json(remove_bytes));
    LOGOS_ASSERT_EQ(json(sync.accounts), json(std::vector<std::string>{CONFIG, REAL_POOL, VAULT_A,
                                                                       VAULT_B, TICK, CLOCK}));
    LOGOS_ASSERT_EQ(json(sync.signers), json(std::vector<bool>(6, false)));
    LOGOS_ASSERT_EQ(json(sync.instruction), json(std::vector<std::uint8_t>{9}));
    for (const auto& tx : f.modules.lez_core.submissions) {
        LOGOS_ASSERT_EQ(tx.program, PROGRAM);
        LOGOS_ASSERT_EQ(tx.payer, PAYER);
    }
}

LOGOS_TEST(real_ffi_rejects_invalid_slippage_and_window_without_submission) {
    RealFixture f;
    assertError(f.module.swapExactOutQuote(TOKEN_A, TOKEN_B, 100, -1), "invalid_slippage");
    auto r = request();
    r["windowDurationMs"] = "123abc";
    assertError(f.module.createPriceObservations(r), "invalid_window");
    r["windowDurationMs"] = "-1";
    assertError(f.module.createOraclePriceAccount(r), "invalid_window");
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_oracle_targets_accept_only_default_accounts) {
    const std::string observations_id =
        "f6c132254d93bf31bd9dafe9293651ad8b7a5b404d4893b9bfe0e21f69ffc292";
    const std::string price_id = "98946833dae09fbc9b48072df9550aaf3699ce383171fdf5438a143cbbbcd24b";
    for (const bool observations : {true, false}) {
        RealFixture f;
        auto r = request();
        r["windowDurationMs"] = "2048";
        const auto invoke = [&] {
            return observations ? f.module.createPriceObservations(r)
                                : f.module.createOraclePriceAccount(r);
        };
        const auto& target = observations ? observations_id : price_id;
        assertOk(invoke());  // Unset wallet entry returns an exact Account::default().
        LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
        const auto& tx = f.modules.lez_core.submissions.front();
        LOGOS_ASSERT_EQ(tx.accounts.at(observations ? 3 : 2), target);
        LOGOS_ASSERT_EQ(json(tx.signers), json(std::vector<bool>(tx.accounts.size(), false)));
        std::vector<std::uint8_t> expected{static_cast<std::uint8_t>(observations ? 2 : 3)};
        appendLittle(expected, 2048, 8);
        LOGOS_ASSERT_EQ(json(tx.instruction), json(expected));
        LOGOS_ASSERT_EQ(tx.payer, PAYER);

        const auto empty = snapshot("", std::string(64, '0'));
        for (const auto& field : {"program_owner", "balance", "nonce", "data"}) {
            auto occupied = empty;
            if (std::string(field) == "program_owner")
                occupied[field] = PROGRAM;
            else if (std::string(field) == "data")
                occupied[field] = "00";
            else
                occupied[field] = "01" + std::string(30, '0');
            f.modules.lez_core.accounts[target] = occupied.dump();
            assertError(invoke(), "already_exists");
            LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
        }
        // An unreadable target cannot be verified as available.
        f.modules.lez_core.accounts[target] = "";
        assertError(invoke(), "backend_error");
        LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
    }
}

LOGOS_TEST(real_ffi_oracle_planners_reject_windows_below_guest_capacity) {
    RealFixture f;
    for (const auto window : {1, 1000, 2047}) {
        auto r = request();
        r["windowDurationMs"] = window;
        assertError(f.module.createPriceObservations(r), "invalid_window");
        assertError(f.module.createOraclePriceAccount(r), "invalid_window");
    }
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
}

LOGOS_TEST(real_ffi_decodes_live_wallet_holdings_and_resolves_definitions) {
    RealFixture f;
    const std::string holding(64, 'c');
    const std::string foreign(64, '9');
    const std::string private_holding(64, 'd');
    const std::string holding_data = "00" + TOKEN_A + littleHex(42, 16);
    const std::string held_definition =
        "00" + littleHex(4, 4) + "48656c64" + littleHex(1000, 16) + "0000";
    const std::string listed_definition =
        "00" + littleHex(6, 4) + "4c6973746564" + littleHex(2000, 16) + "0000";
    // The configured token program is HOLDING_A in this fixture's AmmConfig.
    f.modules.lez_core.accounts[holding] = snapshot(holding_data, HOLDING_A).dump();
    f.modules.lez_core.accounts[foreign] = snapshot(holding_data, OTHER).dump();
    f.modules.lez_core.accounts[private_holding] = snapshot(holding_data, HOLDING_A).dump();
    f.modules.lez_core.accounts[TOKEN_A] = snapshot(held_definition, HOLDING_A).dump();
    f.modules.lez_core.accounts[TOKEN_B] = snapshot(listed_definition, HOLDING_A).dump();
    f.modules.lez_core.inventory =
        json::array({{{"account_id", holding}, {"is_public", true}},
                     {{"account_id", foreign}, {"is_public", true}},
                     {{"account_id", private_holding}, {"is_public", false}}});

    const auto holdings = f.module.tokenHoldings(true);
    LOGOS_ASSERT_EQ(holdings.size(), 1U);
    LOGOS_ASSERT_EQ(holdings[0]["accountId"], json(holding));
    LOGOS_ASSERT_EQ(holdings[0]["definitionIdHex"], json(TOKEN_A));
    LOGOS_ASSERT_EQ(holdings[0]["balanceRaw"], json("42"));
    LOGOS_ASSERT_EQ(holdings[0]["definitionId"],
                    json("CVDFLCAjXhVWiPXH9nTCTpCgVzmDVoiPzNJYuccr1dqB"));

    const json ids = {{"tokenIds", json::array({TOKEN_B, "token-a-base58", TOKEN_A, OTHER})}};
    auto rows = f.module.resolveTokens(ids, true);
    LOGOS_ASSERT_EQ(rows.size(), 2U);
    LOGOS_ASSERT_EQ(rows[0]["name"], json("Held"));
    LOGOS_ASSERT_EQ(rows[0]["totalSupply"], json("1000"));
    LOGOS_ASSERT_EQ(rows[0]["balance"], json("42"));
    LOGOS_ASSERT_EQ(rows[0]["holdingId"], json("EnTJCS15dqbDTU2XywYSMaScoPv4Py4GzExrtY9DQxoD"));
    LOGOS_ASSERT_EQ(rows[1]["name"], json("Listed"));
    LOGOS_ASSERT_EQ(rows[1]["balance"], json("0"));
    LOGOS_ASSERT_EQ(rows[1]["holdingId"], json(""));
    f.modules.lez_core.accounts[holding] =
        snapshot("00" + TOKEN_A + littleHex(500, 16), HOLDING_A).dump();
    rows = f.module.resolveTokens(ids, true);
    LOGOS_ASSERT_EQ(rows[0]["balance"], json("500"));
    const auto inventory_reads = f.modules.lez_core.inventory_reads;
    rows = f.module.resolveTokens(ids, false);
    LOGOS_ASSERT_EQ(f.modules.lez_core.inventory_reads, inventory_reads);
    for (const auto& row : rows) {
        LOGOS_ASSERT_EQ(row["balance"], json("0"));
        LOGOS_ASSERT_EQ(row["holdingId"], json(""));
    }
}
