# Token UI

A QML UI application for the token program.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

> **Status:** interactive UI prototype. The **Create** view models the current
> Token Program's fungible and non-fungible definition settings, and **Inspect**
> renders the supplied July 12, 2026 testnet fixture snapshot. It never creates
> accounts, signs, reads a live chain, or submits a transaction.

## Token-definition prototype

The prototype is deliberately local-only. It lets an operator prepare every
currently supported creation shape and see the resulting stored state before a
production transaction path exists:

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

The **Inspect** fixtures are a historical testnet reference, not a live read.
They include fixed, self-authorized, external-authority, revoked-authority, and
metadata-backed fungibles, plus the Glitchlings NFT collection. Display decimals
shown for fungibles are UI inference from the fixture policy; the Token Program
does not store a decimal field.

Metadata-backed definitions and NFT definitions need typed instruction
serialization today because the generic IDL cannot encode the structured
creation arguments. Any production submission implementation must use that
route rather than treating this prototype as a transaction client.

## Wallet / chain integration

This app is a `ui_qml` module with a hand-written C++ backend
(`src/TokenUiBackend.*`, plugin in `src/TokenUiPlugin.*`) that depends on the
core **`logos_execution_zone`** wallet module. The backend calls the core
module's wallet FFI through `m_logos->logos_execution_zone.*` and exposes an
async QtRO surface (`src/TokenUiBackend.rep`) plus an account list model to the
QML view. The wallet backend and navbar are ported from the AMM UI app.

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
  startup the backend **adopts** the already-open wallet (see
  `openOrAdoptWallet()`), surfacing **shared** accounts across apps.

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
