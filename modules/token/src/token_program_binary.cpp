#include "token_program_binary.h"

#include <algorithm>
#include <cctype>

namespace token_module::detail {

namespace {

constexpr std::size_t IMAGE_ID_HEX_LENGTH = 64;

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return value;
}

}  // namespace

bool binaryMatchesProgramHeader(const std::string& image_id_hex,
                                const std::string& header_data_hex) {
    const std::string image_id = lowercase(image_id_hex);
    if (image_id.size() != IMAGE_ID_HEX_LENGTH
        || !std::all_of(image_id.begin(), image_id.end(), [](unsigned char c) {
               return std::isxdigit(c) != 0;
           })) {
        return false;
    }
    return header_data_hex.size() >= IMAGE_ID_HEX_LENGTH
        && lowercase(header_data_hex.substr(0, IMAGE_ID_HEX_LENGTH)) == image_id;
}

}  // namespace token_module::detail
