#pragma once

#include <string>

namespace token_module::detail {

// Whether a binary's ImageID (lowercase hex, 32 bytes) is the bytecode deployed
// behind a program header. Since LEZ v0.2.5 a program lives at the account its
// `program_loader` header was deployed to, so the ImageID is not the address; the
// Borsh `ProgramHeader` starts with `image_id` ([u32; 8], little-endian words --
// the same bytes as the ImageID), which is what `header_data_hex` is checked
// against.
bool binaryMatchesProgramHeader(const std::string& image_id_hex,
                                const std::string& header_data_hex);

}  // namespace token_module::detail
