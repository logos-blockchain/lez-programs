# Token UI

A Basecamp-compatible QML UI module for creating and inspecting Token Program
assets through the `token_module` core module.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

The UI creates fresh public wallet accounts, submits fungible and
non-fungible definition transactions through `token_module`, and reads the
connected wallet's Token-owned accounts for the Inspect view. Raw `u128`
values stay decimal strings across the QML/C++ boundary.

## Token definitions

The Create view supports every current definition shape:

- Fungible definitions: raw `u128` supply; fixed (`None`), self, or external
  mint authority; metadata omitted or linked.
- Non-fungible definitions: printable supply plus required metadata, with an
  initial master holding that controls printing.
- Metadata: `Simple` or `Expanded` standard, URI, and creators string. The
  program initializes `primary_sale_date` to `0`; it has no creation input for
  decimals, symbol, description, image, royalties, collection, or mutable
  metadata.
- Target accounts: definition, first holding/master, and metadata (when used)
  are surfaced because each must be fresh and authorized at submission time.

Before a wallet is connected, Inspect shows bundled example records so the
module remains useful as a visual shell. After connection it replaces those
records with live `walletTokenAccounts`, `inspectDefinition`, and
`inspectMetadata` results. Display decimals shown for bundled examples are UI
inference; the Token Program does not store a decimal field.

Metadata-backed and NFT definitions use the typed token-module submission path;
the UI does not assemble transaction instructions itself.

## Wallet / chain integration

This app is a `ui_qml` module with a hand-written C++ backend
(`src/TokenUiBackend.*`, plugin in `src/TokenUiPlugin.*`) that depends on the
core **`logos_execution_zone`** wallet module and **`token_module`** Token
Program API. The backend exposes an async QtRO surface
(`src/TokenUiBackend.rep`) plus an account list model to the QML view. Wallet
session behavior and the `Logos.Wallet` control come from
`apps/shared/wallet`.

**Onboarding is non-invasive.** The app opens straight to the first screen; the
navbar shows **Connect** (opens a password-only modal) or **Connected** + the
account selector. There is no path picking — the wallet uses LEZ's canonical
home, `~/.lee/wallet/` (override with `LEE_WALLET_HOME_DIR`, the same var LEZ
honors), and its config (`wallet_config.json`) self-initializes.

Account/keystore sharing follows the runtime:

- **Standalone** (`nix run .`): own core-module instance, but the canonical
  `~/.lee/wallet` keystore is shared with the LEZ wallet UI and any other LEZ
  app on the machine. A previously-created wallet auto-opens on launch.
- **Inside Basecamp**: the core wallet module is a single shared instance, so on
  startup `LogosWalletProvider` adopts the already-open wallet, surfacing
  **shared** accounts across apps.

## Setup

This project requires Nix with experimental features enabled. If you haven't already, enable them permanently:

```bash
mkdir -p ~/.config/nix && echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

## Running the UI

From the repository root, start the packaged UI with:

```bash
nix run .#token-ui
```

This builds and runs the application in development mode.

## Updating Dependencies

To update the pinned versions of dependencies in `flake.lock`:

```bash
nix flake update
```
