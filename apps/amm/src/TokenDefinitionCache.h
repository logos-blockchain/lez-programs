#pragma once

#include <functional>
#include <memory>
#include <optional>

#include <QString>
#include <QStringList>
#include <QVector>

#include "WalletProvider.h"

struct TokenDefinitionCacheKey {
    QString networkId;
    QString networkFingerprint;
    QString sequencerAddress;
    QStringList tokenIds;

    bool isReusable() const;
    bool operator==(const TokenDefinitionCacheKey& other) const;
};

class TokenDefinitionCache final {
public:
    using Callback = std::function<void(QVector<WalletAccountRead>)>;

    explicit TokenDefinitionCache(WalletProvider& provider);
    ~TokenDefinitionCache();

    void read(const TokenDefinitionCacheKey& key, Callback callback);
    bool contains(const TokenDefinitionCacheKey& key) const;
    void cancelPending();
    void clear();

private:
    struct InFlight {
        TokenDefinitionCacheKey key;
        QVector<Callback> callbacks;
        bool cancelled = false;
    };

    struct State {
        std::optional<TokenDefinitionCacheKey> cachedKey;
        QVector<WalletAccountRead> cachedReads;
        std::shared_ptr<InFlight> inFlight;
    };

    static bool isComplete(const TokenDefinitionCacheKey& key,
                           const QVector<WalletAccountRead>& reads);

    WalletProvider& m_provider;
    std::shared_ptr<State> m_state;
};
