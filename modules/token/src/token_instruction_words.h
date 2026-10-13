#pragma once

#include <cstdint>
#include <vector>

#include <nlohmann/json_fwd.hpp>

namespace token_module::detail {

// Decodes a plan's borsh `instruction` array (one number per byte) into the byte
// string the wallet module's send_generic_public_transaction expects (its
// `instruction` param is a byte-string IPC type). Returns {} on any non-array
// input or an element that is negative, fractional, or exceeds a byte.
std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input);

}  // namespace token_module::detail
