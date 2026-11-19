#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <logos_json.h>

// Wallet boundary used by the public AMM module entrypoints in these tests.
struct LogosModules {
    struct Wallet {
        LogosMap account;
        int submissions = 0;

        std::string get_account_public(const std::string&) const { return account.dump(); }
        std::string account_id_from_base58(const std::string&) const { return {}; }
        LogosList list_accounts() const { return LogosList::array(); }
        std::string send_generic_public_transaction(
            const std::vector<std::string>&, std::uint64_t, const std::vector<bool>&,
            const std::vector<std::uint8_t>&, const std::string&, const std::vector<std::string>&) {
            ++submissions;
            return LogosMap{{"success", true}, {"tx_hash", std::string(64, 'f')}}.dump();
        }
    } lez_core;
};
