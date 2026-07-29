# AMM UI

A QML UI application for the Automated Market Maker (AMM) program.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

## Wallet / chain integration

This app is a `ui_qml` module with a hand-written C++ backend
(`src/AmmUiBackend.*`, plugin in `src/AmmUiPlugin.*`) that depends on the core
**`logos_execution_zone`** wallet module. The backend calls the core module's
wallet FFI through `m_logos->logos_execution_zone.*` and exposes an async QtRO
surface (`src/AmmUiBackend.rep`) plus an account list model to the QML view.

**Onboarding is non-invasive.** The app opens straight to the Trade screen; the
navbar shows **Connect** (opens a password-only modal) or **Connected** + the
account selector. There is no path picking — the wallet uses LEZ's canonical
home, `~/.lee/wallet/` (override with `LEE_WALLET_HOME_DIR`, the same var LEZ
honors), and its config (`wallet_config.json`) self-initializes.

Account/keystore sharing follows the runtime:

- **Standalone** (`nix run .#amm-ui`): own core-module instance, but the canonical
  `~/.lee/wallet` keystore is shared with the LEZ wallet UI and any other LEZ
  app on the machine. A previously-created wallet auto-opens on launch.
- **Inside Basecamp**: the core wallet module is a single shared instance, so on
  startup the backend **adopts** the already-open wallet (see
  `openOrAdoptWallet()`), surfacing **shared** accounts across apps.

> Follow-up: the app reconstructs the wallet paths itself because the
> `logos_execution_zone` module only exposes path-taking `create_new`/`open`.
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
`logos_execution_zone` (see the Wallet / chain integration section above). Both
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

To actually use the on-chain views (**Swap** and **Liquidity**) you must also
set `AMM_PROGRAM_BIN` and `TOKENS_CONFIG` (both explained below). Both views read
the same two — the AMM program id is derived from `AMM_PROGRAM_BIN`, the token
set from `TOKENS_CONFIG`, and the sequencer from the wallet config. Run this
**from the repo root** — use absolute paths (`$(pwd)/…`), because `nix run` may
not preserve the working directory, so relative paths won't resolve:

```bash
AMM_PROGRAM_BIN=$(pwd)/programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
nix run .#amm-ui
```

Without `AMM_PROGRAM_BIN` the Swap and Liquidity views stay disabled; without
`TOKENS_CONFIG` the token picker is empty. Each is detailed below.

### AMM program binary (required for swaps and liquidity)

To execute a swap, the app must submit a transaction against the **exact AMM
program you deployed** (its ELF determines the program id, and therefore every
pool/vault/config PDA and the transaction's target). The app therefore needs the
deployed `amm.bin` bytes at runtime — it does **not** derive them from the wallet
module (whose embedded AMM program may differ from your deployment).

Point the app at your deployed binary with the `AMM_PROGRAM_BIN` environment
variable (absolute path):

```bash
AMM_PROGRAM_BIN=/abs/path/to/amm.bin nix run .#amm-ui
```

This is the same `amm.bin` you deployed via the testnet runbook
(`programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin`).
The app reads it, derives the AMM program id (the binary's RISC Zero Image ID,
via `amm_ffi`'s `program_id` op), and reads the on-chain AMM config account to
discover the TWAP oracle program id for the pool's current-tick PDA. If `AMM_PROGRAM_BIN` is unset or unreadable, the
Swap view stays disabled (no pool can be resolved).

> **Golden rule (from the runbook):** recompiling the AMM changes its program id
> and *every* derived PDA. After any redeploy, point `AMM_PROGRAM_BIN` at the new
> `amm.bin` — never mix a stale binary with a fresh deployment.

### Token list config (required for the Swap token picker)

The Swap view's token picker is config-driven: it doesn't derive tokens from
chain state, it reads a flat JSON list from the `TOKENS_CONFIG` environment
variable (absolute path). Each entry needs, at minimum, the token's
`definitionId` and **your own** `holding` account address for that token (the
account the wallet will sign transfers from/to for that token):

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

If `TOKENS_CONFIG` is unset, unreadable, or not a valid JSON array, the token
picker stays empty (a `qWarning` naming the exact cause is logged to stderr; no
swap can be started). `definitionId`/`holding` may be given as base58 (as the
wallet/runbook display them) or hex — the app normalizes both to hex.

Full command with both variables set (absolute paths, from the repo root):

```bash
AMM_PROGRAM_BIN=$(pwd)/programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
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
sequencer via the app's `resolvePool`).

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
  AMM_PROGRAM_BIN=$(pwd)/programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
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
