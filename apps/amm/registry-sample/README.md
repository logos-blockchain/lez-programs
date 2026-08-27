# AMM registry — sample

A minimal remote **known-tokens / known-pools registry** for the AMM app, so you
can test the remote-loading path (`AMM_REGISTRY_URL`) end to end. See
`docs/amm-registry-plan.md` for the full design.

This sample deliberately ships **empty** `tokens.json` / `pools.json` — the first
thing to verify is that the app loads and behaves sanely when the registry
resolves successfully but is empty.

## Files

| File | Role |
|---|---|
| `registry.json` | The **manifest** `AMM_REGISTRY_URL` points at. Names the token/pool files and the deployment the list targets. |
| `tokens.json` | The known-tokens array (currently `[]`). |
| `pools.json` | The known-pools array (currently `[]`). |

`tokensUrl` / `poolsUrl` in the manifest are resolved **relative to the manifest
URL**, so all three files just need to sit in the same directory.

## How to test

1. Push this directory on a branch of your fork/repo (e.g. `logos-blockchain/lez-programs`).
2. Point the app at the manifest's **raw** URL, and make sure the local-file
   overrides are unset (a local `TOKENS_CONFIG` / `AMM_POOLS_CONFIG` replaces the
   remote source entirely):

   ```bash
   unset TOKENS_CONFIG AMM_POOLS_CONFIG
   AMM_REGISTRY_URL=https://raw.githubusercontent.com/<owner>/lez-programs/<branch>/apps/amm/registry-sample/registry.json \
   nix run .#amm-ui
   ```

The app fetches `registry.json`, then `tokens.json` + `pools.json`, caches them
under the app's data dir, and shows the (empty) lists. A failed/unreachable fetch
falls back to the last cached copy.

## Manifest fields

- `name`, `version`, `timestamp`, `network` — informational. `timestamp` is the
  freshness key: **bump it whenever you edit `tokens.json` / `pools.json`** so the
  app re-downloads them instead of serving its cache.
- `tokensUrl`, `poolsUrl` — required; relative to the manifest URL.
- `programIds: { amm, token }` — the deployment this list targets. Left **empty**
  here so the sample loads against any deployment. Fill them in (base58, from
  `spel inspect` / the app's `configAccount`) to enable the **deployment guard**:
  the app then rejects the list unless these match the AMM/token programs it's
  connected to — which prevents showing stale IDs after a redeploy.

## Adding tokens / pools later

`tokens.json` entries: `{ symbol, name, definitionId, decimals }` (base58 ids;
`holding` is per-wallet and resolved by the app, so it is **not** in the shared
list). `pools.json` entries: `{ tokenA, tokenB, feeBps, poolId,
tokenADefinitionId, tokenBDefinitionId }`. Malformed entries are skipped, not
fatal. Remember to bump `timestamp`.
