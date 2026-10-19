#include "WalletAccountModel.h"

#include <utility>

namespace {
const QString DEFAULT_PROGRAM_OWNER(64, QLatin1Char('0'));
}

WalletAccountModel::WalletAccountModel(QObject* parent)
    : QAbstractListModel(parent)
{
}

int WalletAccountModel::rowCount(const QModelIndex& parent) const
{
    return parent.isValid() ? 0 : m_accounts.size();
}

QVariant WalletAccountModel::data(const QModelIndex& index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_accounts.size())
        return {};

    const Entry& account = m_accounts.at(index.row());
    switch (role) {
    case NameRole:
        return account.name;
    case AddressRole:
        return account.address;
    case DisplayAddressRole:
        return account.displayAddress;
    case BalanceRole:
        return account.balance;
    case IsPublicRole:
        return account.isPublic;
    case KindRole:
        return account.kind;
    case SectionRole:
        return account.section;
    case ProgramOwnerRole:
        return account.programOwner;
    case ReadStatusRole:
        return account.readStatus;
    case ProgramNameRole:
        return account.programName;
    case AccountTypeRole:
        return account.accountType;
    case VisibilityRole:
        return account.isPublic ? QStringLiteral("public") : QStringLiteral("private");
    case ControlRole:
        return QStringLiteral("wallet");
    case CanBePrimaryRole:
        return account.canBePrimary;
    case IsPrimaryRole:
        return account.isPrimary;
    case DefinitionIdRole:
        return account.definitionId;
    case AliasRole:
        return account.alias;
    case DecodedDataRole:
        return account.decodedData;
    default:
        return {};
    }
}

QHash<int, QByteArray> WalletAccountModel::roleNames() const
{
    return {
        { NameRole, "name" },
        { AddressRole, "address" },
        { BalanceRole, "balance" },
        { IsPublicRole, "isPublic" },
        { KindRole, "kind" },
        { SectionRole, "section" },
        { ProgramOwnerRole, "programOwner" },
        { ReadStatusRole, "readStatus" },
        { ProgramNameRole, "programName" },
        { AccountTypeRole, "accountType" },
        { VisibilityRole, "visibility" },
        { ControlRole, "control" },
        { CanBePrimaryRole, "canBePrimary" },
        { IsPrimaryRole, "isPrimary" },
        { DefinitionIdRole, "definitionId" },
        { AliasRole, "alias" },
        { DisplayAddressRole, "displayAddress" },
        { DecodedDataRole, "decodedData" },
    };
}

void WalletAccountModel::replaceAccounts(const QVector<WalletAccount>& accounts,
                                         const QHash<QString, QString>& aliases,
                                         const QString& primaryAddress)
{
    beginResetModel();
    const qsizetype oldCount = m_accounts.size();
    m_accounts.clear();
    m_accounts.reserve(accounts.size());
    for (const WalletAccount& account : accounts) {
        Entry entry;
        entry.alias = aliases.value(account.address);
        entry.address = account.address;
        entry.displayAddress = account.displayAddress.isEmpty()
            ? account.address : account.displayAddress;
        entry.balance = account.balance;
        entry.isPublic = account.isPublic;
        entry.programOwner = account.programOwner;
        entry.readStatus = account.readStatus;
        if (!account.isPublic) {
            entry.kind = QStringLiteral("private");
            entry.canBePrimary = true;
        } else if (account.readStatus != QStringLiteral("ok")) {
            entry.kind = QStringLiteral("unknown");
        } else if (account.programOwner == DEFAULT_PROGRAM_OWNER) {
            entry.kind = QStringLiteral("user");
            entry.canBePrimary = true;
        } else {
            entry.kind = QStringLiteral("program");
        }
        entry.section = sectionFor(entry);
        entry.isPrimary = account.address == primaryAddress && entry.canBePrimary;
        updateEntryName(entry);
        m_accounts.append(std::move(entry));
    }
    endResetModel();
    if (oldCount != m_accounts.size())
        emit countChanged();
}

bool WalletAccountModel::applyPresentations(
    const QVector<WalletAccountPresentation>& presentations)
{
    if (presentations.isEmpty())
        return clearPresentations();

    QHash<QString, int> rowsByAddress;
    rowsByAddress.reserve(m_accounts.size());
    for (int row = 0; row < m_accounts.size(); ++row) {
        const QString& address = m_accounts.at(row).address;
        if (!rowsByAddress.contains(address))
            rowsByAddress.insert(address, row);
    }

    int firstChanged = m_accounts.size();
    int lastChanged = -1;
    for (const WalletAccountPresentation& presentation : presentations) {
        const auto row = rowsByAddress.constFind(presentation.address);
        if (row == rowsByAddress.cend())
            continue;
        const Entry current = m_accounts.at(row.value());
        Entry entry = current;
        if (!presentation.kind.isEmpty())
            entry.kind = presentation.kind;
        entry.programName = presentation.programName;
        entry.accountType = presentation.accountType;
        entry.definitionId = presentation.definitionId;
        entry.decodedData = presentation.decodedData;
        entry.semanticName = presentation.semanticName;
        entry.section = sectionFor(entry, presentation.hiddenFromAccounts);
        entry.canBePrimary = entry.kind == QStringLiteral("user")
            || entry.kind == QStringLiteral("private");
        if (!entry.canBePrimary)
            entry.isPrimary = false;
        updateEntryName(entry);
        if (entry.alias == current.alias
            && entry.semanticName == current.semanticName
            && entry.name == current.name
            && entry.address == current.address
            && entry.displayAddress == current.displayAddress
            && entry.balance == current.balance
            && entry.isPublic == current.isPublic
            && entry.kind == current.kind
            && entry.section == current.section
            && entry.programOwner == current.programOwner
            && entry.readStatus == current.readStatus
            && entry.programName == current.programName
            && entry.accountType == current.accountType
            && entry.definitionId == current.definitionId
            && entry.decodedData == current.decodedData
            && entry.canBePrimary == current.canBePrimary
            && entry.isPrimary == current.isPrimary) {
            continue;
        }
        m_accounts[row.value()] = std::move(entry);
        if (row.value() < firstChanged)
            firstChanged = row.value();
        if (row.value() > lastChanged)
            lastChanged = row.value();
    }
    if (lastChanged < 0)
        return false;
    emit dataChanged(index(firstChanged), index(lastChanged), {
        NameRole,
        KindRole,
        SectionRole,
        ProgramNameRole,
        AccountTypeRole,
        DecodedDataRole,
        CanBePrimaryRole,
        IsPrimaryRole,
        DefinitionIdRole,
    });
    return true;
}

bool WalletAccountModel::clearPresentations()
{
    int firstChanged = m_accounts.size();
    int lastChanged = -1;
    for (int row = 0; row < m_accounts.size(); ++row) {
        Entry& entry = m_accounts[row];
        const Entry current = entry;
        resetPresentation(entry);
        if (entry.alias == current.alias
            && entry.semanticName == current.semanticName
            && entry.name == current.name
            && entry.kind == current.kind
            && entry.section == current.section
            && entry.programName == current.programName
            && entry.accountType == current.accountType
            && entry.definitionId == current.definitionId
            && entry.decodedData == current.decodedData
            && entry.canBePrimary == current.canBePrimary
            && entry.isPrimary == current.isPrimary) {
            continue;
        }
        if (row < firstChanged)
            firstChanged = row;
        lastChanged = row;
    }
    if (lastChanged < 0)
        return false;
    emit dataChanged(index(firstChanged), index(lastChanged), {
        NameRole,
        KindRole,
        SectionRole,
        ProgramNameRole,
        AccountTypeRole,
        DecodedDataRole,
        CanBePrimaryRole,
        IsPrimaryRole,
        DefinitionIdRole,
    });
    return true;
}

void WalletAccountModel::setAlias(const QString& address, const QString& alias)
{
    const int row = indexOf(address);
    if (row < 0)
        return;
    Entry& entry = m_accounts[row];
    entry.alias = alias;
    updateEntryName(entry);
    emit dataChanged(index(row), index(row), { NameRole, AliasRole });
}

void WalletAccountModel::setPrimaryAddress(const QString& address)
{
    for (int row = 0; row < m_accounts.size(); ++row) {
        Entry& entry = m_accounts[row];
        const bool next = entry.address == address && entry.canBePrimary;
        if (entry.isPrimary == next)
            continue;
        entry.isPrimary = next;
        emit dataChanged(index(row), index(row), { IsPrimaryRole });
    }
}

bool WalletAccountModel::contains(const QString& address) const
{
    return indexOf(address) >= 0;
}

bool WalletAccountModel::canBePrimary(const QString& address) const
{
    const int row = indexOf(address);
    return row >= 0 && m_accounts.at(row).canBePrimary;
}

QString WalletAccountModel::firstAutomaticPrimary() const
{
    for (const Entry& entry : m_accounts) {
        if (entry.kind == QStringLiteral("user"))
            return entry.address;
    }
    return {};
}

int WalletAccountModel::indexOf(const QString& address) const
{
    for (int row = 0; row < m_accounts.size(); ++row) {
        if (m_accounts.at(row).address == address)
            return row;
    }
    return -1;
}

QString WalletAccountModel::defaultName(const Entry& entry)
{
    if (!entry.accountType.isEmpty()) {
        QString name = entry.accountType;
        for (qsizetype index = 1; index < name.size(); ++index) {
            if (name.at(index).isUpper() && name.at(index - 1).isLower())
                name.insert(index++, QLatin1Char(' '));
        }
        return name;
    }
    if (entry.kind == QStringLiteral("user"))
        return QStringLiteral("User account");
    if (entry.kind == QStringLiteral("private"))
        return QStringLiteral("Private account");
    if (entry.kind == QStringLiteral("unknown"))
        return QStringLiteral("Unknown account");
    return QStringLiteral("Program account");
}

QString WalletAccountModel::sectionFor(const Entry& entry, bool hiddenFromAccounts)
{
    if (hiddenFromAccounts || entry.kind == QStringLiteral("token_holding"))
        return QStringLiteral("hidden");
    if (entry.kind == QStringLiteral("user") || entry.kind == QStringLiteral("private"))
        return QStringLiteral("accounts");
    return QStringLiteral("advanced");
}

void WalletAccountModel::resetPresentation(Entry& entry)
{
    entry.semanticName.clear();
    entry.programName.clear();
    entry.accountType.clear();
    entry.definitionId.clear();
    entry.decodedData.clear();
    if (!entry.isPublic) {
        entry.kind = QStringLiteral("private");
        entry.canBePrimary = true;
    } else if (entry.readStatus != QStringLiteral("ok")) {
        entry.kind = QStringLiteral("unknown");
        entry.canBePrimary = false;
    } else if (entry.programOwner == DEFAULT_PROGRAM_OWNER) {
        entry.kind = QStringLiteral("user");
        entry.canBePrimary = true;
    } else {
        entry.kind = QStringLiteral("program");
        entry.canBePrimary = false;
    }
    if (!entry.canBePrimary)
        entry.isPrimary = false;
    entry.section = sectionFor(entry);
    updateEntryName(entry);
}

void WalletAccountModel::updateEntryName(Entry& entry)
{
    entry.name = !entry.alias.isEmpty()
        ? entry.alias
        : (!entry.semanticName.isEmpty() ? entry.semanticName : defaultName(entry));
}
