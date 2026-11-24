#pragma once
#include "test_support.h"
#include "mocks/mock_amm_ffi.h"

namespace amm_test {
struct MockFixture : Fixture {
    MockFixture() {
        ffi::reset();
        ffi::success("amm_pool_id", {{"poolId", POOL}});
        ffi::success("amm_swap_pair", {{"status", "ok"}, {"poolId", POOL}});
        ffi::success("amm_config_account", {{"status", "ok"}, {"error", ""}});
    }
};

inline json plan(const std::vector<std::string>& accounts, const std::vector<bool>& signers,
                 const std::vector<std::uint8_t>& instruction) {
    return {{"programId", PROGRAM},
            {"rootMention", 0},
            {"actorProgramIds", std::vector<std::string>(accounts.size(), PROGRAM)},
            {"accountIds", accounts},
            {"signingRequirements", signers},
            {"message", instruction}};
}

inline json oraclePlan(json transaction, const std::string& observations, const std::string& price) {
    transaction["rootMention"] = 0;
    transaction["windowActors"] = {{"programId", PROGRAM}, {"accountIds", {observations, price}}};
    return transaction;
}

inline void assertSubmission(const TestLezCore::Submission& sent, const json& expected,
                             const std::string& payer = PAYER) {
    LOGOS_ASSERT_EQ(json(sent.accounts), expected.at("accountIds"));
    LOGOS_ASSERT_EQ(json(sent.signers), expected.at("signingRequirements"));
    LOGOS_ASSERT_EQ(json(sent.instruction), expected.at("message"));
    LOGOS_ASSERT_EQ(sent.root, expected.at("rootMention").get<std::uint64_t>());
    LOGOS_ASSERT_EQ(json(sent.programs), expected.at("actorProgramIds"));
    LOGOS_ASSERT_EQ(sent.payer, payer);
}
}  // namespace amm_test
