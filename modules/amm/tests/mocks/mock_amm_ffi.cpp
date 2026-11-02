#include "mock_amm_ffi.h"
#include "amm_ffi.h"

#include <cstdlib>
#include <cstring>
#include <string>
#include <unordered_map>
#include <vector>

namespace amm_test::ffi {
namespace {
using json = nlohmann::json;
std::unordered_map<std::string, std::string> replies;
std::unordered_map<std::string, std::vector<json>> requests;
int freed = 0;
}  // namespace

void reset() {
    replies.clear();
    requests.clear();
    freed = 0;
}
void success(const std::string& name, const json& value) {
    replies[name] = json{{"ok", true}, {"value", value}}.dump();
}
void failure(const std::string& name, const std::string& error) {
    replies[name] = json{{"ok", false}, {"error", error}}.dump();
}
int calls(const std::string& name) { return static_cast<int>(requests[name].size()); }
int frees() { return freed; }
json lastRequest(const std::string& name) {
    const auto it = requests.find(name);
    return it == requests.end() || it->second.empty() ? json() : it->second.back();
}

char* invoke(const char* name, const char* request) {
    requests[name].push_back(json::parse(request));
    const auto it = replies.find(name);
    if (it == replies.end()) return nullptr;
    auto* copy = static_cast<char*>(std::malloc(it->second.size() + 1));
    if (copy) std::memcpy(copy, it->second.c_str(), it->second.size() + 1);
    return copy;
}
void release(char* value) {
    if (value) ++freed;
    std::free(value);
}
}  // namespace amm_test::ffi

#define AMM_MOCK(name) \
    extern "C" char* name(const char* request) { return amm_test::ffi::invoke(#name, request); }
AMM_MOCK(amm_config_id)
AMM_MOCK(amm_config_account)
AMM_MOCK(amm_pair_ids)
AMM_MOCK(amm_resolve_tokens)
AMM_MOCK(amm_swap_pair)
AMM_MOCK(amm_resolve_pool)
AMM_MOCK(amm_pool_id)
AMM_MOCK(amm_swap_exact_in_quote)
AMM_MOCK(amm_swap_exact_out_quote)
AMM_MOCK(amm_swap_exact_in_plan)
AMM_MOCK(amm_swap_exact_out_plan)
AMM_MOCK(amm_create_pool_quote)
AMM_MOCK(amm_create_pool_plan)
AMM_MOCK(amm_add_liquidity_quote)
AMM_MOCK(amm_add_liquidity_plan)
AMM_MOCK(amm_remove_liquidity_quote)
AMM_MOCK(amm_remove_liquidity_plan)
AMM_MOCK(amm_sync_reserves_plan)
AMM_MOCK(amm_transfer_ownership_plan)
AMM_MOCK(amm_withdraw_protocol_fees_plan)
AMM_MOCK(amm_create_price_observations_plan)
AMM_MOCK(amm_create_oracle_price_account_plan)
AMM_MOCK(amm_token_holdings)
AMM_MOCK(amm_program_id)
AMM_MOCK(amm_fee_tiers)
#undef AMM_MOCK
extern "C" void amm_free(char* value) { amm_test::ffi::release(value); }
