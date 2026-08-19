#include "token_instruction_words.h"

#include <cstdint>
#include <limits>

#include <logos_test.h>
#include <nlohmann/json.hpp>

LOGOS_TEST(instruction_words_encodes_words_little_endian) {
    const nlohmann::json input = nlohmann::json::array(
        {std::uint64_t{0}, std::uint64_t{1},
         std::uint64_t{std::numeric_limits<std::uint32_t>::max()}});

    const auto actual = token_module::detail::jsonInstructionLeBytes(input);
    const std::vector<std::uint8_t> expected = {
        0x00, 0x00, 0x00, 0x00,  // 0
        0x01, 0x00, 0x00, 0x00,  // 1 (little-endian)
        0xff, 0xff, 0xff, 0xff,  // u32::MAX
    };
    LOGOS_ASSERT_EQ(actual.size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        LOGOS_ASSERT_EQ(actual[i], expected[i]);
    }
}

LOGOS_TEST(instruction_words_rejects_negative_word) {
    const nlohmann::json input = nlohmann::json::array({std::int64_t{-1}});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_words_rejects_overflow_word) {
    const nlohmann::json input = nlohmann::json::array(
        {std::uint64_t{std::numeric_limits<std::uint32_t>::max()} + 1});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_words_rejects_partial_invalid_input) {
    const nlohmann::json input = nlohmann::json::array({std::uint64_t{1}, 1.5});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}

LOGOS_TEST(instruction_words_rejects_non_array) {
    const nlohmann::json input = nlohmann::json::object({{"word", 1}});

    LOGOS_ASSERT_TRUE(token_module::detail::jsonInstructionLeBytes(input).empty());
}
