#pragma once

#include <functional>

#include <QHash>
#include <QObject>
#include <QQueue>
#include <QSet>
#include <QStringList>
#include <QUrl>
#include <QVector>

#include "WalletProvider.h"

class AmmClient;
class QNetworkAccessManager;
class QNetworkRequest;

class SequencerClient final : public QObject {
    Q_OBJECT

public:
    using AccountsCallback = std::function<void(QVector<WalletAccountRead>)>;
    using TransactionCallback = std::function<void(bool ok, bool included)>;

    explicit SequencerClient(AmmClient* client, QObject* parent = nullptr);

    bool configure(const QString& configPath,
                   const QString& effectiveEndpoint = {});
    QString endpoint() const { return m_endpoint.toString(); }
    bool isConfigured() const { return m_endpoint.isValid() && !m_endpoint.isEmpty(); }
    void applyAuthorization(QNetworkRequest& request) const;

    void readAccounts(const QStringList& accountIds,
                      bool forceRefresh,
                      AccountsCallback callback);
    void queryTransaction(const QString& nativeHash, TransactionCallback callback);
    void clear();

private:
    using AccountCallback = std::function<void(WalletAccountRead)>;

    struct PendingRead {
        QString accountId;
    };

    void readAccount(const QString& accountId, bool forceRefresh, AccountCallback callback);
    void startPendingReads();
    void startRead(const QString& accountId);
    void completeRead(const QString& accountId, const WalletAccountRead& read);
    void cancelPendingReads();
    QNetworkRequest request() const;

    AmmClient* m_client;
    QNetworkAccessManager* m_network;
    QUrl m_endpoint;
    QByteArray m_authorization;
    QHash<QString, WalletAccountRead> m_cache;
    QHash<QString, QVector<AccountCallback>> m_waiters;
    QHash<QString, QVector<AccountCallback>> m_forcedWaiters;
    QQueue<PendingRead> m_pending;
    QSet<QString> m_activeReadIds;
    int m_activeReads = 0;
    quint64 m_generation = 0;
};
