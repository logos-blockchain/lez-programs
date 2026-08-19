#include "token_instruction_words.h"

#include <cstdint>
#include <limits>

#include <nlohmann/json.hpp>

namespace token_module::detail {

std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input) {
    std::vector<std::uint8_t> result;
    if (!input.is_array()) return result;
    result.reserve(input.size() * sizeof(std::uint32_t));
    for (const auto& item : input) {
        if (!item.is_number_unsigned() && !item.is_number_integer()) return {};
        std::uint64_t raw = 0;
        if (item.is_number_unsigned()) {
            raw = item.get<std::uint64_t>();
        } else {
            const std::int64_t signed_raw = item.get<std::int64_t>();
            if (signed_raw < 0) return {};
            raw = static_cast<std::uint64_t>(signed_raw);
        }
        if (raw > std::numeric_limits<std::uint32_t>::max()) return {};
        const auto word = static_cast<std::uint32_t>(raw);
        result.push_back(static_cast<std::uint8_t>(word & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 8) & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 16) & 0xff));
        result.push_back(static_cast<std::uint8_t>((word >> 24) & 0xff));
    }
    return result;
}

}  // namespace token_module::detail
