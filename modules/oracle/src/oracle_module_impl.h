#pragma once

#include <logos_json.h>
#include <logos_module_context.h>

#include <initializer_list>
#include <mutex>
#include <string>

// Universal, Qt-free oracle orchestration: discovery plus one method per TWAP
// oracle instruction. Rust owns codecs, PDAs, validation, and instruction
// plans; lez_core owns wallet lifecycle and transaction submission.
//
// Every transaction method returns {status:"ok", error:"", transactionId, ...}
// with the operation's account ids (base58 and ...Hex), or
// {status:"error", error:<code>}. Numeric arguments are decimal strings.
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

    /// {priceSourceId, initialPrice}: create the source's current-tick account.
    /// initialPrice is a Q64.64 spot price; the source must be a public wallet
    /// account (it signs) and the account must not exist yet.
    LogosMap createCurrentTickAccount(const LogosMap& request);

    /// {priceSourceId, initialTick, windowDurationMs}: create the observations
    /// ring buffer for one window. initialTick is a signed i32.
    LogosMap createPriceObservations(const LogosMap& request);

    /// {priceSourceId, baseAssetId, quoteAssetId, initialPrice, windowDurationMs}:
    /// create the canonical price account for one window. initialPrice is a
    /// non-zero Q64.64 price (quote per base).
    LogosMap createOraclePriceAccount(const LogosMap& request);

    /// {priceSourceId, price}: write a new Q64.64 spot price to the source's
    /// existing current-tick account. The source signs.
    LogosMap updateCurrentTick(const LogosMap& request);

    /// {priceSourceId, windowDurationMs}: permissionless; sample the current
    /// tick into the window's observations. The program skips samples taken
    /// sooner than windowDurationMs / 2048 after the previous one.
    LogosMap recordTick(const LogosMap& request);

    /// {priceSourceId, windowDurationMs}: permissionless; publish the window's
    /// TWAP to its price account. A no-op until two observations exist.
    LogosMap publishPrice(const LogosMap& request);

private:
    // Defined only by tests; private helpers never become Logos API methods.
    friend struct OracleModuleTestAccess;

    std::string normalizeAccountId(const std::string& id);
    nlohmann::json readPublicAccount(const std::string& id);
    nlohmann::json resolveProgram(std::string& error);
    bool validIdentity(const nlohmann::json& value, const std::string& key);
    LogosMap submitPlan(const nlohmann::json& plan, const nlohmann::json& identities);

    using Planner = char* (*)(const char*);
    // An identity key from the plan and whether that account must already be
    // initialized by this program (true) or still be uninitialized (false).
    struct AccountRequirement {
        const char* identity;
        bool initialized;
    };
    LogosMap planAndSubmit(Planner planner, nlohmann::json request,
                           std::initializer_list<AccountRequirement> requirements);

    std::mutex selectionMutex_;
    std::string oracleProgramId_;
};
