#include "token_instruction_words.h"

#include <cstdint>
#include <limits>

#include <logos_test.h>
#include <nlohmann/json.hpp>

LOGOS_TEST(instruction_bytes_passes_bytes_through) {
    const nlohmann::json input = nlohmann::json::array(
        {std::uint64_t{0}, std::uint64_t{1},
         std::uint64_t{std::numeric_limits<std::uint8_t>::max()}});

    const auto actual = token_module::detail::jsonInstructionLeBytes(input);
    const std::vector<std::uint8_t> expected = {0x00, 0x01, 0xff};
    LOGOS_ASSERT_EQ(actual.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        LOGOS_ASSERT_EQ(actual[i], expected[i]);
    }
}

LOGOS_TEST(instruction_bytes_rejects_negative_element) {
    const nlohmann::json input = nlohmann::json::array({std::int64_t{-1}});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_bytes_rejects_element_above_a_byte) {
    const nlohmann::json input = nlohmann::json::array(
        {std::uint64_t{std::numeric_limits<std::uint8_t>::max()} + 1});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_bytes_rejects_partial_invalid_input) {
    const nlohmann::json input = nlohmann::json::array({std::uint64_t{1}, 1.5});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_bytes_rejects_non_array) {
    const nlohmann::json input = nlohmann::json::object({{"word", 1}});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}
