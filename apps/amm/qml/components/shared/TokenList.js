.pragma library

// The app's one token list, shared by the swap and liquidity views so a token
// never appears in one picker and not the other.
//
// Two sources have to be reconciled:
//   * tokenList()     — TOKENS_CONFIG verbatim. Carries the display `symbol`,
//                       which is the only place a symbol exists. No chain check.
//   * resolveTokens() — the same configured ids PLUS the user's persisted custom
//                       ids, each read on-chain. Drops any id whose definition
//                       isn't a readable fungible token owned by the configured
//                       TokenProgram, and carries holdingId/balance. Ids come
//                       back as canonical base58.
//
// Merging them keeps every configured token listed (a dropped one is marked
// unselectable rather than vanishing), adds the custom tokens, and puts the
// configured symbol back on rows that resolveTokens returned without one.

// resolveTokens() echoes ids as base58, so only a base58-configured id can be
// compared against a resolved row here. See merge().
function isBase58Id(tokenId) {
    return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(String(tokenId || ""))
}

function configTokenFor(configTokens, definitionId) {
    for (var i = 0; i < configTokens.length; ++i) {
        if (String(configTokens[i].definitionId || "") === definitionId)
            return configTokens[i]
    }
    return null
}

// Rows are `{ definitionId, symbol, name, totalSupply, holdingId, balance }`,
// plus `selectable: false` and a `code` on a configured token that did not
// resolve. `unresolvedCode` is the reason code such a row carries.
function merge(configTokens, resolvedTokens, unresolvedCode) {
    var config = configTokens || []
    var resolved = resolvedTokens || []

    // No resolution data at all (module unavailable, AMM not initialized, or the
    // call simply hasn't landed) is "unknown", not "everything is broken" —
    // greying out the whole list would make the view unusable. Fall back to the
    // configured list as-is, which is what the swap view always did.
    if (resolved.length === 0)
        return config.map(function(token) {
            return {
                "definitionId": String(token.definitionId || ""),
                "symbol": String(token.symbol || ""),
                "name": String(token.name || ""),
                "totalSupply": "0",
                "holdingId": "",
                "balance": "0"
            }
        })

    var rows = []
    var resolvedIds = {}

    for (var i = 0; i < resolved.length; ++i) {
        var row = resolved[i]
        var id = String(row.definitionId || "")
        var configured = configTokenFor(config, id)
        resolvedIds[id] = true
        rows.push({
            "definitionId": id,
            "symbol": configured ? String(configured.symbol || "") : "",
            // Prefer the configured name so both pickers agree; fall back to the
            // on-chain name for a custom token the config doesn't carry.
            "name": configured && configured.name
                    ? String(configured.name) : String(row.name || ""),
            "totalSupply": row.totalSupply,
            "holdingId": row.holdingId,
            "balance": row.balance
        })
    }

    for (var j = 0; j < config.length; ++j) {
        var token = config[j]
        var configuredId = String(token.definitionId || "")
        if (configuredId.length === 0 || resolvedIds[configuredId])
            continue
        // A hex-configured id can't be compared against the base58 rows above, so
        // synthesizing a row risks duplicating a token that did resolve. Skip it
        // and fall back to the pre-merge behaviour for that entry.
        if (!isBase58Id(configuredId))
            continue
        rows.push({
            "definitionId": configuredId,
            "symbol": String(token.symbol || ""),
            "name": String(token.name || ""),
            "totalSupply": "0",
            "holdingId": "",
            "balance": "0",
            // Listed but greyed out, with the reason on hover, instead of
            // vanishing with no explanation.
            "selectable": false,
            "code": unresolvedCode || "token_unresolved"
        })
    }

    return rows
}
