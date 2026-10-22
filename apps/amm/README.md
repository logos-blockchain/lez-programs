# AMM UI

A QML UI application for the Automated Market Maker (AMM) program.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

## Wallet / chain integration

This app is a `ui_qml` module with a hand-written C++ backend
(`src/AmmUiBackend.*`, plugin in `src/AmmUiPlugin.*`) that depends on the core
**`lez_core`** wallet module. The backend calls the core module's
wallet FFI through `m_logos->lez_core.*` and exposes an async QtRO
surface (`src/AmmUiBackend.rep`) plus an account list model to the QML view.

**Onboarding is non-invasive.** The app opens straight to the Trade screen. The
navbar shows **Connect**, or **Connected** plus the account selector. There is
no path picking — the wallet uses LEZ's canonical home, `~/.lee/wallet/`
(override with `LEE_WALLET_HOME_DIR`, the same var LEZ honors), and its config
(`wallet_config.json`) self-initializes.

**Connect** and wallet creation are different flows:

- When a wallet already exists, **Connect** calls `openExisting()`. That adopts
  an already-open shared wallet session, or opens the canonical wallet. Neither
  path asks for a password.
- When no wallet exists, **Connect** opens the create-wallet dialog. The
  password entered there is passed to `lez_core.create_new` for creation. A
  later **Connect** does not use that dialog as an unlock prompt.

Startup auto-connects an existing wallet unless this app was disconnected
before. **Disconnect** records that preference for this app, clears this app's
open state and fee payer, and leaves the shared `lez_core` wallet available to
other apps. It is not a process-wide lock or close. Swap and liquidity
submission stay rejected while this app shows **Connect**.

Account/keystore sharing follows the runtime:

- **Standalone** (`nix run .#amm-ui`): own core-module instance, but the canonical
  `~/.lee/wallet` keystore is shared with the LEZ wallet UI and any other LEZ
  app on the machine. A previously-created wallet auto-opens on launch unless
  this app was disconnected.
- **Inside Basecamp**: the core wallet module is a single shared instance, so on
  startup `WalletController::openOnStartup()` adopts that already-open wallet,
  surfacing **shared** accounts across apps.

> Follow-up: the app reconstructs the wallet paths itself because the
> `lez_core` module only exposes path-taking `create_new`/`open`.
> LEZ's wallet FFI now provides path-free variants (`wallet_ffi_create_new_default`,
> `wallet_ffi_open_default`, plus `wallet_ffi_default_config_path` /
> `_storage_path` / `wallet_ffi_wallet_exists_default`). Once the module surfaces
> those over QtRO, the app can drop its `defaultWalletHome/Config/Storage` logic.

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

This builds and runs the application in development mode. The Logos bridge is unavailable in standalone mode, but the UI layout and mock data are fully functional.

Build just the AMM core module with `nix build .#amm-module`, or its underlying
logic crate with `nix build .#amm_ffi`. (Each UI is exposed under its own
name, so future apps are `nix run .#<name>` — there is no bare `nix run .`
default.)

## Running inside Logos Basecamp

This app is a UI plugin that depends on the **core wallet module**
`lez_core` (see the Wallet / chain integration section above). Both
have to be installed into Basecamp — the UI plugin alone will show the AMM tab
but fail to open it with `Failed to load core dependencies for amm_ui`.

### 1. Build the LGX packages

```bash
# The AMM UI plugin — development variant (requires nix store at runtime)
nix build '.#lgx' --out-link result-lgx

# Portable variant (self-contained, works without nix)
nix build '.#lgx-portable' --out-link result-lgx-portable

# The core wallet module it depends on. These are the same immutable upstream
# revisions used by this app's flake, including the merged macOS Metal fix.
nix build 'github:logos-blockchain/logos-execution-zone-module?rev=d70225ced646934d2294fd9e8f8b03615c104b80#lgx' \
  --override-input logos-execution-zone \
  'github:logos-blockchain/logos-execution-zone?rev=a7e06a660940a00093b1760560d37ff84aff5a05' \
  --out-link result-core
```

### 2. Install into Basecamp

```bash
# Launch Basecamp once to initialise its data directory, then quit (see below)

# Set the data directory path
# macOS:
BASECAMP_DIR="$HOME/Library/Application Support/Logos/LogosBasecampDev"
# Linux:
# BASECAMP_DIR="$HOME/.local/share/Logos/LogosBasecampDev"

# Install the core wallet module first (into the modules dir), then the UI plugin
lgpm --modules-dir "$BASECAMP_DIR/modules" \
  install --file result-core/*.lgx
lgpm --ui-plugins-dir "$BASECAMP_DIR/plugins" \
  install --file result-lgx/*.lgx
```

> **Note:** Use matching variants throughout — dev with dev, portable with portable. Mixing variants causes loading failures. The portable build uses the `LogosBasecamp` data directory instead of `LogosBasecampDev`.

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

Alternatively, use the built-in package manager. Install both packages — the
core module from `result-core/` and the UI plugin from `result-lgx/`:

1. Launch Basecamp
2. Open Package Manager
3. Select "Install from file"
4. Choose the core module `.lgx` from `result-core/`, then the UI plugin `.lgx`
   from `result-lgx/`

To actually use the on-chain views (**Swap** and **Liquidity**) without a
registry you must also set `AMM_PROGRAM_ID`, `AMM_CONFIG_ID` and a token list
(explained below). Both views read the same ones — the AMM program from the
registry network or `AMM_PROGRAM_ID`, its instance from the registry network or
`AMM_CONFIG_ID`, the token and pool lists from the registry snapshot, and the
sequencer from the wallet config. Run this **from the repo root** — use
absolute paths (`$(pwd)/…`), because `nix run` may not preserve the working
directory, so relative paths won't resolve:

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> \
AMM_CONFIG_ID=<amm-config-id> \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
nix run .#amm-ui
```

Without a program id (from a registry network or `AMM_PROGRAM_ID`) the Swap and
Liquidity views stay disabled. Without a registry snapshot the token picker is
empty. Each is detailed below.

### AMM program id (required for swaps and liquidity)

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

`apps/amm/tests/testnet/setup-amm-testnet.sh` prints both after deploying. The
app reads the on-chain AMM config account to discover the TWAP oracle program
id for the pool's current-tick PDA. Without a program id the Swap view stays
disabled (no pool can be resolved).

> **Golden rule:** a redeploy creates a new header account, so a new program id
> and *every* derived PDA. After any redeploy, update `AMM_PROGRAM_ID` (and
> `AMM_CONFIG_ID`) — never point the app at a stale deployment.

### Where the token and pool lists come from

`tokenList()` and `poolList()` return the latest `RegistryLoader` snapshot.
They do not read a file on each call. `refresh()` picks that snapshot in this
order:

1. Local files, when `TOKENS_CONFIG` or `AMM_POOLS_CONFIG` is set. Either
   variable selects this path, and it wins over a remote registry. These files
   are bare JSON arrays and carry no network, program id, or config id.
2. Otherwise `AMM_REGISTRY_URL`, when it is set.
3. Otherwise the registry URL saved in Settings.
4. When none of those is set, the snapshot is empty (`source` `"none"`).

A remote refresh can serve the on-disk cache for that same URL first
(`source` `"cache"`), then replace it when the fetch succeeds (`source`
`"remote"`). A failed fetch keeps the snapshot already on screen. Registry
entries are filtered to the active network. Symbols, names, and decimals in
the snapshot are display metadata from that file or registry. They are not
chain-verified, and a placeholder id in the list is not proof that a later
transaction will accept it.

Clearing the saved URL, or switching onto the local-file path, drops the
registry's selected program id and config id. Later operations then use
`AMM_PROGRAM_ID` and `AMM_CONFIG_ID`.

### Token list config (local-file source for the Swap token picker)

The Swap view's token picker is config-driven: it doesn't derive tokens from
chain state. On the local-file path it reads a flat JSON list from the
`TOKENS_CONFIG` environment variable (absolute path). Each entry needs, at
minimum, the token's `definitionId` and **your own** `holding` account address
for that token (the account the wallet will sign transfers from/to for that
token):

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

If no registry snapshot is loaded, the token picker stays empty and no swap can
be started. On the local-file path, an unset, unreadable, or invalid
`TOKENS_CONFIG` produces that empty snapshot. `definitionId`/`holding` may be
given as base58 (as the wallet/runbook display them) or hex — the app
normalizes both to hex.

### Known-pools config (local-file source for the Pools list)

The Pools view uses the same snapshot. On the local-file path it reads a flat
JSON list from the `AMM_POOLS_CONFIG` environment variable (absolute path) and
renders one row per entry. `tokenA`/`tokenB` are the display symbols;
`poolId`/`tokenADefinitionId`/`tokenBDefinitionId` identify the pool on-chain.
The swap fee is not a pool field — it is instance-wide (`AmmConfig.swapFeeBps`),
read from the config. (An instance may also set a `protocolFeeBps` — a cut of the
swap fee routed to the protocol; it is an admin/instance concern and is not
surfaced in the UI. See [AMM protocol fees](../../docs/amm-protocol-fees.md).)
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

If the snapshot has no pools, the Pools list shows its empty state. On the
local-file path that happens when `AMM_POOLS_CONFIG` is unset, unreadable, or
not a valid JSON array. Entries missing `tokenA` or `tokenB` are skipped
individually. The AMM testnet setup script writes this file for the pool(s) it
seeds (see below).

The **Pool** tab in the nav bar is a dropdown with two entries. *Create pool*
opens the new-position / add-liquidity form. *View positions* lists the wallet's
liquidity positions: the program has no "list my positions" read and an LP
definition id cannot be reversed back to its pool, so the app resolves every
pool in this config and matches each pool's `lpDefinitionId` against the
wallet's token holdings. A pool that is not in the config therefore cannot
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
the ones in the token snapshot (Swap) and in the token selector's resolved list
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

> **For the fully isolated, script-driven test flow** (a dedicated wallet from a
> fixed mnemonic + auto-created pool + isolated token config, touching nothing in
> your local setup), see **[`apps/amm/tests/README.md`](tests/README.md)**. The
> steps below run the test against *your own* local wallet/config instead.

The test framework itself — the `test()` / `run()` / `app.*` API that the tests
import from `test-framework/framework.mjs` — comes from the
[**`logos-co/logos-qt-mcp`**](https://github.com/logos-co/logos-qt-mcp) repo.
It isn't vendored here; the `nix build .#test-framework` step below materializes
it (Nix resolves it via this app's flake inputs, pinned in `flake.lock`).

Run everything **from the repository root** (the `apps/amm` flake can't resolve
`amm_client_ffi` on its own).

**Prerequisites** for the swap test to complete:

- a token list with ≥2 tokens — copy `apps/amm/amm-tokens.json.example` to
  `apps/amm/amm-tokens.json` and fill it in (see [Token list config](#token-list-config-required-for-the-swap-token-picker)),
- the AMM program binary (see [AMM program binary](#amm-program-binary-required-for-swaps)),
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
