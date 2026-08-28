#include <cstdlib>
#include <cstring>

#include <logos_clib_mock.h>

extern "C" {
#include "stablecoin_ffi.h"
}

namespace {

char* copyMockResponse(const char* function_name) {
    LOGOS_CMOCK_RECORD(function_name);
    const char* response = LOGOS_CMOCK_RETURN_STRING(function_name);
    if (response == nullptr) return nullptr;

    const std::size_t size = std::strlen(response) + 1;
    auto* copy = static_cast<char*>(std::malloc(size));
    if (copy == nullptr) return nullptr;
    std::memcpy(copy, response, size);
    return copy;
}

}  // namespace

extern "C" char* stablecoin_program_info(const char*) {
    return copyMockResponse("stablecoin_program_info");
}

extern "C" char* stablecoin_decode_protocol_parameters(const char*) {
    return copyMockResponse("stablecoin_decode_protocol_parameters");
}

extern "C" char* stablecoin_position_info(const char*) {
    return copyMockResponse("stablecoin_position_info");
}

extern "C" char* stablecoin_decode_position(const char*) {
    return copyMockResponse("stablecoin_decode_position");
}

extern "C" char* stablecoin_initialize_program_plan(const char*) {
    return copyMockResponse("stablecoin_initialize_program_plan");
}

extern "C" void stablecoin_free(char* value) {
    LOGOS_CMOCK_RECORD("stablecoin_free");
    std::free(value);
}
