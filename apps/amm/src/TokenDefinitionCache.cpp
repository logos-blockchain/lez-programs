#include "TokenDefinitionCache.h"

#include <utility>

bool TokenDefinitionCacheKey::isReusable() const
{
    return !networkId.isEmpty()
        && !networkFingerprint.isEmpty()
        && !sequencerAddress.isEmpty()
        && !tokenIds.isEmpty();
}

bool TokenDefinitionCacheKey::operator==(const TokenDefinitionCacheKey& other) const
{
    return networkId == other.networkId
        && networkFingerprint == other.networkFingerprint
        && sequencerAddress == other.sequencerAddress
        && tokenIds == other.tokenIds;
}

TokenDefinitionCache::TokenDefinitionCache(WalletProvider& provider)
    : m_provider(provider),
      m_state(std::make_shared<State>())
{
}

TokenDefinitionCache::~TokenDefinitionCache()
{
    clear();
}

void TokenDefinitionCache::read(const TokenDefinitionCacheKey& key, Callback callback)
{
    if (contains(key)) {
        callback(m_state->cachedReads);
        return;
    }
    if (m_state->inFlight && m_state->inFlight->key == key) {
        m_state->inFlight->callbacks.append(std::move(callback));
        return;
    }
    cancelPending();

    const auto request = std::make_shared<InFlight>();
    request->key = key;
    request->callbacks.append(std::move(callback));
    m_state->inFlight = request;
    const std::weak_ptr<State> state = m_state;
    m_provider.readPublicAccountsAsync(
        key.tokenIds,
        [state, request](QVector<WalletAccountRead> reads) mutable {
            const std::shared_ptr<State> lockedState = state.lock();
            if (!lockedState || request->cancelled)
                return;
            if (lockedState->inFlight == request) {
                lockedState->inFlight.reset();
                if (TokenDefinitionCache::isComplete(request->key, reads)) {
                    lockedState->cachedKey = request->key;
                    lockedState->cachedReads = reads;
                }
            }

            QVector<Callback> callbacks = std::move(request->callbacks);
            for (Callback& callback : callbacks)
                callback(reads);
        });
}

bool TokenDefinitionCache::contains(const TokenDefinitionCacheKey& key) const
{
    return m_state->cachedKey && *m_state->cachedKey == key;
}

void TokenDefinitionCache::cancelPending()
{
    if (m_state->inFlight) {
        m_state->inFlight->cancelled = true;
        m_state->inFlight->callbacks.clear();
    }
    m_state->inFlight.reset();
}

void TokenDefinitionCache::clear()
{
    m_state->cachedKey.reset();
    m_state->cachedReads.clear();
    cancelPending();
}

bool TokenDefinitionCache::isComplete(
    const TokenDefinitionCacheKey& key,
    const QVector<WalletAccountRead>& reads)
{
    if (!key.isReusable() || reads.size() != key.tokenIds.size())
        return false;
    for (qsizetype index = 0; index < reads.size(); ++index) {
        const WalletAccountRead& read = reads.at(index);
        if (!read.ok() || read.accountId != key.tokenIds.at(index)
            || read.programOwner.isEmpty() || read.dataHex.isEmpty()) {
            return false;
        }
    }
    return true;
}
