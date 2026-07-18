#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QVector>

#include "WalletProvider.h"

struct WalletAccountPresentation {
    QString address;
    QString kind;
    QString semanticName;
    QString programName;
    QString accountType;
    QString definitionId;
    bool hiddenFromAccounts = false;
    QString decodedData;
};

class WalletAccountModel final : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        AddressRole,
        BalanceRole,
        IsPublicRole,
        KindRole,
        SectionRole,
        ProgramOwnerRole,
        ReadStatusRole,
        ProgramNameRole,
        AccountTypeRole,
        VisibilityRole,
        ControlRole,
        CanBePrimaryRole,
        IsPrimaryRole,
        DefinitionIdRole,
        AliasRole,
        DisplayAddressRole,
        DecodedDataRole,
    };
    Q_ENUM(Role)

    explicit WalletAccountModel(QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = QModelIndex()) const override;
    QVariant data(const QModelIndex& index, int role = Qt::DisplayRole) const override;
    QHash<int, QByteArray> roleNames() const override;

    void replaceAccounts(const QVector<WalletAccount>& accounts,
                         const QHash<QString, QString>& aliases = {},
                         const QString& primaryAddress = {});
    bool applyPresentations(const QVector<WalletAccountPresentation>& presentations);
    void setAlias(const QString& address, const QString& alias);
    void setPrimaryAddress(const QString& address);
    bool contains(const QString& address) const;
    bool canBePrimary(const QString& address) const;
    QString firstAutomaticPrimary() const;
    int indexOf(const QString& address) const;
    int count() const { return m_accounts.size(); }

signals:
    void countChanged();

private:
    struct Entry {
        QString alias;
        QString semanticName;
        QString name;
        QString address;
        QString displayAddress;
        QString balance;
        bool isPublic = true;
        QString kind;
        QString section;
        QString programOwner;
        QString readStatus;
        QString programName;
        QString accountType;
        QString definitionId;
        QString decodedData;
        bool canBePrimary = false;
        bool isPrimary = false;
    };

    static QString defaultName(const Entry& entry);
    static QString sectionFor(const Entry& entry, bool hiddenFromAccounts = false);
    void updateEntryName(Entry& entry);

    QVector<Entry> m_accounts;
};
