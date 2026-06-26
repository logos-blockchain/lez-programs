#pragma once

#include <QJsonArray>
#include <QJsonObject>
#include <QString>
#include <QVariantMap>

#include "ActiveNetwork.h"

class AmmClient;
class WalletProvider;

class NewPositionRuntime {
public:
    NewPositionRuntime(WalletProvider* wallet, AmmClient* client);

    void clearWalletAccounts();

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
    QJsonArray walletAccountReads(bool walletOpen, bool refresh) const;
    QJsonObject buildQuoteInput(const QVariantMap& request,
                                const ActiveNetworkSnapshot& network,
                                bool walletOpen,
                                bool freshWalletAccounts,
                                QJsonObject* error) const;

    WalletProvider* m_wallet;
    AmmClient* m_client;
    bool m_submitInFlight = false;
};
