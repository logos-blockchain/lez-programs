# AMM UI

A QML UI application for the Automated Market Maker (AMM) program.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

## Wallet / chain integration

This app is a `ui_qml` module with a hand-written C++ backend
(`src/AmmUiBackend.*`, plugin in `src/AmmUiPlugin.*`) that depends on the core
**`lez_core`** wallet module. The backend calls the core module's
wallet FFI through `m_logos->lez_core.*` and exposes an async QtRO
surface (`src/AmmUiBackend.rep`) plus an account list model to the QML view.

**Onboarding is non-invasive.** The app opens straight to the Trade screen; the
navbar shows **Connect** or **Connected** + the account selector. The app can
adopt an open shared wallet. To open an existing wallet, choose **Connect**
and enter the storage password. Use an empty password for unencrypted storage.
If no wallet exists, it opens the wallet-creation dialog, which asks for a
password and confirmation, then displays the recovery phrase while the new
wallet syncs. Until the first sync finishes, the navbar shows its progress and
transaction submission stays blocked. There is no path
picking — the wallet uses LEZ's canonical
home, `~/.lee/wallet/` (override with `LEE_WALLET_HOME_DIR`, the same var LEZ
honors), and its config (`wallet_config.json`) self-initializes.

Account/keystore sharing follows the runtime:

- **Standalone** (`nix run .#amm-ui`): own core-module instance, but the canonical
  `~/.lee/wallet` keystore is shared with the LEZ wallet UI and any other LEZ
  app on the machine. On launch the app tries an empty password unless it was
  explicitly disconnected. Encrypted storage requires the password at **Connect**.
- **Inside Basecamp**: the core wallet module is a single shared instance, so on
  startup the backend **adopts** the already-open wallet through
  `LogosWalletProvider::connectAsync()`, surfacing **shared** accounts across apps.
  The same app-local disconnected preference applies.

**Disconnect** clears this app's account list, blocks its transaction submission
paths, and skips automatic connection on subsequent launches until **Connect**
is used. Cancelling the first sync does the same. It leaves the shared wallet
open for other apps. The creation password encrypts the new wallet's storage.
[LEZ wallet storage](https://github.com/logos-blockchain/logos-execution-zone/blob/1806e873ea2ac0e1b608aa7cf2f9b6e1fb12db21/lez/wallet/src/storage/mod.rs)

## Setup

This project requires Nix with experimental features enabled. If you haven't already, enable them permanently:

```bash
mkdir -p ~/.config/nix && echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

Install the Logos package manager CLI globally (one-time):

```bash
nix profile install 'github:logos-co/logos-package-manager#cli'
```

This makes `lgpm` available as a global command.

## Running the UI standalone

The app is built from the **repository-root** flake (which also builds the
`amm_module` core module the UI delegates its AMM logic to). From the repo root,
launch it with its named attribute:

```bash
nix run .#amm-ui
```

This launches the development UI with its AMM and wallet modules. Live operations
use the sequencer in the wallet configuration.

Build just the AMM core module with `nix build .#amm-module`, or its underlying
logic crate with `nix build .#amm_ffi`. (Each UI is exposed under its own
name, so future apps are `nix run .#<name>` — there is no bare `nix run .`
default.)

## Running inside Logos Basecamp

Basecamp needs three packages to open the AMM tab. Install the wallet module
(`lez_core` 0.6.0), the AMM core module (`amm_module`), and the UI (`amm_ui`).

### 1. Build the LGX packages

From the repository root, build the three development packages. The
`amm_lez_core` input selects the exact wallet revision used by both AMM packages.

```bash
nix build --inputs-from . 'amm_lez_core#lgx' --out-link result-wallet
nix build '.#amm-module-lgx' --out-link result-amm
nix build '.#amm-ui-lgx' --out-link result-ui
```

For a portable Basecamp installation, replace `lgx` with `lgx-portable` in all
three package names and use the `LogosBasecamp` data directory.

### 2. Install into Basecamp

```bash
# Launch Basecamp once to initialise its data directory, then quit (see below)

# Set the data directory path
# macOS:
BASECAMP_DIR="$HOME/Library/Application Support/Logos/LogosBasecampDev"
# Linux:
# BASECAMP_DIR="$HOME/.local/share/Logos/LogosBasecampDev"

# Install wallet and AMM core modules, then the UI plugin.
lgpm --modules-dir "$BASECAMP_DIR/modules" \
  install --file result-wallet/*.lgx
lgpm --modules-dir "$BASECAMP_DIR/modules" \
  install --file result-amm/*.lgx
lgpm --ui-plugins-dir "$BASECAMP_DIR/plugins" \
  install --file result-ui/*.lgx
```

### 3. Launch Basecamp

```bash
nix build 'github:logos-co/logos-basecamp' --accept-flake-config -o ~/.basecamp-result
~/.basecamp-result/bin/LogosBasecamp
```

The AMM UI appears as a new tab in the Basecamp sidebar.

> **Note:** `nix run 'github:logos-co/logos-basecamp'` currently fails with
> `unable to execute ... No such file or directory` — the flake's default app
> is named `logos-basecamp` but the packaged binary is `LogosBasecamp`. Build
> the package and run the `bin/LogosBasecamp` wrapper directly, as above.

### Installing via the Basecamp UI

In Basecamp, open **Package Manager** and use **Install from file** for each
`.lgx`: first the wallet in `result-wallet/`, then the AMM core in `result-amm/`,
then the UI in `result-ui/`.

The **Swap** and **Liquidity** views use the registry snapshot described below.
A registry can supply tokens, pools and the selected network's AMM program and
config ids. Without one, set `AMM_PROGRAM_ID`, `AMM_CONFIG_ID` and
`TOKENS_CONFIG`; the sequencer comes from the wallet config. Run this **from the
repo root** — use absolute paths (`$(pwd)/…`), because `nix run` may not
preserve the working directory, so relative paths won't resolve:

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> \
AMM_CONFIG_ID=<amm-config-id> \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
nix run .#amm-ui
```

Without a program id (from a registry or `AMM_PROGRAM_ID`) the Swap and
Liquidity views stay disabled; without a token source the token picker is
empty. Each is detailed below.

### Registry sources and metadata

`tokenList()` and `poolList()` return the latest `RegistryLoader` snapshot;
calling either slot performs no file, network or chain read. On refresh, the
loader chooses sources in this order:

1. If either `TOKENS_CONFIG` or `AMM_POOLS_CONFIG` is nonempty, both lists use
   local JSON files. An unset, unreadable or invalid file produces an empty
   corresponding list. Local mode never falls back to a registry URL, and its
   lists carry no program or config ids.
2. Otherwise, a nonempty `AMM_REGISTRY_URL` overrides the registry URL saved in
   **Settings**. There is no built-in default URL.
3. With no local source and no URL, both lists are empty (`none`).

`TOKENS_CONFIG`, `AMM_POOLS_CONFIG`, `AMM_REGISTRY_URL` and `AMM_NETWORK` are
read once when the app starts; changing them later has no effect until a
restart. The local files themselves are re-read on every registry refresh.

A URL supplies a multi-network JSON object with `networks`, `tokens` and `pools`
arrays. The selected network filters the tokens and pools; selection prefers
an explicit user pick, then a declared `AMM_NETWORK`, then the first declared
network. The loader also adopts that network's `programIds.amm` and
`ammConfigId` values for module calls.

While fetching, an existing snapshot remains visible, even after changing the
URL. Only when both current lists are empty does the loader first try
`amm-registry-cache.json` in its app-data directory; the stored URL must match
the requested URL. A successful fetch publishes a `remote` snapshot and writes
the cache with the reply URL. The `cache` label means local disk storage of a
prior registry document. Network errors or non-object JSON keep the existing
snapshot; a JSON object without a usable network publishes empty lists.
`registryRevision` changes when a snapshot is published, so views can fetch the
lists again. Settings shows the source and any fetch error.

Token names, symbols and pool labels are source-provided metadata. The loader
verifies no registry signature and does not check those labels or token/pool
identities against chain state. Cache storage adds no authenticity guarantee;
display labels alone do not establish which asset an account represents.

### AMM program id (without a registry)

To execute a swap, the app must submit a transaction against the **exact AMM
program you deployed**: its address determines every pool/vault/config PDA and
the transaction's target. Since LEZ v0.2.5 that address is the account its
`program_loader` header was deployed to, chosen at deploy time — it can't be
derived from `amm.bin` (a binary yields only its ImageID).

A registry network supplies it. Without one, set `AMM_PROGRAM_ID` to the
deployed header account id (base58 or hex), together with `AMM_CONFIG_ID` for
the AMM instance:

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> AMM_CONFIG_ID=<amm-config-id> nix run .#amm-ui
```

Use the IDs from the deployment on your selected sequencer. The module reads
the token and TWAP Oracle program IDs from the AMM config. Pool creation
initializes the Oracle's current tick at a PDA derived by the AMM program from
the pool ID. Each configured TWAP window has its own observations and price
accounts. Without an AMM program ID the Swap view stays disabled.

> **Golden rule:** a redeploy creates a new header account, so a new program id
> and *every* derived PDA. After any redeploy, update `AMM_PROGRAM_ID` (and
> `AMM_CONFIG_ID`) — never point the app at a stale deployment.

### Token list config (local-file setup)

In local mode, the Swap token picker uses the flat JSON array at `TOKENS_CONFIG`
(absolute path). Each entry needs a nonempty `definitionId`. The source can also
supply `symbol`, `name`, `holding` and `decimals`; none of them is checked
against chain state. A holding, when supplied, should be **your own** account
for that token. Shared registries omit per-wallet holdings; the app resolves
them from the connected wallet:

```json
[
  {
    "symbol": "TKA",
    "name": "Token A",
    "definitionId": "9qbX…",
    "holding": "4T69…",
    "decimals": 18
  }
]
```

The quickest start is to copy the checked-in template and edit it:

```bash
cp apps/amm/amm-tokens.json.example apps/amm/amm-tokens.json   # then replace the REPLACE_… placeholders
```

`amm-tokens.json` is git-ignored so your own accounts never get committed.

In local mode, an unset, unreadable or invalid `TOKENS_CONFIG` yields an empty
token list. `definitionId`/`holding` may be given as base58 (as the
wallet/runbook display them) or hex; the module normalizes ids when used.

### Known-pools config (local-file setup)

In local mode, the Pools view uses the flat JSON array at `AMM_POOLS_CONFIG`
(absolute path) and renders one row per entry. Remote and cached registries
provide the same entry shape after filtering to the active network.
`tokenA`/`tokenB` are the display symbols;
`poolId`/`tokenADefinitionId`/`tokenBDefinitionId` identify the pool on-chain.
The swap fee is not a pool field — it is instance-wide (`AmmConfig.swapFeeBps`),
read from the config. (An instance may also set a `protocolFeeBps` — a cut of the
swap fee routed to the protocol; it is an admin/instance concern and is not
surfaced in the UI.)
Adding more pairs is purely a config edit — no app change:

```json
[
  {
    "tokenA": "TKA",
    "tokenB": "TKB",
    "poolId": "9qbX…",
    "tokenADefinitionId": "4T69…",
    "tokenBDefinitionId": "7Zc2…"
  }
]
```

Copy the checked-in template to start (`amm-pools.json` is git-ignored):

```bash
cp apps/amm/amm-pools.json.example apps/amm/amm-pools.json   # then replace the REPLACE_… placeholders
```

In local mode, an unset, unreadable or invalid `AMM_POOLS_CONFIG` yields an
empty pool list. Entries missing `tokenA` or `tokenB` are skipped
individually. The AMM testnet setup script writes this file for the pool(s) it
seeds (see below).

The **Pool** tab in the nav bar is a dropdown with two entries. *Create pool*
opens the new-position / add-liquidity form. *View positions* lists the wallet's
liquidity positions: the program has no "list my positions" read and an LP
definition id cannot be reversed back to its pool, so the app resolves every
pool in the current snapshot and matches each pool's `lpDefinitionId` against
the wallet's token holdings. A pool absent from the snapshot therefore cannot
appear, however many LP tokens the wallet holds for it. Each row shows the pair,
the instance swap fee, the wallet's claim on both reserves (`reserve × lpBalance / lpSupply`,
floored like the program's own payout), and its share of the pool. The list
needs an open wallet.

On the pool detail view, the secondary action reads *Add liquidity* until the
wallet holds LP tokens for that pool; then it becomes *Manage position*, whose
hover dropdown offers both *Add liquidity* and *Remove liquidity*. Removing
opens a sheet with the usual percentage presets and a slider, previews the two
withdrawals through `removeLiquidityQuote`, and submits through
`removeLiquidity` with the previewed amounts as the slippage floors. Note that
every add mints into a *fresh* LP holding, so a wallet that has added twice
holds two LP accounts for one pool; a burn names a single account, so the sheet
draws on the largest and says so when the position spans more than one.

Clicking a row in the Pools list opens the pool detail view, which reads the live pool through
`resolvePoolAccount` and shows the reserve split, spot price, the instance swap fee, LP
supply, an estimate of the fees accrued into the reserves, and the pool's
account ids. Its **Swap** and **Add liquidity** buttons switch tabs with the
pair preselected. Both the detail view and the preselection need
`tokenADefinitionId`/`tokenBDefinitionId` on the entry, and the ids must match
the ones in the registry token snapshot (Swap) and in the token selector's resolved list
(Add liquidity); an entry without them still lists, but its detail view can only
report that the ids are missing. Volume and transaction history are not shown —
the AMM program stores no history to read them from.

Full command with the variables set (absolute paths, from the repo root):

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> \
AMM_CONFIG_ID=<amm-config-id> \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
AMM_POOLS_CONFIG=$(pwd)/apps/amm/amm-pools.json \
nix run .#amm-ui
```

## Validation

New Position validation commands and acceptance criteria live in
[VALIDATION.md](VALIDATION.md).

## Running the UI tests

The UI tests live in `apps/amm/tests/` (e.g. `swap.mjs`). They drive the running
app through a QML inspector: each test connects to the inspector's TCP server,
finds elements, clicks them, and asserts on the resulting state. `swap.mjs`
selects two tokens, enters a sell amount, submits a swap end-to-end, and then
verifies the pool reserves actually changed on-chain (read back from the
sequencer via the app's `resolvePoolAccount`).

Use an initialized LEZ 0.4 pool and wallet for the steps below. The older
testnet setup script uses pre-0.4 token and AMM instruction arguments.

The test framework itself — the `test()` / `run()` / `app.*` API that the tests
import from `test-framework/framework.mjs` — comes from the
[**`logos-co/logos-qt-mcp`**](https://github.com/logos-co/logos-qt-mcp) repo.
It isn't vendored here; the `nix build .#test-framework` step below materializes
it (Nix resolves it via this app's flake inputs, pinned in `flake.lock`).

Run everything **from the repository root** (the `apps/amm` flake can't resolve
`amm_client_ffi` on its own).

**Prerequisites** for the swap test to complete:

- a token list with ≥2 tokens — copy `apps/amm/amm-tokens.json.example` to
  `apps/amm/amm-tokens.json` and fill it in (see [Token list config](#token-list-config-local-file-setup)),
- the AMM program and config ids (see [AMM program id](#amm-program-id-without-a-registry)),
- a running sequencer with a pool + liquidity for that token pair, and an open
  wallet — otherwise the swap resolves to "No pool / no liquidity" and can't submit.

**From scratch:**

```bash
# 1. Build the JS test framework once. The -o path is where the tests expect it
#    (apps/amm/tests/swap.mjs imports ../result-mcp); or set LOGOS_QT_MCP instead.
nix build .#test-framework -o apps/amm/result-mcp

# 2. Terminal 1 — launch the AMM UI with a real, visible window. The inspector
#    listens on localhost:3768. Absolute paths ($(pwd)/…) because nix run may
#    not preserve the working directory.
AMM_DEBUG=1 \
  AMM_PROGRAM_ID=<deployed-amm-program-id> \
  AMM_CONFIG_ID=<amm-config-id> \
  TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
  nix run .#amm-ui

# 3. Terminal 2 — run a test against the running app; watch it drive the UI.
node apps/amm/tests/swap.mjs
```

On failure the test prints the relevant `SwapCard` state and saves screenshot
PNGs next to the test (`apps/amm/tests/swap-*.png`, git-ignored) for inspection.

**Headless CI variant** (no window, launches the app itself, pass/fail only):

```bash
nix build .#integration-test -L
```

It runs every `*.mjs` under `apps/amm/tests/` with `QT_QPA_PLATFORM=offscreen`.

## Updating Dependencies

To update the pinned versions of dependencies in `flake.lock`:

```bash
nix flake update
```

## Troubleshooting

**Stale QML cache after rebuild:**
```bash
QML_DISABLE_DISK_CACHE=1 ~/.basecamp-result/bin/LogosBasecamp
```

**Reset Basecamp data directory:**
```bash
# macOS
rm -rf ~/Library/Application\ Support/Logos/LogosBasecampDev
# Linux
rm -rf ~/.local/share/Logos/LogosBasecampDev
```
