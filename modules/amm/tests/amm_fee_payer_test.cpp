#include "amm_fee_payer.h"

#include <string>
#include <vector>

#include <logos_test.h>

using amm_module::detail::resolveFeePayer;

namespace {

std::string id(char value) {
    return std::string(64, value);
}

}  // namespace

// syncReserves and the oracle setup ops sign with nothing: the payer is a separate
// funded account and the instruction accounts and signer flags stay as planned.
LOGOS_TEST(separate_payer_pays_for_a_plan_without_signers) {
    const std::vector<std::string> accounts = {id('a'), id('b'), id('c')};
    const std::vector<bool> signers = {false, false, false};

    LOGOS_ASSERT_EQ(resolveFeePayer(id('f'), accounts, signers), id('f'));
}

LOGOS_TEST(missing_payer_is_rejected) {
    LOGOS_ASSERT_TRUE(resolveFeePayer("", {id('a')}, {false}).empty());
    LOGOS_ASSERT_TRUE(resolveFeePayer("", {id('a')}, {true}).empty());
}

LOGOS_TEST(non_signing_participant_cannot_pay) {
    const std::vector<std::string> accounts = {id('a'), id('b')};
    const std::vector<bool> signers = {true, false};

    LOGOS_ASSERT_TRUE(resolveFeePayer(id('b'), accounts, signers).empty());
}

LOGOS_TEST(signer_may_pay_for_itself) {
    const std::vector<std::string> accounts = {id('a'), id('b')};
    const std::vector<bool> signers = {true, false};

    LOGOS_ASSERT_EQ(resolveFeePayer(id('a'), accounts, signers), id('a'));
}

LOGOS_TEST(planned_ids_match_case_insensitively) {
    const std::vector<std::string> accounts = {std::string(64, 'B')};

    LOGOS_ASSERT_TRUE(resolveFeePayer(id('b'), accounts, {false}).empty());
}
