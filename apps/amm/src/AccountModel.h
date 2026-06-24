#pragma once

#include <QAbstractListModel>
#include <QJsonArray>
#include <QString>
#include <QVector>

// One wallet account row. Mirrors the shape returned by the core wallet
// module's list_accounts() (account_id + is_public), with a display name and
// a lazily-fetched balance.
struct AccountEntry {
    QString name;
    QString address;
    QString balance;
    bool isPublic = true;
};

// QAbstractListModel of wallet accounts, exposed to QML via
// logos.model("amm_ui", "accountModel"). Ported from the LEZ wallet UI.
class AccountModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)
public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        AddressRole,
        BalanceRole,
        IsPublicRole
    };
    Q_ENUM(Role)

    explicit AccountModel(QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = QModelIndex()) const override;
    QVariant data(const QModelIndex& index, int role = Qt::DisplayRole) const override;
    QHash<int, QByteArray> roleNames() const override;

    void replaceFromJsonArray(const QJsonArray& arr);
    void setBalanceByAddress(const QString& address, const QString& balance);
    int count() const { return m_entries.size(); }

signals:
    void countChanged();

private:
    QVector<AccountEntry> m_entries;
};
