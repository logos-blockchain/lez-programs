#include "stablecoin_module_impl.h"

#include <algorithm>
#include <cstdint>
#include <cstdlib>
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
