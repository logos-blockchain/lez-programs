#include <logos_clib_mock.h>

#include <cstdlib>
#include <cstring>
#include <string>

extern "C" {
#include "oracle_ffi.h"
}

// Inspect the actual C++ -> Rust wire requests, not just the call count.
std::string oracleLastProgramRequest;
std::string oracleLastFeedRequest;
std::string oracleLastPlanRequest;

namespace {
char* response(const char* function) {
    LOGOS_CMOCK_RECORD(function);
    const char* value = LOGOS_CMOCK_RETURN_STRING(function);
    if (value == nullptr) return nullptr;
    const auto size = std::strlen(value) + 1;
    auto* copy = static_cast<char*>(std::malloc(size));
    if (copy != nullptr) std::memcpy(copy, value, size);
    return copy;
}
}  // namespace

extern "C" char* oracle_program_info(const char* request) {
    oracleLastProgramRequest = request;
    return response("oracle_program_info");
}
extern "C" char* oracle_feed_addresses(const char* request) {
    oracleLastFeedRequest = request;
    return response("oracle_feed_addresses");
}
// One mock per planner; each records its last request for wire assertions.
#define ORACLE_PLAN_MOCK(name)                          \
    extern "C" char* name(const char* request) {        \
        oracleLastPlanRequest = request;                \
        return response(#name);                         \
    }
ORACLE_PLAN_MOCK(oracle_create_current_tick_account_plan)
ORACLE_PLAN_MOCK(oracle_create_price_observations_plan)
ORACLE_PLAN_MOCK(oracle_create_oracle_price_account_plan)
ORACLE_PLAN_MOCK(oracle_update_current_tick_plan)
ORACLE_PLAN_MOCK(oracle_record_tick_plan)
ORACLE_PLAN_MOCK(oracle_publish_price_plan)
#undef ORACLE_PLAN_MOCK

extern "C" void oracle_free(char* value) {
    LOGOS_CMOCK_RECORD("oracle_free");
    std::free(value);
}
