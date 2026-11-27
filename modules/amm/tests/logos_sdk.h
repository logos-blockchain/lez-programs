#pragma once

#include <cstdint>
#include <string>
#include <unordered_map>
#include <vector>

#include <nlohmann/json.hpp>

// Test-only replacement for the generated dependency client. Records the
// std-typed calls made by the AMM; no wallet or sequencer is contacted.
struct TestLezCore {
    struct Submission {
        std::vector<std::string> accounts;
        std::vector<bool> signers;
        std::vector<std::uint8_t> instruction;
        std::uint64_t root;
        std::vector<std::string> programs;
        std::string payer;
    };

    std::unordered_map<std::string, std::string> base58;
    std::unordered_map<std::string, std::string> accounts;
    nlohmann::json inventory = nlohmann::json::array();
    std::vector<std::string> reads;
    std::vector<Submission> submissions;
    int inventory_reads = 0;
    std::string submission_reply = R"({"success":true,"tx_hash":"test-transaction"})";

    std::string account_id_from_base58(const std::string& id) {
        const auto it = base58.find(id);
        return it == base58.end() ? std::string() : it->second;
    }

    nlohmann::json list_accounts() {
        ++inventory_reads;
        return inventory;
    }

    std::string get_account_public(const std::string& id) {
        reads.push_back(id);
        const auto it = accounts.find(id);
        if (it != accounts.end()) return it->second;
        return R"({"nonce":"0","actor_states":[]})";
    }

    std::string send_generic_public_transaction(const std::vector<std::string>& account_ids,
                                                std::uint64_t root,
                                                const std::vector<bool>& signers,
                                                const std::vector<std::uint8_t>& instruction,
                                                const std::string& payer,
                                                const std::vector<std::string>& programs) {
        submissions.push_back({account_ids, signers, instruction, root, programs, payer});
        return submission_reply;
    }
};

struct LogosModules {
    TestLezCore lez_core;
};
