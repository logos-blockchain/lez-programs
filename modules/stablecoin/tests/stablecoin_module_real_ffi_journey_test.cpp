#include "stablecoin_module_impl.h"

#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <boost/multiprecision/cpp_int.hpp>
#include <logos_test.h>
#include <nlohmann/json.hpp>
#include <mock_store.h>

#include "logos_sdk.h"

extern "C" {
#include "stablecoin_ffi.h"
}

namespace {

using boost::multiprecision::cpp_int;
using json = nlohmann::json;

const std::string PROGRAM_ID_HEX = [] {
    std::string value;
    for (int index = 0; index < 8; ++index) value += "11000000";
    return value;
}();
const std::string CALLER_ID_HEX(64, '9');
const std::string TRANSACTION_ID_HEX(64, 'b');
const std::string PROGRAM_OWNER_HEX = PROGRAM_ID_HEX;
const std::string ORACLE_OWNER_HEX = [] {
    std::string value;
    for (int index = 0; index < 8; ++index) value += "33000000";
    return value;
}();
const std::string TOKEN_OWNER_HEX = [] {
    std::string value;
    for (int index = 0; index < 8; ++index) value += "22000000";
    return value;
}();
const std::string FIXED_ONE = "1000000000000000000000000000";
constexpr std::uint64_t START = 1'000;
constexpr std::uint64_t DUE = 601'000;

class ScopedEnvironment {
public:
    ScopedEnvironment(std::string name, const char* value)
        : name_(std::move(name)) {
        if (const char* previous = std::getenv(name_.c_str()); previous != nullptr) {
            previous_ = previous;
        }
        if (value == nullptr) {
            unsetenv(name_.c_str());
        } else {
            setenv(name_.c_str(), value, 1);
        }
    }

    ~ScopedEnvironment() {
        if (previous_.has_value()) {
            setenv(name_.c_str(), previous_->c_str(), 1);
        } else {
            unsetenv(name_.c_str());
        }
    }

    ScopedEnvironment(const ScopedEnvironment&) = delete;
    ScopedEnvironment& operator=(const ScopedEnvironment&) = delete;

private:
    std::string name_;
    std::optional<std::string> previous_;
};

std::string byteHex(unsigned int value) {
    static constexpr char digits[] = "0123456789abcdef";
    std::string result(2, '0');
    result[0] = digits[(value >> 4) & 0x0f];
    result[1] = digits[value & 0x0f];
    return result;
}

std::string bytesHex(const std::string& value) {
    std::string result;
    result.reserve(value.size() * 2);
    for (const unsigned char byte : value) result += byteHex(byte);
    return result;
}

std::string idHex(unsigned int seed) {
    return byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed)
        + byteHex(seed) + byteHex(seed) + byteHex(seed) + byteHex(seed);
}

cpp_int decimal(const std::string& value) {
    cpp_int result = 0;
    for (const char digit : value) result = result * 10 + (digit - '0');
    return result;
}

std::string integerLe(cpp_int value, bool signed_value = false) {
    if (signed_value && value < 0) value += cpp_int(1) << 128;
    std::string result;
    result.reserve(32);
    for (int index = 0; index < 16; ++index) {
        const unsigned int byte = (value & 0xff).convert_to<unsigned int>();
        result += byteHex(byte);
        value >>= 8;
    }
    return result;
}

std::string u128Le(const std::string& value) {
    return integerLe(decimal(value));
}

std::string i128Le(const std::string& value) {
    const bool negative = !value.empty() && value.front() == '-';
    return integerLe(decimal(negative ? value.substr(1) : value), negative);
}

void appendU64(std::string& result, std::uint64_t value) {
    for (int index = 0; index < 8; ++index) {
        result += byteHex(static_cast<unsigned int>(value & 0xff));
        value >>= 8;
    }
}

std::string protocolData(bool frozen = false) {
    std::string result;
    for (unsigned int seed = 1; seed <= 5; ++seed) result += idHex(seed);
    result += u128Le((decimal(FIXED_ONE) + decimal("1500000000000000")).convert_to<std::string>());
    result += i128Le(FIXED_ONE);
    result += i128Le("10000000000000000000000000");
    result += u128Le("1500000000000000000000000000");
    appendU64(result, 300'000);
    appendU64(result, 900'000);
    result += frozen ? "01" : "00";
    return result;
}

std::string accumulatorData(const std::string& rate, std::uint64_t timestamp) {
    std::string result = u128Le(rate);
    appendU64(result, timestamp);
    return result;
}

std::string redemptionData(const std::string& price,
                           const std::string& rate,
                           const std::string& integral,
                           std::uint64_t timestamp) {
    std::string result = u128Le(price) + u128Le(rate) + i128Le(integral);
    appendU64(result, timestamp);
    return result;
}

std::string oracleData(const std::string& price, std::uint64_t timestamp) {
    std::string result = idHex(3) + idHex(4) + u128Le(price);
    appendU64(result, timestamp);
    result += idHex(6) + u128Le("0");
    return result;
}

std::string clockData(std::uint64_t timestamp) {
    std::string result;
    appendU64(result, 1);
    appendU64(result, timestamp);
    return result;
}

std::string collateralDefinitionData() {
    const std::string name = "Collateral";
    return std::string("00") + "0a000000" + bytesHex(name)
        + u128Le("340282366920938463463374607431768211455") + "00" + "00";
}

std::string collateralHoldingData(const std::string& balance) {
    return "00" + idHex(4) + u128Le(balance);
}

std::string mintDefinitionData(const std::string& definition_id,const std::string& supply) {
    return "00" + std::string("04000000") + bytesHex("Coin") + u128Le(supply) + "00" + "01" + definition_id;
}
std::string stablecoinHoldingData(const std::string& definition_id,const std::string& balance) {
    return "00" + definition_id + u128Le(balance);
}

std::string positionData(const std::string& owner_id,
                         const std::string& vault_id,
                         std::uint64_t position_nonce,
                         const std::string& collateral_amount,
                         const std::string& normalized_debt_amount) {
    std::string result = owner_id;
    appendU64(result, position_nonce);
    result += vault_id;
    result += u128Le(collateral_amount);
    result += u128Le(normalized_debt_amount);
    appendU64(result, START);
    return result;
}

std::string accountResponse(const std::string& owner, const std::string& data) {
    return json{
        {"program_owner", owner},
        {"balance", std::string(32, '0')},
        {"nonce", std::string(32, '0')},
        {"data", data},
    }.dump();
}

json accountReadValue(const std::string& account_id,
                      const std::string& owner,
                      const std::string& data) {
    return {
        {"id", account_id},
        {"status", "ok"},
        {"account", {
            {"program_owner", owner},
            {"balance", std::string(32, '0')},
            {"nonce", std::string(32, '0')},
            {"data", data},
        }},
    };
}

QVariantList accountArgs(const std::string& account_id) {
    return {QVariant(QString::fromStdString(account_id))};
}

void expectRead(const std::string& account_id,
                const std::string& owner,
                const std::string& data) {
    MockStore::instance()
        .when(QStringLiteral("lez_core"), QStringLiteral("get_account_public"))
        .withArgs(accountArgs(account_id))
        .thenReturn(QVariant(QString::fromStdString(accountResponse(owner, data))));
}

void expectMissing(const std::string& account_id) {
    MockStore::instance()
        .when(QStringLiteral("lez_core"), QStringLiteral("get_account_public"))
        .withArgs(accountArgs(account_id))
        .thenReturn(QVariant(QString()));
}

QVariantList walletAccounts() {
    QVariantMap owner;
    owner.insert("account_id", QString::fromStdString(CALLER_ID_HEX));
    owner.insert("is_public", true);
    QVariantMap holding;
    holding.insert("account_id", QString::fromStdString(idHex(8)));
    holding.insert("is_public", true);
    return {QVariant(owner), QVariant(holding)};
}

QVariantList submissionArguments(const std::vector<std::string>& account_ids,
                                 std::uint32_t instruction_word,
                                 const std::string& program_id) {
    QStringList ids;
    QVariantList signers;
    for (std::size_t index = 0; index < account_ids.size(); ++index) {
        ids.push_back(QString::fromStdString(account_ids[index]));
        signers.push_back(index == 0);
    }
    QByteArray instruction(1, static_cast<char>(instruction_word));
    instruction.append(3, '\0');
    return {
        QVariant(ids),
        QVariant(signers),
        QVariant(instruction),
        QVariant(QString::fromStdString(program_id)),
    };
}

QVariantList submissionArguments(const json& plan,
                                 const std::vector<std::size_t>& signer_indices,
                                 const std::string& program_id) {
    const std::vector<std::string> account_ids =
        plan.at("accountIds").get<std::vector<std::string>>();
    QStringList ids;
    QVariantList signers;
    for (std::size_t index = 0; index < account_ids.size(); ++index) {
        ids.push_back(QString::fromStdString(account_ids[index]));
        signers.push_back(std::find(signer_indices.begin(), signer_indices.end(), index)
                          != signer_indices.end());
    }
    QByteArray instruction;
    for (const auto& item : plan.at("instruction")) {
        const std::uint32_t word = item.get<std::uint32_t>();
        for (unsigned int shift = 0; shift < 32; shift += 8) {
            instruction.push_back(static_cast<char>((word >> shift) & 0xff));
        }
    }
    return {
        QVariant(ids),
        QVariant(signers),
        QVariant(instruction),
        QVariant(QString::fromStdString(program_id)),
    };
}

std::string successfulTransaction() {
    return json{{"success", true}, {"tx_hash", TRANSACTION_ID_HEX}}.dump();
}

void attachModules(StablecoinModuleImpl& module, LogosModules& modules) {
    module._logosCoreSetLogosModulesPtr_(&modules);
}

void assertOk(const LogosMap& response) {
    LOGOS_ASSERT_EQ(response["status"].get<std::string>(), std::string("ok"));
    LOGOS_ASSERT_EQ(response["error"].get<std::string>(), std::string());
}

}  // namespace

LOGOS_TEST(real_ffi_close_position_pins_submission_and_rejects_donations_and_repeat_close) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    for (const bool frozen : {false, true}) {
        LogosTestContext context("stablecoin_module");
        LogosModules modules(context.api());
        StablecoinModuleImpl module;
        attachModules(module, modules);
        const LogosMap info = module.programInfo();
        assertOk(info);
        const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
        const std::string address_payload = json{
            {"stablecoinProgramId", PROGRAM_ID_HEX}, {"ownerId", CALLER_ID_HEX},
            {"positionNonce", "18446744073709551615"},
        }.dump();
        std::unique_ptr<char, decltype(&stablecoin_free)> addresses(
            stablecoin_position_addresses(address_payload.c_str()), &stablecoin_free);
        LOGOS_ASSERT_TRUE(addresses != nullptr);
        const json address_envelope = json::parse(addresses.get());
        LOGOS_ASSERT_TRUE(address_envelope["ok"].get<bool>());
        const std::string position_id = address_envelope["value"]["positionIdHex"];
        const std::string vault_id = address_envelope["value"]["vaultIdHex"];
        const std::string position = positionData(CALLER_ID_HEX, vault_id, UINT64_MAX, "0", "0");
        std::string parameters = protocolData(frozen);
        parameters.replace(128, 64, info["stablecoinDefinitionIdHex"].get<std::string>());
        expectRead(protocol_id, PROGRAM_OWNER_HEX, parameters);
        expectRead(position_id, PROGRAM_OWNER_HEX, position);
        expectRead(vault_id, TOKEN_OWNER_HEX, collateralHoldingData("1"));
        QVariantMap owner;
        owner.insert("account_id", QString::fromStdString(CALLER_ID_HEX));
        owner.insert("is_public", true);
        context.mockModule("lez_core", "list_accounts")
            .returnsVariant(QVariant(QVariantList{QVariant(owner)}));
        context.mockModule("lez_core", "send_generic_public_transaction").returns(successfulTransaction());
        LogosMap request = {{"ownerId", CALLER_ID_HEX}, {"positionNonce", "18446744073709551615"}};
        const LogosMap donated = module.closePosition(request);
        LOGOS_ASSERT_EQ(donated["error"].get<std::string>(), std::string("vault_not_empty"));
        LOGOS_ASSERT_TRUE(donated.find("transactionId") == donated.end());
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);

        // Only a separately emptied vault permits closure; closePosition itself
        // does not reconcile or withdraw the donation.
        const std::string vault = collateralHoldingData("0");
        expectRead(vault_id, TOKEN_OWNER_HEX, vault);
        const LogosMap closed = module.closePosition(request);
        assertOk(closed);
        LOGOS_ASSERT_EQ(closed["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 6);
        const std::string planner_payload = json{
            {"stablecoinProgramId", PROGRAM_ID_HEX}, {"ownerId", CALLER_ID_HEX},
            {"positionNonce", "18446744073709551615"},
            {"position", accountReadValue(position_id, PROGRAM_OWNER_HEX, position)},
            {"vault", accountReadValue(vault_id, TOKEN_OWNER_HEX, vault)},
            {"protocolParameters", accountReadValue(protocol_id, PROGRAM_OWNER_HEX, parameters)},
        }.dump();
        std::unique_ptr<char, decltype(&stablecoin_free)> planned(
            stablecoin_close_position_plan(planner_payload.c_str()), &stablecoin_free);
        LOGOS_ASSERT_TRUE(planned != nullptr);
        const json envelope = json::parse(planned.get());
        LOGOS_ASSERT_TRUE(envelope["ok"].get<bool>());
        const json plan = envelope["value"];
        LOGOS_ASSERT_EQ(plan["accountIds"], json::array({CALLER_ID_HEX, position_id, vault_id, protocol_id}));
        LOGOS_ASSERT_EQ(plan["signingRequirements"], json::array({true, false, false, false}));
        LOGOS_ASSERT_EQ(plan["instruction"].size(), 1);
        LOGOS_ASSERT_TRUE(context.moduleCalledWith("lez_core", "send_generic_public_transaction",
            submissionArguments(plan, std::vector<std::size_t>{0}, PROGRAM_ID_HEX)));

        // Keep the stablecoin owner and nonzero account nonce after clearing
        // data. This is not a missing or default account.
        json cleared = json::parse(accountResponse(PROGRAM_OWNER_HEX, ""));
        cleared["nonce"] = u128Le("7");
        MockStore::instance().when(QStringLiteral("lez_core"), QStringLiteral("get_account_public"))
            .withArgs(accountArgs(position_id))
            .thenReturn(QVariant(QString::fromStdString(cleared.dump())));
        const LogosMap repeated = module.closePosition(request);
        LOGOS_ASSERT_EQ(repeated["error"].get<std::string>(), std::string("invalid_position_data"));
        LOGOS_ASSERT_TRUE(repeated.find("transactionId") == repeated.end());
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
        request["positionNonce"] = 1.5;
        LOGOS_ASSERT_EQ(module.closePosition(request)["error"].get<std::string>(), std::string("bad_request"));
        request["positionNonce"] = "18446744073709551616";
        LOGOS_ASSERT_EQ(module.closePosition(request)["error"].get<std::string>(), std::string("invalid_numeric_value"));
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);

        expectRead(position_id, PROGRAM_OWNER_HEX, position);
        request["positionNonce"] = "18446744073709551615";
        expectMissing(vault_id);
        LOGOS_ASSERT_EQ(module.closePosition(request)["error"].get<std::string>(), std::string("account_read_failed"));
        expectRead(vault_id, TOKEN_OWNER_HEX, vault);
        expectMissing(position_id);
        LOGOS_ASSERT_EQ(module.closePosition(request)["error"].get<std::string>(), std::string("account_read_failed"));
        LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
    }
}

LOGOS_TEST(real_ffi_journey_reads_quotes_submits_and_rereads_state) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string accumulator_id = info["stabilityFeeAccumulatorIdHex"].get<std::string>();
    const std::string redemption_id = info["redemptionPriceStateIdHex"].get<std::string>();
    const std::string clock_id = info["clockIdHex"].get<std::string>();
    const std::string oracle_id = idHex(5);

    // The same module first observes an uninitialized protocol.
    expectMissing(protocol_id);
    expectMissing(accumulator_id);
    expectMissing(redemption_id);
    expectMissing(clock_id);
    LOGOS_ASSERT_EQ(module.protocolParameters()["error"].get<std::string>(),
                    std::string("not_initialized"));
    LOGOS_ASSERT_EQ(module.stabilityFeeAccumulator()["error"].get<std::string>(),
                    std::string("not_initialized"));
    LOGOS_ASSERT_EQ(module.redemptionPriceState()["error"].get<std::string>(),
                    std::string("not_initialized"));
    LOGOS_ASSERT_EQ(module.currentGlobalState()["error"].get<std::string>(),
                    std::string("not_initialized"));
    LOGOS_ASSERT_EQ(module.redemptionRateUpdateQuote()["error"].get<std::string>(),
                    std::string("not_initialized"));

    const std::string protocol = protocolData();
    const std::string accumulator = accumulatorData(FIXED_ONE, START);
    const std::string redemption = redemptionData(FIXED_ONE, FIXED_ONE, "0", START);
    const std::string oracle = oracleData("500000000000000000000000000", DUE);
    const std::string clock = clockData(DUE);
    expectRead(protocol_id, PROGRAM_OWNER_HEX, protocol);
    expectRead(accumulator_id, PROGRAM_OWNER_HEX, accumulator);
    expectRead(redemption_id, PROGRAM_OWNER_HEX, redemption);
    expectRead(oracle_id, ORACLE_OWNER_HEX, oracle);
    expectRead(clock_id, PROGRAM_OWNER_HEX, clock);
    context.mockModule("lez_core", "list_accounts")
        .returnsVariant(QVariant(walletAccounts()));
    context.mockModule("lez_core", "send_generic_public_transaction")
        .returns(successfulTransaction());

    const LogosMap parameters = module.protocolParameters();
    assertOk(parameters);
    LOGOS_ASSERT_EQ(parameters["protocolParameters"]["marketPriceOracleIdHex"].get<std::string>(),
                    oracle_id);
    const LogosMap accumulator_snapshot = module.stabilityFeeAccumulator();
    assertOk(accumulator_snapshot);
    LOGOS_ASSERT_EQ(
        accumulator_snapshot["stabilityFeeAccumulator"]["lastAccruedAt"].get<std::string>(),
        std::string("1000"));
    const LogosMap redemption_snapshot = module.redemptionPriceState();
    assertOk(redemption_snapshot);
    LOGOS_ASSERT_EQ(
        redemption_snapshot["redemptionPriceState"]["lastUpdatedAt"].get<std::string>(),
        std::string("1000"));

    const LogosMap projected = module.currentGlobalState();
    assertOk(projected);
    LOGOS_ASSERT_EQ(projected["currentGlobalState"]["projectedAt"].get<std::string>(),
                    std::string("601000"));
    const std::string projected_accumulator =
        projected["currentGlobalState"]["currentAccumulatedRate"].get<std::string>();

    const LogosMap quote = module.redemptionRateUpdateQuote();
    assertOk(quote);
    LOGOS_ASSERT_EQ(quote["canSubmit"].get<bool>(), true);
    LOGOS_ASSERT_EQ(quote["code"].get<std::string>(), std::string("ready"));
    LOGOS_ASSERT_EQ(quote["elapsedMilliseconds"].get<std::string>(), std::string("600000"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);

    const LogosMap accrued = module.accrueStabilityFee(CALLER_ID_HEX);
    assertOk(accrued);
    LOGOS_ASSERT_EQ(accrued["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(
            {CALLER_ID_HEX, protocol_id, accumulator_id, clock_id}, 1, PROGRAM_ID_HEX)));

    // Apply the real FFI projection as the persisted post-state seen by the next read.
    expectRead(accumulator_id, PROGRAM_OWNER_HEX,
               accumulatorData(projected_accumulator, DUE));
    const LogosMap persisted_accumulator = module.stabilityFeeAccumulator();
    assertOk(persisted_accumulator);
    LOGOS_ASSERT_EQ(
        persisted_accumulator["stabilityFeeAccumulator"]["accumulatedRateAtLastAccrual"]
            .get<std::string>(),
        projected_accumulator);
    LOGOS_ASSERT_EQ(
        persisted_accumulator["stabilityFeeAccumulator"]["lastAccruedAt"].get<std::string>(),
        std::string("601000"));

    const LogosMap updated = module.updateRedemptionRate(CALLER_ID_HEX);
    assertOk(updated);
    LOGOS_ASSERT_EQ(updated["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(
            {CALLER_ID_HEX, protocol_id, redemption_id, oracle_id, clock_id},
            2,
            PROGRAM_ID_HEX)));

    expectRead(redemption_id, PROGRAM_OWNER_HEX,
               redemptionData(quote["currentRedemptionPrice"].get<std::string>(),
                              quote["nextRedemptionRatePerMillisecond"].get<std::string>(),
                              quote["nextControllerIntegralTerm"].get<std::string>(),
                              DUE));
    const LogosMap persisted_redemption = module.redemptionPriceState();
    assertOk(persisted_redemption);
    LOGOS_ASSERT_EQ(
        persisted_redemption["redemptionPriceState"]["lastUpdatedAt"].get<std::string>(),
        std::string("601000"));
    LOGOS_ASSERT_EQ(
        persisted_redemption["redemptionPriceState"]["redemptionRatePerMillisecond"]
            .get<std::string>(),
        quote["nextRedemptionRatePerMillisecond"].get<std::string>());

    const LogosMap refreshed = module.refreshGlobals(CALLER_ID_HEX);
    assertOk(refreshed);
    LOGOS_ASSERT_EQ(refreshed["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(
            {CALLER_ID_HEX, protocol_id, accumulator_id, redemption_id, oracle_id, clock_id},
            3,
            PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 3);
}

LOGOS_TEST(real_ffi_journey_opens_position_with_two_wallet_signers) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string clock_id = info["clockIdHex"].get<std::string>();
    const std::string collateral_id = idHex(4);
    const std::string holding_id = idHex(8);
    const std::string protocol = protocolData();
    const std::string clock = clockData(DUE);
    const std::string position_nonce = "18446744073709551615";
    const std::string collateral_amount = "340282366920938463463374607431768211455";
    const json planner_request = {
        {"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", CALLER_ID_HEX},
        {"positionNonce", position_nonce},
        {"initialCollateralAmount", collateral_amount},
        {"userCollateralHoldingId", holding_id},
        {"userCollateralHolding",
         accountReadValue(
             holding_id,
             TOKEN_OWNER_HEX,
             collateralHoldingData("340282366920938463463374607431768211455"))},
        {"collateralDefinition",
         accountReadValue(collateral_id, TOKEN_OWNER_HEX, collateralDefinitionData())},
        {"protocolParameters", accountReadValue(protocol_id, PROGRAM_OWNER_HEX, protocol)},
        {"clock", accountReadValue(clock_id, PROGRAM_OWNER_HEX, clock)},
    };
    const std::string planner_payload = planner_request.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> planner_result(
        stablecoin_open_position_plan(planner_payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(planner_result != nullptr);
    const json planner_envelope = json::parse(planner_result.get());
    LOGOS_ASSERT_TRUE(planner_envelope["ok"].get<bool>());
    const json plan = planner_envelope["value"];
    const std::vector<std::string> account_ids =
        plan.at("accountIds").get<std::vector<std::string>>();

    expectRead(protocol_id, PROGRAM_OWNER_HEX, protocol);
    expectRead(
        holding_id,
        TOKEN_OWNER_HEX,
        collateralHoldingData("340282366920938463463374607431768211455"));
    expectRead(collateral_id, TOKEN_OWNER_HEX, collateralDefinitionData());
    expectRead(clock_id, PROGRAM_OWNER_HEX, clock);
    expectMissing(account_ids[1]);
    expectMissing(account_ids[2]);
    context.mockModule("lez_core", "list_accounts")
        .returnsVariant(QVariant(walletAccounts()));
    context.mockModule("lez_core", "send_generic_public_transaction")
        .returns(successfulTransaction());

    const LogosMap opened = module.openPosition({
        {"ownerId", CALLER_ID_HEX},
        {"positionNonce", position_nonce},
        {"initialCollateralAmount", collateral_amount},
        {"userCollateralHoldingId", holding_id},
    });

    assertOk(opened);
    LOGOS_ASSERT_EQ(opened["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    LOGOS_ASSERT_EQ(plan["signingRequirements"],
                    json::array({true, false, false, true, false, false, false}));
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(plan, {0, 3}, PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
}

LOGOS_TEST(real_ffi_borrowing_rechecks_oracle_binding_and_preserves_full_width_amounts) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID",PROGRAM_ID_HEX.c_str());ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN",nullptr);
    LogosTestContext context("stablecoin_module");LogosModules modules(context.api());StablecoinModuleImpl module;attachModules(module,modules);
    const LogosMap info=module.programInfo();assertOk(info);
    const std::string protocol_id=info["protocolParametersIdHex"].get<std::string>();
    const std::string accumulator_id=info["stabilityFeeAccumulatorIdHex"].get<std::string>();
    const std::string redemption_id=info["redemptionPriceStateIdHex"].get<std::string>();
    const std::string definition_id=info["stablecoinDefinitionIdHex"].get<std::string>();
    const std::string clock_id=info["clockIdHex"].get<std::string>();
    const std::string destination_id=idHex(8),oracle_id=idHex(5),rotated_oracle_id=idHex(10);
    const std::string nonce="18446744073709551615";
    const std::string address_payload=json{{"stablecoinProgramId",PROGRAM_ID_HEX},{"ownerId",CALLER_ID_HEX},{"positionNonce",nonce}}.dump();
    std::unique_ptr<char,decltype(&stablecoin_free)> addresses_result(stablecoin_position_addresses(address_payload.c_str()),&stablecoin_free);
    LOGOS_ASSERT_TRUE(addresses_result!=nullptr);
    const json addresses=json::parse(addresses_result.get()).at("value");
    const std::string position_id=addresses["positionIdHex"].get<std::string>(),vault_id=addresses["vaultIdHex"].get<std::string>();
    std::string parameters=protocolData(false);parameters.replace(128,64,definition_id);
    const std::string position=positionData(CALLER_ID_HEX,vault_id,UINT64_MAX,"10","0");
    const std::string definition=mintDefinitionData(definition_id,"100"),destination=stablecoinHoldingData(definition_id,"5");
    const std::string accumulator=accumulatorData("1500000000000000000000000000",DUE);
    const std::string redemption=redemptionData(FIXED_ONE,FIXED_ONE,"0",DUE),clock=clockData(DUE);
    std::string oracle=oracleData("0",DUE);oracle.replace(0,64,definition_id);
    expectRead(protocol_id,PROGRAM_OWNER_HEX,parameters);expectRead(position_id,PROGRAM_OWNER_HEX,position);
    expectRead(definition_id,TOKEN_OWNER_HEX,definition);expectRead(destination_id,TOKEN_OWNER_HEX,destination);
    expectRead(accumulator_id,PROGRAM_OWNER_HEX,accumulator);expectRead(redemption_id,PROGRAM_OWNER_HEX,redemption);
    expectRead(oracle_id,ORACLE_OWNER_HEX,oracle);expectRead(clock_id,PROGRAM_OWNER_HEX,clock);
    QVariantMap owner;owner.insert("account_id",QString::fromStdString(CALLER_ID_HEX));owner.insert("is_public",true);
    context.mockModule("lez_core","list_accounts").returnsVariant(QVariant(QVariantList{QVariant(owner)}));
    context.mockModule("lez_core","send_generic_public_transaction").returns(successfulTransaction());
    LogosMap request={{"ownerId",CALLER_ID_HEX},{"positionNonce",nonce},{"userStablecoinHoldingId",destination_id},{"amount","4"}};
    const LogosMap borrowed=module.generateDebt(request);assertOk(borrowed);
    LOGOS_ASSERT_EQ(borrowed["transactionId"].get<std::string>(),TRANSACTION_ID_HEX);
    const std::string planner_payload=json{
        {"stablecoinProgramId",PROGRAM_ID_HEX},{"ownerId",CALLER_ID_HEX},{"positionNonce",nonce},{"amount","4"},
        {"userStablecoinHoldingId",destination_id},{"position",accountReadValue(position_id,PROGRAM_OWNER_HEX,position)},
        {"stablecoinDefinition",accountReadValue(definition_id,TOKEN_OWNER_HEX,definition)},
        {"userStablecoinHolding",accountReadValue(destination_id,TOKEN_OWNER_HEX,destination)},
        {"stabilityFeeAccumulator",accountReadValue(accumulator_id,PROGRAM_OWNER_HEX,accumulator)},
        {"redemptionPriceState",accountReadValue(redemption_id,PROGRAM_OWNER_HEX,redemption)},
        {"marketPriceOracle",accountReadValue(oracle_id,ORACLE_OWNER_HEX,oracle)},
        {"protocolParameters",accountReadValue(protocol_id,PROGRAM_OWNER_HEX,parameters)},
        {"clock",accountReadValue(clock_id,PROGRAM_OWNER_HEX,clock)},
    }.dump();
    std::unique_ptr<char,decltype(&stablecoin_free)> planned(stablecoin_generate_debt_plan(planner_payload.c_str()),&stablecoin_free);
    LOGOS_ASSERT_TRUE(planned!=nullptr);const json plan=json::parse(planned.get()).at("value");
    LOGOS_ASSERT_TRUE(context.moduleCalledWith("lez_core","send_generic_public_transaction",
        submissionArguments(plan,std::vector<std::size_t>{0},PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(plan["signingRequirements"],json::array({true,false,false,false,false,false,false,false,false}));

    // Carry the first mint forward, then rotate the configured oracle. The old
    // fresh oracle must not authorize borrowing after the rotation.
    expectRead(position_id,PROGRAM_OWNER_HEX,positionData(CALLER_ID_HEX,vault_id,UINT64_MAX,"10","3"));
    expectRead(definition_id,TOKEN_OWNER_HEX,mintDefinitionData(definition_id,"104"));
    expectRead(destination_id,TOKEN_OWNER_HEX,stablecoinHoldingData(definition_id,"9"));
    parameters.replace(256,64,rotated_oracle_id);expectRead(protocol_id,PROGRAM_OWNER_HEX,parameters);
    std::string stale=oracleData("0",DUE);stale.replace(0,64,definition_id);
    // DUE is below the normal age limit, so use a canonical clock later than
    // the observation to cross the exact freshness boundary.
    expectRead(clock_id,PROGRAM_OWNER_HEX,clockData(DUE+900'001));
    expectRead(rotated_oracle_id,ORACLE_OWNER_HEX,stale);
    request["amount"]="1";
    LOGOS_ASSERT_EQ(module.generateDebt(request)["error"].get<std::string>(),std::string("oracle_stale"));
    std::string future=oracleData("0",DUE+900'002);future.replace(0,64,definition_id);
    expectRead(rotated_oracle_id,ORACLE_OWNER_HEX,future);
    LOGOS_ASSERT_EQ(module.generateDebt(request)["error"].get<std::string>(),std::string("oracle_future"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),1);

    const std::uint64_t later=DUE+1'000'000;
    const std::string maximum="340282366920938463463374607431768211455";
    std::string repaired=oracleData("0",later);repaired.replace(0,64,definition_id);
    expectRead(rotated_oracle_id,ORACLE_OWNER_HEX,repaired);
    expectRead(clock_id,PROGRAM_OWNER_HEX,clockData(later));
    expectRead(position_id,PROGRAM_OWNER_HEX,positionData(CALLER_ID_HEX,vault_id,UINT64_MAX,maximum,"0"));
    expectRead(definition_id,TOKEN_OWNER_HEX,mintDefinitionData(definition_id,"0"));
    expectRead(destination_id,TOKEN_OWNER_HEX,stablecoinHoldingData(definition_id,"0"));
    expectRead(accumulator_id,PROGRAM_OWNER_HEX,accumulatorData("1500000000000000000000000000",later));
    expectRead(redemption_id,PROGRAM_OWNER_HEX,redemptionData("500000000000000000000000000",FIXED_ONE,"0",later));
    request["amount"]=maximum;assertOk(module.generateDebt(request));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),2);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","get_account_public"),32);
    request["amount"]=1.5;
    LOGOS_ASSERT_EQ(module.generateDebt(request)["error"].get<std::string>(),std::string("invalid_numeric_value"));
    request["amount"]="340282366920938463463374607431768211456";
    LOGOS_ASSERT_EQ(module.generateDebt(request)["error"].get<std::string>(),std::string("invalid_numeric_value"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),2);
}

LOGOS_TEST(real_ffi_withdrawal_rechecks_health_and_accepts_external_destination) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);
    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string accumulator_id = info["stabilityFeeAccumulatorIdHex"].get<std::string>();
    const std::string redemption_id = info["redemptionPriceStateIdHex"].get<std::string>();
    const std::string clock_id = info["clockIdHex"].get<std::string>();
    const std::string destination_id = idHex(8);
    const std::string nonce = "18446744073709551615";
    const std::string payload = json{{"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", CALLER_ID_HEX}, {"positionNonce", nonce}}.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> addresses_result(
        stablecoin_position_addresses(payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(addresses_result != nullptr);
    const json addresses = json::parse(addresses_result.get()).at("value");
    const std::string position_id = addresses["positionIdHex"].get<std::string>();
    const std::string vault_id = addresses["vaultIdHex"].get<std::string>();
    std::string parameters = protocolData(false);
    parameters.replace(128, 64, info["stablecoinDefinitionIdHex"].get<std::string>());
    const std::string position = positionData(CALLER_ID_HEX, vault_id, UINT64_MAX, "180", "100");
    const std::string vault = collateralHoldingData("190");
    const std::string destination = collateralHoldingData("10");
    const std::string accumulator = accumulatorData(FIXED_ONE, DUE);
    const std::string redemption = redemptionData(FIXED_ONE, FIXED_ONE, "0", DUE);
    const std::string clock = clockData(DUE);
    expectRead(position_id, PROGRAM_OWNER_HEX, position);
    expectRead(vault_id, TOKEN_OWNER_HEX, vault);
    expectRead(destination_id, TOKEN_OWNER_HEX, destination);
    expectRead(protocol_id, PROGRAM_OWNER_HEX, parameters);
    expectRead(accumulator_id, PROGRAM_OWNER_HEX, accumulator);
    expectRead(redemption_id, PROGRAM_OWNER_HEX, redemption);
    expectRead(clock_id, PROGRAM_OWNER_HEX, clock);
    QVariantMap owner;
    owner.insert("account_id", QString::fromStdString(CALLER_ID_HEX));
    owner.insert("is_public", true);
    context.mockModule("lez_core", "list_accounts").returnsVariant(QVariant(QVariantList{QVariant(owner)}));
    context.mockModule("lez_core", "send_generic_public_transaction").returns(successfulTransaction());
    const LogosMap earlier_quote = module.positionHealth({{"ownerId", CALLER_ID_HEX}, {"positionNonce", nonce}});
    assertOk(earlier_quote);
    LOGOS_ASSERT_TRUE(earlier_quote["isCollateralized"].get<bool>());
    LogosMap request = {{"ownerId", CALLER_ID_HEX}, {"positionNonce", nonce},
        {"userCollateralHoldingId", destination_id}, {"amount", "30"}};
    const LogosMap withdrawn = module.withdrawCollateral(request);
    assertOk(withdrawn);
    LOGOS_ASSERT_EQ(withdrawn["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    const std::string planner_payload = json{
        {"stablecoinProgramId",PROGRAM_ID_HEX},{"ownerId",CALLER_ID_HEX},{"positionNonce",nonce},{"amount","30"},
        {"userCollateralHoldingId",destination_id},{"position",accountReadValue(position_id,PROGRAM_OWNER_HEX,position)},
        {"vault",accountReadValue(vault_id,TOKEN_OWNER_HEX,vault)},
        {"userCollateralHolding",accountReadValue(destination_id,TOKEN_OWNER_HEX,destination)},
        {"stabilityFeeAccumulator",accountReadValue(accumulator_id,PROGRAM_OWNER_HEX,accumulator)},
        {"redemptionPriceState",accountReadValue(redemption_id,PROGRAM_OWNER_HEX,redemption)},
        {"protocolParameters",accountReadValue(protocol_id,PROGRAM_OWNER_HEX,parameters)},
        {"clock",accountReadValue(clock_id,PROGRAM_OWNER_HEX,clock)},
    }.dump();
    std::unique_ptr<char,decltype(&stablecoin_free)> planned(stablecoin_withdraw_collateral_plan(planner_payload.c_str()),&stablecoin_free);
    LOGOS_ASSERT_TRUE(planned != nullptr);
    const json plan = json::parse(planned.get()).at("value");
    LOGOS_ASSERT_TRUE(context.moduleCalledWith("lez_core","send_generic_public_transaction", submissionArguments(plan,std::vector<std::size_t>{0},PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(plan["signingRequirements"], json::array({true,false,false,false,false,false,false,false}));

    // Feed the first withdrawal's state forward, then accrue fees. An earlier
    // healthy quote cannot authorize a withdrawal against this new state.
    expectRead(position_id,PROGRAM_OWNER_HEX,positionData(CALLER_ID_HEX,vault_id,UINT64_MAX,"150","100"));
    expectRead(vault_id,TOKEN_OWNER_HEX,collateralHoldingData("160"));
    expectRead(destination_id,TOKEN_OWNER_HEX,collateralHoldingData("40"));
    expectRead(accumulator_id,PROGRAM_OWNER_HEX,accumulatorData("2000000000000000000000000000",DUE));
    request["amount"] = "1";
    const LogosMap rejected = module.withdrawCollateral(request);
    LOGOS_ASSERT_EQ(rejected["error"].get<std::string>(), std::string("position_undercollateralized"));
    LOGOS_ASSERT_TRUE(rejected.find("transactionId") == rejected.end());
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),1);

    const std::string maximum = "340282366920938463463374607431768211455";
    expectRead(position_id,PROGRAM_OWNER_HEX,positionData(CALLER_ID_HEX,vault_id,UINT64_MAX,maximum,"0"));
    expectRead(vault_id,TOKEN_OWNER_HEX,collateralHoldingData(maximum));
    expectRead(destination_id,TOKEN_OWNER_HEX,collateralHoldingData("0"));
    expectRead(clock_id,PROGRAM_OWNER_HEX,clockData(DUE + 60'000'000));
    parameters.replace(320,32,u128Le("2000000000000000000000000000"));
    expectRead(protocol_id,PROGRAM_OWNER_HEX,parameters);
    request["amount"] = maximum;
    assertOk(module.withdrawCollateral(request)); // Zero debt skips even enormous fee projections.
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),2);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","get_account_public"),26);
    request["amount"] = 1.5;
    LOGOS_ASSERT_EQ(module.withdrawCollateral(request)["error"].get<std::string>(),std::string("invalid_numeric_value"));
    request["amount"] = "340282366920938463463374607431768211456";
    LOGOS_ASSERT_EQ(module.withdrawCollateral(request)["error"].get<std::string>(),std::string("invalid_numeric_value"));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core","send_generic_public_transaction"),2);
}

LOGOS_TEST(real_ffi_position_health_rechecks_fractional_debt_while_frozen) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);
    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string accumulator_id = info["stabilityFeeAccumulatorIdHex"].get<std::string>();
    const std::string redemption_id = info["redemptionPriceStateIdHex"].get<std::string>();
    const std::string clock_id = info["clockIdHex"].get<std::string>();
    const std::string nonce = "18446744073709551615";
    const std::string payload = json{{"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", CALLER_ID_HEX}, {"positionNonce", nonce}}.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> addresses_result(
        stablecoin_position_addresses(payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(addresses_result != nullptr);
    const json addresses = json::parse(addresses_result.get()).at("value");
    const std::string position_id = addresses["positionIdHex"].get<std::string>();
    const std::string vault_id = addresses["vaultIdHex"].get<std::string>();
    expectRead(position_id, PROGRAM_OWNER_HEX, positionData(CALLER_ID_HEX, vault_id, UINT64_MAX, "3", "2"));
    std::string parameters = protocolData(true);
    parameters.replace(128, 64, info["stablecoinDefinitionIdHex"].get<std::string>());
    expectRead(protocol_id, PROGRAM_OWNER_HEX, parameters);
    expectRead(accumulator_id, PROGRAM_OWNER_HEX, accumulatorData(FIXED_ONE, DUE));
    expectRead(redemption_id, PROGRAM_OWNER_HEX, redemptionData(FIXED_ONE, FIXED_ONE, "0", DUE));
    expectRead(clock_id, PROGRAM_OWNER_HEX, clockData(DUE));
    const LogosMap request = {{"ownerId", CALLER_ID_HEX}, {"positionNonce", nonce}};
    const LogosMap healthy = module.positionHealth(request);
    assertOk(healthy);
    LOGOS_ASSERT_TRUE(healthy["isCollateralized"].get<bool>());
    LOGOS_ASSERT_EQ(healthy["positionNonce"].get<std::string>(), nonce);
    LOGOS_ASSERT_EQ(healthy["collateralValue"], healthy["requiredCollateralValue"]);

    expectRead(position_id, PROGRAM_OWNER_HEX, positionData(CALLER_ID_HEX, vault_id, UINT64_MAX, "2", "1"));
    expectRead(accumulator_id, PROGRAM_OWNER_HEX, accumulatorData("1900000000000000000000000000", DUE));
    const LogosMap fractional = module.positionHealth(request);
    assertOk(fractional);
    LOGOS_ASSERT_EQ(fractional["nominalDebt"].get<std::string>(), std::string("1"));
    LOGOS_ASSERT_TRUE(!fractional["isCollateralized"].get<bool>());
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 10);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 0);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);

    const std::string maximum = "340282366920938463463374607431768211455";
    expectRead(position_id, PROGRAM_OWNER_HEX, positionData(CALLER_ID_HEX, vault_id, UINT64_MAX, maximum, maximum));
    const LogosMap wide = module.positionHealth(request);
    assertOk(wide);
    LOGOS_ASSERT_EQ(wide["collateralAmount"].get<std::string>(), maximum);
    LOGOS_ASSERT_EQ(wide["normalizedDebtAmount"].get<std::string>(), maximum);
    LOGOS_ASSERT_EQ(wide["nominalDebt"].get<std::string>(), (decimal(maximum) * 19 / 10).convert_to<std::string>());
    LOGOS_ASSERT_EQ(wide["collateralValue"].get<std::string>(),
        (decimal(maximum) * decimal(FIXED_ONE) * decimal(FIXED_ONE) * decimal(FIXED_ONE)).convert_to<std::string>());
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 15);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "list_accounts"), 0);
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 0);

    expectMissing(position_id);
    const LogosMap absent = module.positionHealth(request);
    LOGOS_ASSERT_EQ(absent["error"].get<std::string>(), std::string("position_not_found"));
    LOGOS_ASSERT_EQ(absent["positionIdHex"].get<std::string>(), position_id);
    LOGOS_ASSERT_EQ(absent["vaultIdHex"].get<std::string>(), vault_id);
    LogosMap invalid = request;
    invalid["positionNonce"] = "18446744073709551616";
    LOGOS_ASSERT_EQ(module.positionHealth(invalid)["error"].get<std::string>(), std::string("invalid_numeric_value"));
}

LOGOS_TEST(real_ffi_repay_debt_preserves_large_amounts_and_frozen_preflight) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string accumulator_id = info["stabilityFeeAccumulatorIdHex"].get<std::string>();
    const std::string clock_id = info["clockIdHex"].get<std::string>();
    const std::string definition_id = idHex(3);
    const std::string holding_id = idHex(8);
    const std::string maximum = "340282366920938463463374607431768211455";
    const std::string address_payload = json{{"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", CALLER_ID_HEX}, {"positionNonce", "7"}}.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> address_result(
        stablecoin_position_addresses(address_payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(address_result != nullptr);
    const json addresses = json::parse(address_result.get()).at("value");
    const std::string position_id = addresses["positionIdHex"].get<std::string>();
    const std::string vault_id = addresses["vaultIdHex"].get<std::string>();
    const std::string definition = "00" + std::string("04000000") + bytesHex("Coin")
        + u128Le(maximum) + "00" + "00";
    const std::string holding = "00" + definition_id + u128Le(maximum);
    const std::string position = positionData(CALLER_ID_HEX, vault_id, 7, "1000", maximum);
    const std::string protocol = protocolData(true);
    const std::string accumulator = accumulatorData("1500000000000000000000000000", DUE);
    const std::string clock = clockData(DUE);
    expectRead(protocol_id, PROGRAM_OWNER_HEX, protocol);
    expectRead(position_id, PROGRAM_OWNER_HEX, position);
    expectRead(definition_id, TOKEN_OWNER_HEX, definition);
    expectRead(holding_id, TOKEN_OWNER_HEX, holding);
    expectRead(accumulator_id, PROGRAM_OWNER_HEX, accumulator);
    expectRead(clock_id, PROGRAM_OWNER_HEX, clock);
    context.mockModule("lez_core", "list_accounts").returnsVariant(QVariant(walletAccounts()));
    context.mockModule("lez_core", "send_generic_public_transaction").returns(successfulTransaction());

    LogosMap request = {{"ownerId", CALLER_ID_HEX}, {"positionNonce", "7"},
                       {"userStablecoinHoldingId", holding_id}, {"amount", maximum}};
    const LogosMap repaid = module.repayDebt(request);
    assertOk(repaid);
    LOGOS_ASSERT_EQ(repaid["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    const json planner_request = {
        {"stablecoinProgramId", PROGRAM_ID_HEX}, {"ownerId", CALLER_ID_HEX}, {"positionNonce", "7"},
        {"userStablecoinHoldingId", holding_id}, {"amount", maximum},
        {"position", accountReadValue(position_id, PROGRAM_OWNER_HEX, position)},
        {"stablecoinDefinition", accountReadValue(definition_id, TOKEN_OWNER_HEX, definition)},
        {"userStablecoinHolding", accountReadValue(holding_id, TOKEN_OWNER_HEX, holding)},
        {"stabilityFeeAccumulator", accountReadValue(accumulator_id, PROGRAM_OWNER_HEX, accumulator)},
        {"protocolParameters", accountReadValue(protocol_id, PROGRAM_OWNER_HEX, protocol)},
        {"clock", accountReadValue(clock_id, PROGRAM_OWNER_HEX, clock)},
    };
    const std::string payload = planner_request.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> planned(
        stablecoin_repay_debt_plan(payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(planned != nullptr);
    const json plan = json::parse(planned.get()).at("value");
    LOGOS_ASSERT_TRUE(context.moduleCalledWith("lez_core", "send_generic_public_transaction",
        submissionArguments(plan, {0, 3}, PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "get_account_public"), 6);

    // Each invocation reads current state again; neither earlier success nor
    // the frozen protocol bypasses request parsing and preflight validation.
    for (const json& amount : {json(1.5), json(1.0), json(-1), json("340282366920938463463374607431768211456")}) {
        request["amount"] = amount;
        const LogosMap rejected = module.repayDebt(request);
        LOGOS_ASSERT_EQ(rejected["error"].get<std::string>(), std::string("invalid_numeric_value"));
        LOGOS_ASSERT_TRUE(rejected.find("transactionId") == rejected.end());
    }
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
}

LOGOS_TEST(real_ffi_journey_deposit_collateral_reconciles_frozen_vault_donation) {
    ScopedEnvironment program_id("STABLECOIN_PROGRAM_ID", PROGRAM_ID_HEX.c_str());
    ScopedEnvironment program_binary("STABLECOIN_PROGRAM_BIN", nullptr);
    LogosTestContext context("stablecoin_module");
    LogosModules modules(context.api());
    StablecoinModuleImpl module;
    attachModules(module, modules);

    const LogosMap info = module.programInfo();
    assertOk(info);
    const std::string protocol_id = info["protocolParametersIdHex"].get<std::string>();
    const std::string owner_id = CALLER_ID_HEX;
    const std::string holding_id = idHex(8);
    const std::string position_nonce = "18446744073709551615";
    const json addresses_request = {
        {"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", owner_id},
        {"positionNonce", position_nonce},
    };
    const std::string addresses_payload = addresses_request.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> addresses_result(
        stablecoin_position_addresses(addresses_payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(addresses_result != nullptr);
    const json addresses_envelope = json::parse(addresses_result.get());
    LOGOS_ASSERT_TRUE(addresses_envelope["ok"].get<bool>());
    const json addresses = addresses_envelope["value"];
    const std::string position_id = addresses["positionIdHex"].get<std::string>();
    const std::string vault_id = addresses["vaultIdHex"].get<std::string>();
    const std::string frozen_protocol = protocolData(true);
    const std::string position = positionData(
        owner_id, vault_id, 18446744073709551615ULL, "100", "42");
    const std::string donated_vault = collateralHoldingData("120");
    const std::string source_holding = collateralHoldingData("500");

    expectRead(protocol_id, PROGRAM_OWNER_HEX, frozen_protocol);
    expectRead(position_id, PROGRAM_OWNER_HEX, position);
    expectRead(vault_id, TOKEN_OWNER_HEX, donated_vault);
    expectRead(holding_id, TOKEN_OWNER_HEX, source_holding);
    context.mockModule("lez_core", "list_accounts")
        .returnsVariant(QVariant(walletAccounts()));
    context.mockModule("lez_core", "send_generic_public_transaction")
        .returns(successfulTransaction());

    const LogosMap deposited = module.depositCollateral({
        {"ownerId", owner_id},
        {"positionNonce", position_nonce},
        {"userCollateralHoldingId", holding_id},
        {"amount", "0"},
    });

    assertOk(deposited);
    LOGOS_ASSERT_EQ(deposited["transactionId"].get<std::string>(), TRANSACTION_ID_HEX);
    const std::string planner_payload = json{
        {"stablecoinProgramId", PROGRAM_ID_HEX},
        {"ownerId", owner_id},
        {"positionNonce", position_nonce},
        {"amount", "0"},
        {"userCollateralHoldingId", holding_id},
        {"position", accountReadValue(position_id, PROGRAM_OWNER_HEX, position)},
        {"vault", accountReadValue(vault_id, TOKEN_OWNER_HEX, donated_vault)},
        {"userCollateralHolding", accountReadValue(holding_id, TOKEN_OWNER_HEX, source_holding)},
        {"protocolParameters", accountReadValue(protocol_id, PROGRAM_OWNER_HEX, frozen_protocol)},
    }.dump();
    std::unique_ptr<char, decltype(&stablecoin_free)> planner_result(
        stablecoin_deposit_collateral_plan(planner_payload.c_str()), &stablecoin_free);
    LOGOS_ASSERT_TRUE(planner_result != nullptr);
    const json planner_envelope = json::parse(planner_result.get());
    LOGOS_ASSERT_TRUE(planner_envelope["ok"].get<bool>());
    const json plan = planner_envelope["value"];
    LOGOS_ASSERT_EQ(
        plan["accountIds"],
        json::array({owner_id, position_id, vault_id, holding_id, protocol_id}));
    LOGOS_ASSERT_EQ(plan["signingRequirements"],
                    json::array({true, false, false, true, false}));
    LOGOS_ASSERT_EQ(plan["instruction"].size(), 5);
    LOGOS_ASSERT_EQ(plan["instruction"][1], 0);
    LOGOS_ASSERT_EQ(plan["instruction"][2], 0);
    LOGOS_ASSERT_EQ(plan["instruction"][3], 0);
    LOGOS_ASSERT_EQ(plan["instruction"][4], 0);
    LOGOS_ASSERT_TRUE(context.moduleCalledWith(
        "lez_core",
        "send_generic_public_transaction",
        submissionArguments(plan, {0, 3}, PROGRAM_ID_HEX)));
    LOGOS_ASSERT_EQ(context.moduleCallCount("lez_core", "send_generic_public_transaction"), 1);
}
