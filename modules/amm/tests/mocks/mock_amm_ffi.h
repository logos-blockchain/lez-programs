#pragma once

#include <string>
#include <nlohmann/json.hpp>

namespace amm_test::ffi {
void reset();
void success(const std::string& name, const nlohmann::json& value);
void failure(const std::string& name, const std::string& error);
int calls(const std::string& name);
int frees();
nlohmann::json lastRequest(const std::string& name);
}  // namespace amm_test::ffi
