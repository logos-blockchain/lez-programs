#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>

#include "lez_core_api.h"

// Test adapter between the universal module's std-based generated API and the
// Qt test framework's generated dependency client.
class UniversalLezCore {
public:
    explicit UniversalLezCore(LogosAPI* api)
        : qt_(api) { }

    std::string account_id_from_base58(const std::string& base58) {
        return qt_.account_id_from_base58(QString::fromStdString(base58)).toStdString();
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
        return qt_.send_generic_public_transaction(
                      qt_account_ids,
                      qt_signing_requirements,
                      QVariant(qt_instruction),
                      QString::fromStdString(program_id),
                      error)
            .toStdString();
    }

private:
    LezCore qt_;
};

struct LogosModules {
    explicit LogosModules(LogosAPI* api)
        : lez_core(api) { }

    UniversalLezCore lez_core;
};
