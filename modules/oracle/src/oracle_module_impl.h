#pragma once

#include <logos_json.h>
#include <logos_module_context.h>

#include <mutex>
#include <string>

// Universal, Qt-free oracle orchestration. Only the three public discovery
// methods are exported. Rust owns codecs, PDAs, validation, and instruction
// plans; lez_core owns wallet lifecycle and transaction submission.
class OracleModuleImpl : public LogosModuleContext {
public:
    /// Select the program id, its ImageID (base58 or hex). Empty clears the
    /// instance override and restores TWAP_ORACLE_PROGRAM_ID. Invalid input
    /// leaves the previous selection intact.
    LogosMap setOracleProgramId(const LogosMap& request);

    /// Resolve the program on every call: the selected or TWAP_ORACLE_PROGRAM_ID
    /// id, the ImageID of TWAP_ORACLE_PROGRAM_BIN, or both when they agree.
    LogosMap programInfo();

    /// Derive current-tick, observations, price, and clock IDs from priceSourceId
    /// and decimal-string windowDurationMs. Does not assert account existence.
    LogosMap feedAddresses(const LogosMap& request);

private:
    // Defined only by tests; private helpers never become Logos API methods.
    friend struct OracleModuleTestAccess;

    std::string normalizeAccountId(const std::string& id);
    nlohmann::json readPublicAccount(const std::string& id);
    nlohmann::json resolveProgram(std::string& error);
    bool validIdentity(const nlohmann::json& value, const std::string& key);
    LogosMap submitPlan(const nlohmann::json& plan, const nlohmann::json& identities);

    std::mutex selectionMutex_;
    std::string oracleProgramId_;
};
