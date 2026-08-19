#include "WalletPortfolioService.h"

#include <algorithm>
#include <utility>

#include <QCryptographicHash>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSet>
#include <QVariantMap>

namespace {
QJsonObject enumFields(const QJsonValue& value, const QString& variant)
{
    return value.toObject().value(variant).toObject();
}

QString decodedDataText(const QJsonValue& value)
{
    if (value.isObject()) {
        return QString::fromUtf8(
            QJsonDocument(value.toObject()).toJson(QJsonDocument::Indented)).trimmed();
    }
    if (value.isArray()) {
        return QString::fromUtf8(
            QJsonDocument(value.toArray()).toJson(QJsonDocument::Indented)).trimmed();
    }
    return {};
}

QString decimalAdd(const QString& left, const QString& right)
{
    if (left.isEmpty() || right.isEmpty())
        return {};
    if (!std::all_of(left.cbegin(), left.cend(), [](QChar value) { return value.isDigit(); })
        || !std::all_of(right.cbegin(), right.cend(), [](QChar value) { return value.isDigit(); })) {
        return {};
    }

    QString result;
    result.reserve(std::max(left.size(), right.size()) + 1);
    qsizetype leftIndex = left.size();
    qsizetype rightIndex = right.size();
    int carry = 0;
    while (leftIndex > 0 || rightIndex > 0 || carry > 0) {
        const int leftDigit = leftIndex > 0 ? left.at(--leftIndex).digitValue() : 0;
        const int rightDigit = rightIndex > 0 ? right.at(--rightIndex).digitValue() : 0;
        const int sum = leftDigit + rightDigit + carry;
        result.prepend(QChar(QLatin1Char('0').unicode() + sum % 10));
        carry = sum / 10;
    }
    while (result.size() > 1 && result.startsWith(QLatin1Char('0')))
        result.remove(0, 1);
    return result;
}

void addField(QCryptographicHash& hash, const QString& value)
{
    const QByteArray utf8 = value.toUtf8();
    hash.addData(QByteArray::number(utf8.size()));
    hash.addData(QByteArrayLiteral(":"));
    hash.addData(utf8);
    hash.addData(QByteArrayLiteral(";"));
}

QByteArray accountReadsSignature(const QVector<WalletAccountRead>& reads)
{
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(QByteArray::number(reads.size()));
    hash.addData(QByteArrayLiteral(";"));
    for (const WalletAccountRead& read : reads) {
        addField(hash, read.accountId);
        addField(hash, read.status);
        addField(hash, read.programOwner);
        addField(hash, read.balanceHex);
        addField(hash, read.nonceHex);
        addField(hash, read.dataHex);
    }
    return hash.result();
}

WalletPortfolioResult failureResult(const QString& status, const QString& error)
{
    WalletPortfolioResult result;
    result.status = status;
    result.error = error;
    return result;
}
}

struct WalletPortfolioService::State {
    struct Program {
        QString name;
        QByteArray idl;
    };

    struct Token {
        QString id;
        QString displayId;
        QString name;
    };

    struct DecodeCache {
        QByteArray idl;
        QByteArray readsSignature;
        WalletDecodeResult result;
    };

    explicit State(Decoder decoderFunction)
        : decoder(decoderFunction ? std::move(decoderFunction)
                                  : Decoder(WalletIdlDecoder::decode))
    {
    }

    WalletDecodeResult decode(const QString& programId,
                              const Program& program,
                              const QVector<WalletAccountRead>& reads)
    {
        const QByteArray signature = accountReadsSignature(reads);
        const auto cached = decodedPrograms.constFind(programId);
        if (cached != decodedPrograms.cend()
            && cached->idl == program.idl
            && cached->readsSignature == signature) {
            return cached->result;
        }

        WalletDecodeResult result = decoder(program.idl, reads);
        decodedPrograms.insert(programId, { program.idl, signature, result });
        return result;
    }

    Decoder decoder;
    QHash<QString, Program> programs;
    QHash<QString, DecodeCache> decodedPrograms;
};

WalletPortfolioService::WalletPortfolioService(Decoder decoder)
    : m_state(std::make_unique<State>(std::move(decoder)))
{
}

WalletPortfolioService::~WalletPortfolioService() = default;

void WalletPortfolioService::registerProgram(const QString& programId,
                                             const QString& programName,
                                             const QByteArray& idlJson)
{
    if (programId.isEmpty() || programName.isEmpty() || idlJson.isEmpty())
        return;
    const auto existing = m_state->programs.constFind(programId);
    if (existing != m_state->programs.cend()
        && existing->name == programName
        && existing->idl == idlJson) {
        return;
    }
    m_state->programs.insert(programId, { programName, idlJson });
    m_state->decodedPrograms.remove(programId);
}

WalletPortfolioResult WalletPortfolioService::refresh(
    const WalletPortfolioRequest& request)
{
    if (request.walletFailure != WalletFailure::None)
        return failureResult(QStringLiteral("error"), walletFailureCode(request.walletFailure));
    if (request.tokenProgramId.isEmpty() || request.tokenDefinitionIds.isEmpty()) {
        return failureResult(
            QStringLiteral("blocked"), QStringLiteral("network_context_missing"));
    }
    if (request.tokenIdl.isEmpty())
        return failureResult(QStringLiteral("error"), QStringLiteral("token_idl_missing"));

    QVector<State::Token> tokens;
    QSet<QString> resolvedIds;
    QSet<QString> resolvedTokenIds;
    for (const QVariant& value : request.tokens) {
        const QVariantMap row = value.toMap();
        const QString id = row.value(QStringLiteral("definitionIdHex")).toString();
        const QString displayId = row.value(QStringLiteral("definitionId")).toString();
        if (id.isEmpty() || displayId.isEmpty()
            || resolvedTokenIds.contains(id.toLower())) {
            continue;
        }
        const bool requested = std::any_of(
            request.tokenDefinitionIds.cbegin(),
            request.tokenDefinitionIds.cend(),
            [&id, &displayId](const QString& expected) {
                return expected == displayId
                    || (expected.size() == id.size()
                        && expected.compare(id, Qt::CaseInsensitive) == 0);
            });
        if (!requested) {
            continue;
        }
        QString name = row.value(QStringLiteral("name")).toString().trimmed();
        if (name.isEmpty())
            name = QStringLiteral("Unnamed token");
        tokens.append({ id, displayId, std::move(name) });
        resolvedTokenIds.insert(id.toLower());
        resolvedIds.insert(id);
        resolvedIds.insert(displayId);
    }

    QHash<QString, State::Program> programs = m_state->programs;
    programs.insert(request.tokenProgramId, {
        request.tokenProgramName.isEmpty() ? QStringLiteral("Token")
                                           : request.tokenProgramName,
        request.tokenIdl,
    });

    QHash<QString, QString> tokenNames;
    for (const State::Token& token : tokens)
        tokenNames.insert(token.id, token.name);

    WalletPortfolioResult result;
    QHash<QString, QString> balances;
    bool tokenHoldingFailure = false;
    bool unreadPublicAccount = false;
    bool programFailure = false;
    for (auto program = programs.cbegin(); program != programs.cend(); ++program) {
        QVector<WalletAccountRead> programReads;
        for (const WalletAccountRead& read : request.publicAccountReads) {
            if (!read.ok()) {
                unreadPublicAccount = true;
                continue;
            }
            if (read.programOwner == program.key())
                programReads.append(read);
        }
        if (programReads.isEmpty())
            continue;

        const WalletDecodeResult decoded = m_state->decode(
            program.key(), program.value(), programReads);
        if (!decoded.ok()) {
            programFailure = true;
            if (program.key() == request.tokenProgramId)
                tokenHoldingFailure = true;
            continue;
        }
        if (decoded.accounts.size() != programReads.size()) {
            programFailure = true;
            if (program.key() == request.tokenProgramId)
                tokenHoldingFailure = true;
        }

        const qsizetype count = std::min(decoded.accounts.size(), programReads.size());
        for (qsizetype index = 0; index < count; ++index) {
            const WalletDecodedAccount& account = decoded.accounts.at(index);
            const WalletAccountRead& read = programReads.at(index);
            if (account.id != read.accountId) {
                programFailure = true;
                if (program.key() == request.tokenProgramId)
                    tokenHoldingFailure = true;
                continue;
            }

            WalletAccountPresentation presentation;
            presentation.address = read.accountId;
            presentation.programName = program.value().name;
            presentation.accountType = account.typeName;
            if (account.status == QStringLiteral("decoded"))
                presentation.decodedData = decodedDataText(account.value);

            if (program.key() == request.tokenProgramId
                && account.typeName == QStringLiteral("TokenHolding")) {
                const QJsonObject fungible = enumFields(account.value, QStringLiteral("Fungible"));
                const QString encodedDefinitionId = fungible.value(
                    QStringLiteral("definition_id")).toString();
                const QString definitionId = account.accountIds.value(encodedDefinitionId);
                const QString amount = fungible.value(QStringLiteral("balance")).toString();
                const QString total = decimalAdd(
                    balances.value(definitionId, QStringLiteral("0")), amount);
                if (account.status != QStringLiteral("decoded")
                    || fungible.isEmpty()
                    || definitionId.isEmpty()
                    || total.isEmpty()) {
                    tokenHoldingFailure = true;
                } else {
                    balances.insert(definitionId, total);
                }
                presentation.kind = QStringLiteral("token_holding");
                presentation.definitionId = definitionId;
                presentation.hiddenFromAccounts = true;
                const QString tokenName = tokenNames.value(definitionId);
                if (!tokenName.isEmpty())
                    presentation.semanticName = tokenName + QStringLiteral(" holding");
            } else if (program.key() == request.tokenProgramId
                       && account.typeName == QStringLiteral("TokenDefinition")) {
                presentation.kind = QStringLiteral("token_definition");
                presentation.semanticName = enumFields(
                    account.value, QStringLiteral("Fungible"))
                                                .value(QStringLiteral("name")).toString();
            } else if (program.key() == request.tokenProgramId
                       && account.typeName == QStringLiteral("TokenMetadata")) {
                presentation.kind = QStringLiteral("token_metadata");
            } else {
                presentation.kind = QStringLiteral("program");
                presentation.semanticName = account.typeName;
            }
            result.presentations.append(std::move(presentation));
        }
    }

    QVariantList available;
    auto appendAsset = [&result, &available](const QString& id,
                                             const QString& displayId,
                                             const QString& name,
                                             const QString& programOwner,
                                             const QString& balance,
                                             bool unavailable) {
        const bool positive = !unavailable
            && balance != QStringLiteral("0") && !balance.isEmpty();
        QVariantMap asset {
            { QStringLiteral("name"), name },
            { QStringLiteral("symbol"), name },
            { QStringLiteral("balance"), unavailable ? QString() : balance },
            { QStringLiteral("definitionId"), id },
            { QStringLiteral("displayDefinitionId"), displayId },
            { QStringLiteral("programOwner"), programOwner },
            { QStringLiteral("status"), unavailable ? QStringLiteral("unavailable")
                                                       : QStringLiteral("ready") },
            { QStringLiteral("section"), positive ? QStringLiteral("assets")
                                                     : QStringLiteral("available") },
        };
        if (positive)
            result.assets.append(std::move(asset));
        else
            available.append(std::move(asset));
    };

    const bool balancesUnavailable = tokenHoldingFailure || unreadPublicAccount;
    for (const State::Token& token : tokens) {
        appendAsset(token.id,
                    token.displayId,
                    token.name,
                    request.tokenProgramId,
                    balances.value(token.id, QStringLiteral("0")),
                    balancesUnavailable);
    }

    QSet<QString> missingIds;
    for (const QString& id : request.tokenDefinitionIds) {
        if (resolvedIds.contains(id)
            || resolvedTokenIds.contains(id.toLower())
            || missingIds.contains(id)) {
            continue;
        }
        missingIds.insert(id);
        appendAsset(id,
                    id,
                    QStringLiteral("Unknown token"),
                    {},
                    {},
                    true);
    }
    result.assets.append(available);

    if (tokens.isEmpty()) {
        result.status = QStringLiteral("error");
        result.error = QStringLiteral("definitions_unavailable");
    } else if (!missingIds.isEmpty() || tokenHoldingFailure || unreadPublicAccount) {
        result.status = QStringLiteral("partial");
        result.error = tokenHoldingFailure
            ? QStringLiteral("holding_decode_failed")
            : unreadPublicAccount
            ? QStringLiteral("public_account_read_failed")
            : QStringLiteral("some_definitions_unavailable");
    } else if (programFailure) {
        result.status = QStringLiteral("partial");
        result.error = QStringLiteral("program_decode_failed");
    } else {
        result.status = QStringLiteral("ready");
    }
    return result;
}
