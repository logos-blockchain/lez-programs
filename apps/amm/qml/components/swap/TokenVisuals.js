.pragma library

// Shared derivation helpers for a token's display avatar (color + letter).
// The real token config (see AmmUiBackend::tokenList / TOKENS_CONFIG) only
// carries symbol/name/definitionId/holding/decimals — no color/letter — so
// every place that used to read token.color/token.letter derives them here
// instead, deterministically from the token's symbol.

var PALETTE = [
    "#627eea", "#2775ca", "#26a17b", "#f7931a",
    "#9b59b6", "#e91e63", "#00bcd4", "#8bc34a"
];

function letterFor(symbol) {
    return (symbol && symbol.length > 0) ? symbol.charAt(0).toUpperCase() : "?";
}

function colorFor(symbol) {
    if (!symbol || symbol.length === 0)
        return PALETTE[0];

    var hash = 0;
    for (var i = 0; i < symbol.length; i++)
        hash = (hash * 31 + symbol.charCodeAt(i)) >>> 0;

    return PALETTE[hash % PALETTE.length];
}
