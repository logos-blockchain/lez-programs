#pragma once

#include <QJsonArray>
#include <QJsonObject>
#include <QObject>
#include <QString>
#include <QVariantMap>
#include <QVector>

#include <functional>

#include "ActiveNetwork.h"

class AmmClient;
class WalletProvider;
class SequencerClient;
struct WalletAccount;

class NewPositionRuntime : public QObject {
public:
    using ResultCallback = std::function<void(QVariantMap)>;

    NewPositionRuntime(WalletProvider* wallet,
                       AmmClient* client,
                       SequencerClient* sequencer = nullptr);

    void clearWalletAccounts();
    void setWalletAccounts(const QVector<WalletAccount>& accounts);

    void contextAsync(const QVariantMap& request,
                      const ActiveNetworkSnapshot& network,
                      bool walletOpen,
                      bool refreshPublicData,
                      ResultCallback callback);
    void quoteAsync(const QVariantMap& request,
                    const ActiveNetworkSnapshot& network,
                    bool walletOpen,
                    bool forceRefresh,
                    bool isPoolProbe,
                    ResultCallback callback);
    void submitAsync(const QVariantMap& request,
                     const QString& quoteHash,
                     const ActiveNetworkSnapshot& network,
                     bool walletCanSubmit,
                     ResultCallback callback);
    void cancelSubmit();

    QVariantMap context(const QVariantMap& request,
                        const ActiveNetworkSnapshot& network,
                        bool walletOpen,
                        bool refreshWalletAccounts);
    QVariantMap quote(const QVariantMap& request,
                      const ActiveNetworkSnapshot& network,
                      bool walletOpen);
    QVariantMap submit(const QVariantMap& request,
                       const QString& quoteHash,
                       const ActiveNetworkSnapshot& network,
                       bool walletOpen);

private:
    enum class FreshLpState {
        None,
        Creating,
        Ready,
        Submitting,
    };

    QJsonArray walletAccountReads(bool walletOpen, bool refresh) const;
    QJsonObject buildQuoteInput(const QVariantMap& request,
                                const ActiveNetworkSnapshot& network,
                                bool walletOpen,
                                bool freshWalletAccounts,
                                QJsonObject* error) const;
    void buildQuoteInputAsync(const QVariantMap& request,
                              const ActiveNetworkSnapshot& network,
                              bool walletOpen,
                              bool forceRefresh,
                              std::function<bool()> shouldContinue,
                              std::function<void(QJsonObject, QJsonObject)> callback);
    void submitPlanAsync(QJsonObject input,
                         const QString& quoteHash,
                         QJsonValue freshLp,
                         QString freshLpAccountId,
                         quint64 submitGeneration,
                         quint64 walletGeneration);
    void prepareFreshLpAsync(QJsonObject input,
                             const QString& quoteHash,
                             quint64 submitGeneration,
                             quint64 walletGeneration);
    void validatePendingFreshLpAsync(QJsonObject input,
                                     const QString& quoteHash,
                                     quint64 submitGeneration,
                                     quint64 walletGeneration);
    void rememberPendingFreshLp(const QString& accountId);
    void clearPendingFreshLp();
    void finishSubmit(quint64 submitGeneration, QVariantMap result);
    bool submitIsCurrent(quint64 submitGeneration,
                         quint64 walletGeneration) const;

    WalletProvider* m_wallet;
    AmmClient* m_client;
    SequencerClient* m_sequencer;
    QStringList m_walletPublicAccountIds;
    QString m_pendingFreshLpAccountId;
    FreshLpState m_freshLpState = FreshLpState::None;
    bool m_submitInFlight = false;
    quint64 m_walletGeneration = 0;
    quint64 m_contextGeneration = 0;
    quint64 m_userQuoteGeneration = 0;
    quint64 m_submitGeneration = 0;
    ResultCallback m_submitCallback;
};
