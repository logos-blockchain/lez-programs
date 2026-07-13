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

> Follow-up: the wallet FFI requires explicit `config_path`/`storage_path` even
> though the wallet crate already defines defaults (`~/.lee/wallet`,
> `from_path_or_initialize_default`). A `wallet_ffi_create_new_default()` /
> `_open_default()` upstream would let the app drop its path handling entirely.

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

The app is built from the **repository-root** flake (which also provides the
`amm_client_ffi` library it links). From the repo root, launch it with its named
attribute:

```bash
nix run .#amm-ui
```

This builds and runs the application in development mode. The Logos bridge is unavailable in standalone mode, but the UI layout and mock data are fully functional.

Build just the FFI crate with `nix build .#amm_client_ffi`. (Each UI is exposed
under its own name, so future apps are `nix run .#<name>` — there is no bare
`nix run .` default.)

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

# The core wallet module it depends on. Use the same rev this app pins as the
# `logos_execution_zone` input in flake.nix so the ImageID/ABI match.
nix build 'github:logos-blockchain/logos-execution-zone-module?rev=d2e9400ac06c3cdbfc2405b4f153fff9841a453c#lgx' \
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
