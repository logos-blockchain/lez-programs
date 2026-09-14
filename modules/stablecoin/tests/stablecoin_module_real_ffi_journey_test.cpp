#include "stablecoin_module_impl.h"

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

std::string protocolData() {
    std::string result;
    for (unsigned int seed = 1; seed <= 5; ++seed) result += idHex(seed);
    result += u128Le((decimal(FIXED_ONE) + decimal("1500000000000000")).convert_to<std::string>());
    result += i128Le(FIXED_ONE);
    result += i128Le("10000000000000000000000000");
    result += u128Le("1500000000000000000000000000");
    appendU64(result, 300'000);
    appendU64(result, 900'000);
    result += "00";
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

std::string accountResponse(const std::string& owner, const std::string& data) {
    return json{
        {"program_owner", owner},
        {"balance", std::string(32, '0')},
        {"nonce", std::string(32, '0')},
        {"data", data},
    }.dump();
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
    QVariantMap account;
    account.insert("account_id", QString::fromStdString(CALLER_ID_HEX));
    account.insert("is_public", true);
    return {QVariant(account)};
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
