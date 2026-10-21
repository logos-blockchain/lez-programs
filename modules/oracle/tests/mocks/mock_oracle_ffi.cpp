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
extern "C" void oracle_free(char* value) {
    LOGOS_CMOCK_RECORD("oracle_free");
    std::free(value);
}
