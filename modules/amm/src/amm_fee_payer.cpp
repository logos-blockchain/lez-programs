#include "amm_fee_payer.h"

#include <algorithm>
#include <cctype>

namespace amm_module::detail {

namespace {

std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return value;
}

}  // namespace

std::string resolveFeePayer(const std::string& payer,
                            const std::vector<std::string>& accounts,
                            const std::vector<bool>& signers) {
    if (payer.empty())
        return {};
    for (size_t i = 0; i < accounts.size(); ++i) {
        const bool signs = i < signers.size() && signers[i];
        if (lowercase(accounts[i]) == payer && !signs)
            return {};
    }
    return payer;
}

}  // namespace amm_module::detail
