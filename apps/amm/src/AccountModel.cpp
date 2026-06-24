#include "AccountModel.h"

#include <QJsonObject>

AccountModel::AccountModel(QObject* parent)
    : QAbstractListModel(parent)
{
}

int AccountModel::rowCount(const QModelIndex& parent) const
{
    if (parent.isValid())
        return 0;
    return m_entries.size();
}

QVariant AccountModel::data(const QModelIndex& index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_entries.size())
        return QVariant();

    const AccountEntry& e = m_entries.at(index.row());
    switch (role) {
    case NameRole:     return e.name;
    case AddressRole:  return e.address;
    case BalanceRole:  return e.balance;
    case IsPublicRole: return e.isPublic;
    default:           return QVariant();
    }
}

QHash<int, QByteArray> AccountModel::roleNames() const
{
    return {
        { NameRole,     "name"     },
        { AddressRole,  "address"  },
        { BalanceRole,  "balance"  },
        { IsPublicRole, "isPublic" }
    };
}

void AccountModel::replaceFromJsonArray(const QJsonArray& arr)
{
    beginResetModel();
    const int oldCount = m_entries.size();
    m_entries.clear();
    int idx = 0;
    for (const QJsonValue& v : arr) {
        AccountEntry e;
        e.name = QStringLiteral("Account %1").arg(++idx);
        e.balance = QString();
        if (v.isObject()) {
            const QJsonObject obj = v.toObject();
            e.address = obj.value(QStringLiteral("account_id")).toString();
            e.isPublic = obj.value(QStringLiteral("is_public")).toBool(true);
        } else {
            e.address = v.toString();
            e.isPublic = true;
        }
        m_entries.append(e);
    }
    endResetModel();
    if (oldCount != m_entries.size())
        emit countChanged();
}

void AccountModel::setBalanceByAddress(const QString& address, const QString& balance)
{
    for (int i = 0; i < m_entries.size(); ++i) {
        if (m_entries.at(i).address == address) {
            if (m_entries.at(i).balance != balance) {
                m_entries[i].balance = balance;
                const QModelIndex idx = index(i, 0);
                emit dataChanged(idx, idx, { BalanceRole });
            }
            return;
        }
    }
}
