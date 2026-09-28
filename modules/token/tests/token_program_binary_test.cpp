#include "token_program_binary.h"

#include <string>

#include <logos_test.h>

using token_module::detail::binaryMatchesProgramHeader;

namespace {

// A deployed header: image_id, first segment account id, immutable flag.
const std::string IMAGE_ID = std::string(62, 'a') + "0f";
const std::string HEADER = IMAGE_ID + std::string(64, 'c') + "01";

}  // namespace

LOGOS_TEST(binary_matches_the_header_it_was_deployed_behind) {
    LOGOS_ASSERT_TRUE(binaryMatchesProgramHeader(IMAGE_ID, HEADER));
    LOGOS_ASSERT_TRUE(binaryMatchesProgramHeader(IMAGE_ID, std::string(62, 'A') + "0F"
                                                               + std::string(66, '0')));
}

LOGOS_TEST(binary_for_other_bytecode_does_not_match) {
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader(std::string(64, 'b'), HEADER));
}

// The program address is chosen at deploy time and is not the ImageID: a header
// whose first segment happens to equal the ImageID must not count as a match.
LOGOS_TEST(image_id_is_compared_with_the_header_not_the_address) {
    const std::string header = std::string(64, 'd') + IMAGE_ID + "00";
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader(IMAGE_ID, header));
}

LOGOS_TEST(truncated_or_empty_inputs_do_not_match) {
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader(IMAGE_ID, IMAGE_ID.substr(0, 62)));
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader(IMAGE_ID, ""));
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader("", HEADER));
    LOGOS_ASSERT_FALSE(binaryMatchesProgramHeader(std::string(64, 'z'), std::string(130, 'z')));
}
