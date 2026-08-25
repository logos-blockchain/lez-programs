#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <logos_json.h>
#include <logos_module_context.h>

// Universal Logos core module for the LEZ Token program. Public methods form
// the generated API, so this header deliberately uses only standard C++ and
// Logos JSON types. Rust token_ffi owns token codecs and instruction encoding;
// lez_core owns wallet/account I/O and transaction submission.
class TokenModuleImpl : public LogosModuleContext {
public:
    TokenModuleImpl() = default;
    ~TokenModuleImpl() = default;

    /// Returns `{status,error,programId,programIdHex,networkFingerprint}`.
    /// Configure TOKEN_PROGRAM_ID or TOKEN_PROGRAM_BIN. If both are set, they
    /// must identify the same program.
    LogosMap programInfo();

    /// Reads and decodes any public token definition account. `definition_id`
    /// accepts 32-byte hex or base58. Success adds `definition`; failures use
    /// invalid_account_id, account_read_failed, token_program_mismatch,
    /// invalid_definition_data, config_missing, or backend_error.
    LogosMap inspectDefinition(const std::string& definition_id);

    /// Reads and decodes any public token holding account. Success adds
    /// `holding`; failures follow the same stable read envelope.
    LogosMap inspectHolding(const std::string& holding_id);

    /// Reads and decodes any public token metadata account. Success adds
    /// `metadata`; failures follow the same stable read envelope.
    LogosMap inspectMetadata(const std::string& metadata_id);

    /// Discovers wallet-owned public token accounts from live reads. Invalid,
    /// foreign, unreadable, and ambiguously decoded rows are skipped. Success
    /// adds `accounts`, sorted by lowercase accountIdHex.
    LogosMap walletTokenAccounts();

    /// Creates a fungible definition and initial holding. Both target IDs must
    /// be fresh public wallet accounts and sign. `total_supply_raw` is an exact
    /// non-negative u128 decimal. `mint_authority`: none, self, or account ID.
    LogosMap createFungible(const std::string& definition_target_id,
                            const std::string& holding_target_id,
                            const std::string& name,
                            const nlohmann::json& total_supply_raw,
                            const std::string& mint_authority);

    /// Creates a metadata-backed fungible definition. All three target IDs are
    /// fresh public wallet accounts and sign. Standard is simple or expanded.
    LogosMap createFungibleWithMetadata(const std::string& definition_target_id,
                                        const std::string& holding_target_id,
                                        const std::string& metadata_target_id,
                                        const std::string& name,
                                        const nlohmann::json& total_supply_raw,
                                        const std::string& mint_authority,
                                        const std::string& metadata_standard,
                                        const std::string& uri,
                                        const std::string& creators);

    /// Creates a non-fungible definition, master holding, and metadata. All
    /// targets must be fresh public wallet accounts and sign.
    LogosMap createNonFungible(const std::string& definition_target_id,
                               const std::string& master_holding_target_id,
                               const std::string& metadata_target_id,
                               const std::string& name,
                               const nlohmann::json& printable_supply_raw,
                               const std::string& metadata_standard,
                               const std::string& uri,
                               const std::string& creators);

    /// Initializes a fresh wallet-owned holding; holding target signs.
    LogosMap initializeHolding(const std::string& definition_id,
                               const std::string& holding_target_id);

    /// Transfers exact raw amount. Sender signs. Recipient signs only when its
    /// live account is empty and wallet ownership proves it is fresh.
    LogosMap transfer(const std::string& sender_holding_id,
                      const std::string& recipient_holding_id,
                      const nlohmann::json& amount_raw);

    /// Burns exact raw amount. Holding signs.
    LogosMap burn(const std::string& definition_id,
                  const std::string& holding_id,
                  const nlohmann::json& amount_raw);

    /// Mints through self-authority. Definition signs; holding signs only when
    /// live state and wallet ownership prove it is fresh.
    LogosMap mint(const std::string& definition_id,
                  const std::string& holding_id,
                  const nlohmann::json& amount_raw);

    /// Mints through explicit external authority. Authority signs; holding
    /// signs only when live state and wallet ownership prove it is fresh.
    LogosMap mintWithAuthority(const std::string& definition_id,
                               const std::string& holding_id,
                               const std::string& authority_id,
                               const nlohmann::json& amount_raw);

    /// Rotates/revokes self authority. `new_authority`: none, self, or account
    /// ID. Definition signs.
    LogosMap setAuthority(const std::string& definition_id,
                          const std::string& new_authority);

    /// Rotates/revokes explicit external authority. Current authority signs.
    LogosMap setAuthorityWithAuthority(const std::string& definition_id,
                                       const std::string& authority_id,
                                       const std::string& new_authority);

    /// Prints an NFT copy. Master and fully fresh printed target both sign.
    LogosMap printNft(const std::string& master_holding_id,
                      const std::string& printed_holding_target_id);

private:
    using TokenOperation = char* (*)(const char*);

    std::vector<std::uint8_t> loadTokenBinary() const;
    std::string loadTokenProgramId() const;
    nlohmann::json tokenProgramInfo();
    std::string normalizeAccountId(const std::string& id);
    nlohmann::json readPublicAccount(const std::string& account_id);
    nlohmann::json walletAccountIds();
    bool isFreshOwnedAccount(const std::string& account_id,
                             bool& fresh,
                             std::string& error);
    bool requireFreshOwnedAccount(const std::string& account_id,
                                  std::string& error);
    LogosMap inspectAccount(const std::string& account_id,
                            TokenOperation operation,
                            const char* payload_key);
    bool normalizeAuthority(const std::string& authority,
                            std::string& normalized);
    LogosMap planAndSubmit(TokenOperation planner, nlohmann::json request);
    LogosMap submitPlan(const nlohmann::json& plan);

    bool programInfoResolved_ = false;
    std::string programId_;
    std::string programIdHex_;
};
