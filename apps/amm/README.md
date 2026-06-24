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

- **Standalone** (`nix run .`): own core-module instance, but the canonical
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

## Running the UI

Start the UI with:

```bash
nix run .
```

This builds and runs the application in development mode.

## Updating Dependencies

To update the pinned versions of dependencies in `flake.lock`:

```bash
nix flake update
```
