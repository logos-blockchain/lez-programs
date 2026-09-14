#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include <nlohmann/json.hpp>

#include "lez_core_api.h"

// Test adapter between the universal module's std-based generated API and the
// Qt test framework's generated dependency client.
class UniversalLezCore {
public:
    static std::string transportErrorSentinel() {
        return "__logos_test_transport_error__";
    }

    explicit UniversalLezCore(LogosAPI* api)
        : qt_(api) { }

    std::string account_id_from_base58(const std::string& base58) {
        return qt_.account_id_from_base58(QString::fromStdString(base58)).toStdString();
    }

    nlohmann::json list_accounts(logos::CallError* error = nullptr) {
        const QVariantList accounts = qt_.list_accounts(error);
        if (accounts.size() == 1
            && accounts.front().toString().toStdString() == transportErrorSentinel()) {
            if (error != nullptr) {
                error->code = "transport_error";
                error->message = "simulated transport failure";
                error->origin = "lez_core";
            }
            return nlohmann::json::array();
        }
        nlohmann::json result = nlohmann::json::array();
        for (const QVariant& account : accounts) {
            const QVariantMap fields = account.toMap();
            result.push_back({
                {"account_id", fields.value("account_id").toString().toStdString()},
                {"is_public", fields.value("is_public").toBool()},
            });
        }
        return result;
    }

    std::string get_account_public(const std::string& account_id,
                                   logos::CallError* error = nullptr) {
        return qt_.get_account_public(QString::fromStdString(account_id), error).toStdString();
    }

    std::string send_generic_public_transaction(
        const std::vector<std::string>& account_ids,
        const std::vector<bool>& signing_requirements,
        const std::vector<std::uint8_t>& instruction,
        const std::string& program_id,
        logos::CallError* error = nullptr) {
        QStringList qt_account_ids;
        qt_account_ids.reserve(static_cast<qsizetype>(account_ids.size()));
        for (const auto& account_id : account_ids) {
            qt_account_ids.push_back(QString::fromStdString(account_id));
        }

        QVariantList qt_signing_requirements;
        qt_signing_requirements.reserve(
            static_cast<qsizetype>(signing_requirements.size()));
        for (const bool required : signing_requirements) {
            qt_signing_requirements.push_back(required);
        }

        const QByteArray qt_instruction(
            reinterpret_cast<const char*>(instruction.data()),
            static_cast<qsizetype>(instruction.size()));
        const std::string result = qt_.send_generic_public_transaction(
                                       qt_account_ids,
                                       qt_signing_requirements,
                                       QVariant(qt_instruction),
                                       QString::fromStdString(program_id),
                                       error)
                                       .toStdString();
        if (result == transportErrorSentinel() && error != nullptr) {
            error->code = "transport_error";
            error->message = "simulated transport failure";
            error->origin = "lez_core";
            return {};
        }
        return result;
    }

private:
    LezCore qt_;
};

struct LogosModules {
    explicit LogosModules(LogosAPI* api)
        : lez_core(api) { }

    UniversalLezCore lez_core;
};
