#pragma once

#include <logos_json.h>
#include <logos_module_context.h>

#include <mutex>
#include <string>

// Universal, Qt-free oracle orchestration. Only the three public discovery
// methods are exported. Rust owns codecs, PDAs, validation, and Borsh plans;
// lez_core owns wallet lifecycle and transaction submission.
class OracleModuleImpl : public LogosModuleContext {
public:
    /// Select a deployed header address (base58 or hex). Empty clears the
    /// instance override and restores TWAP_ORACLE_PROGRAM_ID. Invalid input
    /// leaves the previous selection intact.
    LogosMap setOracleProgramId(const LogosMap& request);

    /// Resolve the current deployment on every call. TWAP_ORACLE_PROGRAM_BIN
    /// optionally verifies its ImageID against the live loader header. A binary
    /// alone cannot provide the required deployed program address.
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
    LogosMap submitPlan(const nlohmann::json& plan, const std::string& fee_payer,
                        const nlohmann::json& identities);

    std::mutex selectionMutex_;
    std::string oracleProgramId_;
};
