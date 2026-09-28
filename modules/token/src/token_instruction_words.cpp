#include "token_instruction_words.h"

#include <cstdint>
#include <limits>

#include <nlohmann/json.hpp>

namespace token_module::detail {

std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input) {
    std::vector<std::uint8_t> result;
    if (!input.is_array()) return result;
    result.reserve(input.size());
    for (const auto& item : input) {
        if (!item.is_number_unsigned() && !item.is_number_integer()) return {};
        std::int64_t raw = item.get<std::int64_t>();
        if (raw < 0 || raw > std::numeric_limits<std::uint8_t>::max()) return {};
        result.push_back(static_cast<std::uint8_t>(raw));
    }
    return result;
}

}  // namespace token_module::detail
