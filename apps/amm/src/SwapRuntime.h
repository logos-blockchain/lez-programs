#pragma once

#include <QJsonObject>
#include <QString>
#include <QVariantMap>

#include "ActiveNetwork.h"

class AmmClient;
class WalletProvider;

// Off-chain orchestration for the Swap view: reads accounts through the wallet,
// drives the amm_client swap ops, and submits the swap transaction. The AMM
// domain logic lives in the amm_client crate; this class only glues account
// reads and submission to it. Mirrors NewPositionRuntime.
class SwapRuntime {
public:
    SwapRuntime(WalletProvider* wallet, AmmClient* client);

    // { exists, reserveA, reserveB, feeBps } for the (tokenIn, tokenOut) pool.
    // reserveA/reserveB follow the pool's canonical def order (the caller maps
    // them to sell/buy). exists=false when the pool is absent or has no liquidity.
    QVariantMap resolvePool(const QString& tokenInId,
                            const QString& tokenOutId,
                            const ActiveNetworkSnapshot& network);

    // Builds and submits a SwapExactInput transaction; returns the native tx
    // hash on success, an empty string on any failure.
    QString swap(const QString& tokenInId,
                 const QString& tokenOutId,
                 const QString& userInputHoldingId,
                 const QString& userOutputHoldingId,
                 const QString& amountInDecimal,
                 const QString& minOutDecimal,
                 const QString& deadlineMs,
                 const ActiveNetworkSnapshot& network,
                 bool walletOpen);

private:
    // Derives the config account id (config_id) and reads it. Returns an empty
    // object only when the config_id op itself fails.
    QJsonObject readConfig(const ActiveNetworkSnapshot& network) const;

    WalletProvider* m_wallet;
    AmmClient* m_client;
};
