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
        std::string program;
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
        // LEZ returns Account::default() for an absent public account.
        return nlohmann::json{{"program_owner", std::string(64, '0')},
                              {"balance", std::string(32, '0')},
                              {"nonce", std::string(32, '0')},
                              {"data", ""}}
            .dump();
    }

    std::string send_generic_public_transaction(const std::vector<std::string>& account_ids,
                                                const std::vector<bool>& signers,
                                                const std::vector<std::uint8_t>& instruction,
                                                const std::string& program,
                                                const std::string& payer) {
        submissions.push_back({account_ids, signers, instruction, program, payer});
        return submission_reply;
    }
};

struct LogosModules {
    TestLezCore lez_core;
};
