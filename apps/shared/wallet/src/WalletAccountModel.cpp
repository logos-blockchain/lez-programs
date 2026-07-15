#include "WalletAccountModel.h"

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
    case BalanceRole:
        return account.balance;
    case IsPublicRole:
        return account.isPublic;
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
    };
}

void WalletAccountModel::replaceAccounts(const QVector<WalletAccount>& accounts)
{
    beginResetModel();
    const qsizetype oldCount = m_accounts.size();
    m_accounts.clear();
    m_accounts.reserve(accounts.size());
    for (qsizetype index = 0; index < accounts.size(); ++index) {
        const WalletAccount& account = accounts.at(index);
        m_accounts.append({
            QStringLiteral("Account %1").arg(index + 1),
            account.address,
            account.balance,
            account.isPublic,
        });
    }
    endResetModel();
    if (oldCount != m_accounts.size())
        emit countChanged();
}
