#pragma once

#include <cstdint>
#include <vector>

#include <nlohmann/json_fwd.hpp>

namespace token_module::detail {

// Decodes a plan's `instruction` word array (u32 values) into the little-endian
// byte string the wallet module's send_generic_public_transaction expects (its
// `instruction` param is a byte-string IPC type). Returns {} on any non-array
// input or a word that is negative, fractional, or exceeds u32.
std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input);

}  // namespace token_module::detail
