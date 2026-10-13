#pragma once

#include <string>
#include <vector>

namespace amm_module::detail {

// Checks a transaction's fee payer against its planned accounts. `payer` is an
// already-normalized lowercase hex id. Returns it unchanged when it is one of the
// signers or a separate account (the wallet then co-signs the fee), and empty
// when it is missing or takes part without signing, which the wallet rejects.
// A plan with no signers at all is fine: the payer is always separate there.
std::string resolveFeePayer(const std::string& payer,
                            const std::vector<std::string>& accounts,
                            const std::vector<bool>& signers);

}  // namespace amm_module::detail
