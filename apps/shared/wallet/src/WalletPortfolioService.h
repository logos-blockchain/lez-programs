#pragma once

#include <functional>
#include <memory>

#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVector>

#include "WalletAccountModel.h"
#include "WalletIdlDecoder.h"
#include "WalletProvider.h"

// Input for a portfolio refresh. The snapshot constructor deliberately copies
// only `WalletSnapshot::publicAccountReads`; callers must not reconstruct reads
// from the display-model accounts.
struct WalletPortfolioRequest {
    WalletPortfolioRequest() = default;
    explicit WalletPortfolioRequest(const WalletSnapshot& snapshot)
        : walletFailure(snapshot.failure),
          publicAccountReads(snapshot.publicAccountReads)
    {
    }

    WalletFailure walletFailure = WalletFailure::None;
    QVector<WalletAccountRead> publicAccountReads;
    QStringList tokenDefinitionIds;
    QVariantList tokens;
    QString tokenProgramId;
    QByteArray tokenIdl;
    QString tokenProgramName = QStringLiteral("Token");
};

struct WalletPortfolioResult {
    QVector<WalletAccountPresentation> presentations;
    QVariantList assets;
    QString status = QStringLiteral("idle");
    QString error;
};

// Presents accounts owned by registered IDL programs and combines decoded token
// holdings with token definitions already resolved by the token module.
class WalletPortfolioService final {
public:
    using Decoder = std::function<WalletDecodeResult(
        const QByteArray&, const QVector<WalletAccountRead>&)>;

    explicit WalletPortfolioService(Decoder decoder = {});
    ~WalletPortfolioService();

    WalletPortfolioService(const WalletPortfolioService&) = delete;
    WalletPortfolioService& operator=(const WalletPortfolioService&) = delete;

    // Adds an IDL-backed account presentation. Later registrations for the
    // same program id replace the prior definition.
    void registerProgram(const QString& programId,
                         const QString& programName,
                         const QByteArray& idlJson);
    WalletPortfolioResult refresh(const WalletPortfolioRequest& request);

private:
    struct State;

    std::unique_ptr<State> m_state;
};
