#pragma once

#include <QAbstractListModel>
#include <QVector>

#include "WalletProvider.h"

class WalletAccountModel final : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        AddressRole,
        BalanceRole,
        IsPublicRole,
    };
    Q_ENUM(Role)

    explicit WalletAccountModel(QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = QModelIndex()) const override;
    QVariant data(const QModelIndex& index, int role = Qt::DisplayRole) const override;
    QHash<int, QByteArray> roleNames() const override;

    void replaceAccounts(const QVector<WalletAccount>& accounts);
    int count() const { return m_accounts.size(); }

signals:
    void countChanged();

private:
    struct Entry {
        QString name;
        QString address;
        QString balance;
        bool isPublic = true;
    };

    QVector<Entry> m_accounts;
};
