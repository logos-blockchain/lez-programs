# Shared wallet UI

Single source of truth for the wallet surface reused across LEZ QML apps
(`apps/amm`, `apps/token`, …): the account/settings navbar controls, the account
list model, and the wallet backend logic that talks to the core
`logos_execution_zone` module.

This is a plain source tree, **not a flake**. Each app consumes it as a
non-flake path input, sharing it two ways (see any app's `flake.nix`):

```nix
# apps/<app>/flake.nix
inputs.wallet_ui = { url = "path:../common/wallet-ui"; flake = false; };
# C++  → a per-system runCommand overlays src/* into the app's build source.
# QML  → the same runCommand overlays qml/Logos into the app's view dir, so the
#        module ships at <pluginDir>/qml/Logos/Wallet and NavBar imports it with
#        `import "Logos/Wallet"` (relative).
```

## Why a relative import (not `import Logos.Wallet`)

`Logos.Wallet` is structured as a proper QML module (a `qmldir` exporting only
`WalletControl`), but it is imported by **relative path** — `import
"Logos/Wallet"` — rather than by URI. The reason is the runtime: the LEZ
`ui-host` that hosts the view only searches its own baked QML import path
(`<standalone>/lib` + Qt), and does **not** add the app's plugin dir. A bare
`import Logos.Wallet` URI therefore fails with *module "Logos.Wallet" is not
installed*, while a relative import (which resolves against the importing file's
directory, like `import "pages"`) works. Verified by running the app headlessly
against the real standalone/ui-host.

PR #228 makes the URI form work by compiling the module with
`qt_add_qml_module(... RESOURCE_PREFIX /qt/qml)`, which embeds the QML as Qt
resources under `qrc:/qt/qml/Logos/Wallet` — a path Qt always searches — so it
does not depend on the app dir being on the import path. When that lands, each
app's `import "Logos/Wallet"` becomes `import Logos.Wallet` (one line), and the
overlay step is replaced by linking the compiled module. The directory layout
here already matches #228, so that swap is mechanical.

## Contents

| Path | What | Shared how |
| --- | --- | --- |
| `qml/Logos/Wallet/` | The importable **`Logos.Wallet`** QML module. Public: `WalletControl` (the navbar account/settings control). Private: `internal/*` (create-wallet / create-account dialogs, account delegate, icon buttons) — not listed in `qmldir`, so importers only see `WalletControl`. | Installed to `<app>/lib/Logos/Wallet` via `postInstall`; used as `import Logos.Wallet` |
| `src/AccountModel.{h,cpp}` | `QAbstractListModel` of wallet accounts, exposed to QML via `logos.model("<app>_ui", "accountModel")` | Overlaid into `<app>/src/` at build |
| `src/WalletBackendLogic.h` | All wallet behaviour (open/adopt, account create, balances, sequencer settings, reachability) as a CRTP base | Overlaid into `<app>/src/` at build |

## How the QML is shared (`Logos.Wallet`)

Rather than copying QML files into each app, the wallet UI is a real QML module
imported by URI — the same pattern as `Logos.Controls` / `Logos.Theme`. The
standalone/basecamp runtime puts the app's own plugin dir on the QML import
path, so an app that ships `lib/Logos/Wallet/qmldir` can simply:

```qml
import Logos.Wallet
// ...
WalletControl { backend: ...; accountModel: ... }
```

The module boundary keeps the implementation controls private (`internal/`),
and mirrors PR #228's `Logos.Wallet` layout so its richer version (transaction
confirmation, submitted-transaction views, a `WalletProvider` abstraction) can
replace this module's contents without touching the consuming apps.

## How the backend is shared

Every app's backend derives from a QtRO `*SimpleSource` generated from its own
`<App>UiBackend.rep`. Those `.rep` files are byte-identical except for the class
name, so the generated sources expose an identical property/slot surface. That
lets `WalletBackendLogic<Base>` inherit the generated source, reach its protected
PROP setters, and override its pure-virtual slots directly. A host backend is
then a near-empty shell:

```cpp
class TokenUiBackend : public WalletBackendLogic<TokenUiBackendSimpleSource> {
    Q_OBJECT
    Q_PROPERTY(AccountModel* accountModel READ accountModel CONSTANT)
public:
    explicit TokenUiBackend(LogosAPI* api = nullptr, QObject* parent = nullptr)
        : WalletBackendLogic(api, parent, "token_ui", "TokenUI") {}
};
```

The QML wallet components are program-agnostic: the host app passes its concrete
backend replica and account model in via the `backend`/`accountModel`
properties.

## Editing

Change these files once here; both apps pick the change up on their next build.
When adding a new wallet slot or PROP, update every app's `<App>UiBackend.rep`
in lockstep (the surfaces must stay identical) and the CMake `SOURCES` lists if
you add files.
