#pragma once

#include <cstdint>
#include <functional>
#include <string>
#include <vector>

#include <nlohmann/json_fwd.hpp>

namespace stablecoin_module::detail {

using Base58Decoder = std::function<std::string(const std::string&)>;

// Accepts a 32-byte hex or base58 account ID and returns lowercase hex. Empty
// means invalid, zero, or rejected by the supplied base58 decoder.
std::string normalizeAccountId(const std::string& value,
                               const Base58Decoder& base58_decoder);

// Accepts only a nonzero 32-byte hexadecimal account ID.
bool isValidAccountIdHex(const std::string& value);

// Validates the wallet module's public-account response and converts it to the
// stablecoin_ffi AccountRead shape. Empty/default state becomes `not_found`;
// malformed state becomes `backend_error`.
nlohmann::json publicAccountRead(const std::string& account_id,
                                 const std::string& raw_response);

// Converts a RISC Zero word array to the byte string expected by the wallet
// module. Empty means malformed input.
std::vector<std::uint8_t> jsonInstructionLeBytes(const nlohmann::json& input);

// Preserves only documented FFI error codes at the public module boundary.
std::string stableFfiError(const std::string& error);

// Returns a validated lowercase 32-byte transaction hash, or empty on any
// malformed/failed wallet response.
std::string transactionId(const std::string& raw_response);

}  // namespace stablecoin_module::detail
