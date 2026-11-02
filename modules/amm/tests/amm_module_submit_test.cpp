#include "mock_support.h"

#include <functional>
#include <initializer_list>
#include <limits>
#include <utility>
#include <vector>

using namespace amm_test;
namespace {
struct Operation {
    const char* planner;
    json planned;
    json expected_request;
    std::function<json(AmmModuleImpl&, const json&)> invoke;
    bool returns_hash;
};

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

std::vector<Operation> operations() {
    const std::string vault_a(64, 'c'), vault_b(64, 'd'), tick(64, 'e'), clock(64, 'f');
    const std::string lp_definition(64, '0'), fee_account(64, '9');
    constexpr std::uint64_t deadline = 1700000000000ULL;
    auto transfer_bytes = std::vector<std::uint8_t>{1};
    transfer_bytes.insert(transfer_bytes.end(), 32, 0x99);
    const auto transfer = plan({CONFIG, SIGNER}, {false, true}, transfer_bytes);
    const auto fees = plan({CONFIG, fee_account, HOLDING_A, SIGNER}, {false, false, false, true},
                           encoded(10, {{42, 16}}));
    const auto observations = plan({CONFIG, POOL, tick, vault_b, clock},
                                   {false, false, false, false, false}, encoded(2, {{2048, 8}}));
    const auto price =
        plan({CONFIG, POOL, vault_b, clock}, {false, false, false, false}, encoded(3, {{2048, 8}}));
    const std::vector<std::string> swap_accounts = {CONFIG,    POOL, vault_a, vault_b,    HOLDING_A,
                                                    HOLDING_B, tick, clock,   fee_account};
    const std::vector<bool> swap_signers = {false, false, false, false, true,
                                            false, false, false, false};
    const auto swap_in =
        plan(swap_accounts, swap_signers, encoded(7, {{4000000, 16}, {1, 16}, {deadline, 8}}));
    const auto swap_out = plan(swap_accounts, swap_signers,
                               encoded(8, {{9000000, 16}, {4000000, 16}, {deadline, 8}}));
    const auto create =
        plan({CONFIG, POOL, vault_a, vault_b, lp_definition, fee_account, HOLDING_B, HOLDING_A, LP,
              tick, clock},
             {false, false, false, false, false, false, true, true, true, false, false},
             encoded(4, {{9000000, 16}, {4000000, 16}, {deadline, 8}}));
    const std::vector<std::string> liquidity_accounts = {
        CONFIG, POOL, vault_a, vault_b, lp_definition, HOLDING_A, HOLDING_B, LP, tick, clock};
    const auto add = plan(liquidity_accounts,
                          {false, false, false, false, false, true, true, true, false, false},
                          encoded(5, {{10, 16}, {4000000, 16}, {9000000, 16}, {deadline, 8}}));
    const auto remove = plan(liquidity_accounts,
                             {false, false, false, false, false, false, false, true, false, false},
                             encoded(6, {{20, 16}, {1, 16}, {2, 16}, {deadline, 8}}));
    const auto sync = plan({CONFIG, POOL, vault_a, vault_b, tick, clock},
                           {false, false, false, false, false, false}, encoded(9, {}));
    const auto with_config = [](json fields) {
        fields["ammProgramId"] = PROGRAM;
        fields["config"] = {{"id", CONFIG}, {"status", "ok"}, {"account", snapshot(configData())}};
        return fields;
    };
    return {
        {"amm_transfer_ownership_plan", transfer, with_config({{"newAuthorityId", OTHER}}),
         [](auto& m, const auto& r) { return m.transferOwnership(r); }, false},
        {"amm_withdraw_protocol_fees_plan", fees,
         with_config(
             {{"tokenDefinitionId", TOKEN_A}, {"destinationId", HOLDING_A}, {"amount", "42"}}),
         [](auto& m, const auto& r) { return m.withdrawProtocolFees(r); }, false},
        {"amm_create_price_observations_plan", observations,
         with_config({{"tokenAId", TOKEN_A}, {"tokenBId", TOKEN_B}, {"windowDurationMs", 2048}}),
         [](auto& m, const auto& r) { return m.createPriceObservations(r); }, false},
        {"amm_create_oracle_price_account_plan", price,
         with_config({{"tokenAId", TOKEN_A}, {"tokenBId", TOKEN_B}, {"windowDurationMs", 2048}}),
         [](auto& m, const auto& r) { return m.createOraclePriceAccount(r); }, false},
        {"amm_swap_exact_in_plan", swap_in,
         with_config({{"tokenInId", TOKEN_A},
                      {"tokenOutId", TOKEN_B},
                      {"poolData", poolData()},
                      {"userInputHoldingId", HOLDING_A},
                      {"userOutputHoldingId", HOLDING_B},
                      {"amountIn", "4000000"},
                      {"minOut", "1"},
                      {"deadlineMs", "1700000000000"}}),
         [](auto& m, const auto& r) {
             return json(m.swapExactInput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, r.at("amountA"),
                                          r.at("minAmountA"), r.at("deadlineMs"),
                                          r.at("feePayerId").template get<std::string>()));
         },
         true},
        {"amm_swap_exact_out_plan", swap_out,
         with_config({{"tokenInId", TOKEN_A},
                      {"tokenOutId", TOKEN_B},
                      {"poolData", poolData()},
                      {"userInputHoldingId", HOLDING_A},
                      {"userOutputHoldingId", HOLDING_B},
                      {"amountOut", "9000000"},
                      {"maxIn", "4000000"},
                      {"deadlineMs", "1700000000000"}}),
         [](auto& m, const auto& r) {
             return json(m.swapExactOutput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, r.at("amountB"),
                                           r.at("maxAmountA"), r.at("deadlineMs"),
                                           r.at("feePayerId").template get<std::string>()));
         },
         true},
        {"amm_create_pool_plan", create,
         with_config({{"tokenAId", TOKEN_A},
                      {"tokenBId", TOKEN_B},
                      {"amountA", "4000000"},
                      {"amountB", "9000000"},
                      {"deadlineMs", "1700000000000"},
                      {"userHoldingAId", HOLDING_A},
                      {"userHoldingBId", HOLDING_B},
                      {"userHoldingLpId", LP}}),
         [](auto& m, const auto& r) { return m.createPool(r); }, false},
        {"amm_add_liquidity_plan", add,
         with_config({{"tokenAId", TOKEN_A},
                      {"tokenBId", TOKEN_B},
                      {"maxAmountA", "4000000"},
                      {"maxAmountB", "9000000"},
                      {"minLp", "10"},
                      {"deadlineMs", "1700000000000"},
                      {"userHoldingAId", HOLDING_A},
                      {"userHoldingBId", HOLDING_B},
                      {"userHoldingLpId", LP},
                      {"poolData", poolData()}}),
         [](auto& m, const auto& r) { return m.addLiquidity(r); }, false},
        {"amm_remove_liquidity_plan", remove,
         with_config({{"tokenAId", TOKEN_A},
                      {"tokenBId", TOKEN_B},
                      {"lpAmount", "20"},
                      {"minAmountA", "1"},
                      {"minAmountB", "2"},
                      {"deadlineMs", "1700000000000"},
                      {"userHoldingAId", HOLDING_A},
                      {"userHoldingBId", HOLDING_B},
                      {"userHoldingLpId", LP},
                      {"poolData", poolData()}}),
         [](auto& m, const auto& r) { return m.removeLiquidity(r); }, false},
        {"amm_sync_reserves_plan", sync,
         with_config({{"tokenAId", TOKEN_A}, {"tokenBId", TOKEN_B}, {"poolData", poolData()}}),
         [](auto& m, const auto& r) { return m.syncReserves(r); }, false},
    };
}

void assertSuccess(const Operation& operation, const json& result) {
    if (operation.returns_hash) {
        LOGOS_ASSERT_EQ(result, json(TX));
    } else {
        assertOk(result);
        LOGOS_ASSERT_EQ(result.at("transactionId"), json(TX));
    }
}
void assertFailure(const Operation& operation, const json& result, const char* error) {
    if (operation.returns_hash)
        LOGOS_ASSERT_EQ(result, json(""));
    else
        assertError(result, error);
}
}  // namespace

LOGOS_TEST(all_transaction_methods_forward_plans_and_wallet_results) {
    for (const auto& op : operations()) {
        MockFixture f;
        if (std::string(op.planner) == "amm_create_pool_plan")
            f.modules.lez_core.accounts.erase(POOL);
        ffi::success(op.planner, op.planned);
        const auto result = op.invoke(f.module, request());
        assertSuccess(op, result);
        LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
        assertSubmission(f.modules.lez_core.submissions.front(), op.planned);
        LOGOS_ASSERT_EQ(ffi::lastRequest(op.planner), op.expected_request);
        LOGOS_ASSERT_EQ(ffi::frees(), ffi::calls(op.planner) + ffi::calls("amm_pool_id"));
    }
}

LOGOS_TEST(all_transaction_methods_propagate_failures_and_reject_non_signing_payers) {
    for (const auto& op : operations()) {
        MockFixture f;
        if (std::string(op.planner) == "amm_create_pool_plan")
            f.modules.lez_core.accounts.erase(POOL);
        ffi::failure(op.planner, "config_unavailable");
        assertFailure(op, op.invoke(f.module, request()), "config_unavailable");
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
        ffi::success(op.planner, op.planned);
        auto bad_payer = request();
        bad_payer["feePayerId"] = CONFIG;
        assertFailure(op, op.invoke(f.module, bad_payer), "invalid_fee_payer");
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
        for (const auto& reply :
             {std::string(R"({"success":false})"), std::string(), std::string("not-json")}) {
            const auto previous = f.modules.lez_core.submissions.size();
            f.modules.lez_core.submission_reply = reply;
            assertFailure(op, op.invoke(f.module, request()), "wallet_submission_failed");
            LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), previous + 1);
        }
    }
}

LOGOS_TEST(pool_id_failures_stop_dependent_planners_before_submission) {
    for (const auto& op : operations()) {
        const std::string name = op.planner;
        if (name != "amm_swap_exact_in_plan" && name != "amm_swap_exact_out_plan" &&
            name != "amm_add_liquidity_plan" && name != "amm_remove_liquidity_plan" &&
            name != "amm_sync_reserves_plan")
            continue;
        MockFixture f;
        f.modules.lez_core.accounts[CONFIG] = "";
        ffi::failure("amm_pool_id", "account read failed");
        assertFailure(op, op.invoke(f.module, request()), "account read failed");
        LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 1);
        LOGOS_ASSERT_EQ(ffi::lastRequest("amm_pool_id")["config"]["status"], json("read_failed"));
        LOGOS_ASSERT_EQ(ffi::calls(op.planner), 0);
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    }
}

LOGOS_TEST(missing_program_prevents_all_transaction_plans) {
    for (const auto& op : operations()) {
        MockFixture f;
        ScopedEnv no_program("AMM_PROGRAM_ID", nullptr);
        assertFailure(op, op.invoke(f.module, request()), "config_missing");
        LOGOS_ASSERT_EQ(ffi::calls(op.planner), 0);
        LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 0);
        LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    }
}

LOGOS_TEST(missing_config_prevents_all_transaction_plans) {
    for (const auto& op : operations()) {
        MockFixture f;
        ScopedEnv no_config("AMM_CONFIG_ID", nullptr);
        const std::string name = op.planner;
        const bool admin_or_oracle = name == "amm_transfer_ownership_plan" ||
                                     name == "amm_withdraw_protocol_fees_plan" ||
                                     name == "amm_create_price_observations_plan" ||
                                     name == "amm_create_oracle_price_account_plan";
        assertFailure(op, op.invoke(f.module, request()),
                      admin_or_oracle ? "config_missing" : "backend_error");
        LOGOS_ASSERT_EQ(ffi::calls(op.planner), 0);
        LOGOS_ASSERT_EQ(ffi::calls("amm_pool_id"), 0);
        LOGOS_ASSERT_TRUE(f.modules.lez_core.reads.empty());
        LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
    }
}

LOGOS_TEST(signer_can_pay_and_invalid_amounts_do_not_submit) {
    MockFixture f;
    std::vector<std::uint8_t> instruction(33, 0x99);
    instruction[0] = 1;
    const auto tx = plan({CONFIG, SIGNER}, {false, true}, instruction);
    ffi::success("amm_transfer_ownership_plan", tx);
    auto r = request();
    r["feePayerId"] = SIGNER;
    assertOk(f.module.transferOwnership(r));
    assertSubmission(f.modules.lez_core.submissions.front(), tx, SIGNER);
    r["amount"] = -1;
    assertError(f.module.withdrawProtocolFees(r), "invalid_amount");
    r["amountA"] = 1.5;
    assertError(f.module.createPool(r), "bad_amount");
    r["maxAmountA"] = "1e5";
    assertError(f.module.addLiquidity(r), "bad_amount");
    r["lpAmount"] = "-1";
    assertError(f.module.removeLiquidity(r), "bad_amount");
    LOGOS_ASSERT_TRUE(
        f.module.swapExactInput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, 1.5, 1, 1, PAYER).empty());
    LOGOS_ASSERT_TRUE(
        f.module.swapExactOutput(TOKEN_A, TOKEN_B, HOLDING_A, HOLDING_B, 1, 1, -1, PAYER).empty());
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 1U);
}

LOGOS_TEST(base58_fee_payers_are_normalized_before_signer_checks) {
    MockFixture f;
    std::vector<std::uint8_t> instruction(33, 0x99);
    instruction[0] = 1;
    const auto tx = plan({CONFIG, SIGNER}, {false, true}, instruction);
    ffi::success("amm_transfer_ownership_plan", tx);
    auto r = request();
    r["feePayerId"] = "payer-base58";
    assertOk(f.module.transferOwnership(r));
    assertSubmission(f.modules.lez_core.submissions.back(), tx, PAYER);
    r["feePayerId"] = "signer-base58";
    assertOk(f.module.transferOwnership(r));
    assertSubmission(f.modules.lez_core.submissions.back(), tx, SIGNER);
    r["feePayerId"] = "config-base58";
    assertError(f.module.transferOwnership(r), "invalid_fee_payer");
    LOGOS_ASSERT_EQ(f.modules.lez_core.submissions.size(), 2U);
}

LOGOS_TEST(invalid_account_ids_do_not_reach_transaction_planning) {
    MockFixture f;
    auto r = request();
    r["holdingAId"] = "invalid!";
    assertError(f.module.createPool(r), "invalid_account_id");
    assertError(f.module.addLiquidity(r), "invalid_account_id");
    assertError(f.module.removeLiquidity(r), "invalid_account_id");
    r = request();
    r["tokenAId"] = "invalid!";
    assertError(f.module.syncReserves(r), "invalid_token_id");
    assertError(f.module.createPriceObservations(r), "invalid_token_id");
    assertError(f.module.createOraclePriceAccount(r), "invalid_token_id");
    r = request();
    r["newAuthorityId"] = "invalid!";
    assertError(f.module.transferOwnership(r), "invalid_account_id");
    r["destinationId"] = "invalid!";
    assertError(f.module.withdrawProtocolFees(r), "invalid_account_id");
    LOGOS_ASSERT_EQ(ffi::calls("amm_create_pool_plan"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_add_liquidity_plan"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_remove_liquidity_plan"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_sync_reserves_plan"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_transfer_ownership_plan"), 0);
    LOGOS_ASSERT_EQ(ffi::calls("amm_withdraw_protocol_fees_plan"), 0);
    LOGOS_ASSERT_TRUE(f.modules.lez_core.submissions.empty());
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
