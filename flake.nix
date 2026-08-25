{
  description = "LEZ programs — host client modules and FFIs";

  # Fetch the prebuilt lez_core wallet-ffi (and its deps, e.g. `ring`) from the
  # Logos Cachix instead of compiling locally — CI builds the module on macos-15 +
  # ubuntu with its native LEZ v0.2.2 pin, so this hits for both. Building the
  # wallet-ffi from source fails on Apple Silicon (ring's ARM asm vs the nix
  # cc-wrapper), so the cache is effectively required on macOS. First `nix build`
  # prompts to trust this config (or set accept-flake-config = true).
  nixConfig = {
    extra-substituters = [ "https://logos-co.cachix.org" ];
    extra-trusted-public-keys = [
      "logos-co.cachix.org-1:12K8609ho1pCt0erUQrOrs/KmOuhWq/EhVwhzQRb35E="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # The AMM QML UI module (apps/amm) is built from this same flake so it can
    # reference the amm_ffi crate package via `self` — no filesystem
    # path or git-remote reference to this repo is needed (see apps/amm/flake.nix
    # history: a `git+file://` URL pointing at a local checkout is
    # machine-specific and not portable).
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve it
    # as a module dependency.
    #
    # Upstream logos-blockchain/logos-execution-zone-module, fix/generic-tx-instruction-bstr
    # branch: carries the QtRO serialization fix — send_generic_public_transaction's
    # `instruction` uses a byte-string IPC type so the args survive the cross-process
    # boundary (the fix is NOT on `main`; see docs/amm-swap-qtro-serialization-bug.md).
    #
    # No LEZ override: the branch natively pins logos-execution-zone v0.2.2, which is the
    # deployed sequencer's version — the wallet-ffi and sequencer must agree on the
    # JSON-RPC API and the wallet-config schema (v0.2.1 introduced the `sequencers` list).
    # Keeping the native pin also means this is the exact derivation upstream CI built, so
    # the wallet-ffi is fetched from the Logos nix binary cache instead of compiling `ring`
    # locally (which fails under the nix cc-wrapper on Apple Silicon).
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr";

  };

  outputs =
    inputs@{ self, nixpkgs, flake-utils, crane, rust-overlay, logos-module-builder, lez_core, ... }:
    let
      crateOutputs = flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # Honor the repo's pinned toolchain (rust-toolchain.toml -> 1.94.0).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Whole Cargo workspace, excluding build outputs and non-Rust assets.
        # Crane still sees Cargo.lock and every path dependency, while local
        # target directories cannot become multi-gigabyte Nix source snapshots.
        src = craneLib.cleanCargoSource ./.;

        # Scope every build to one host FFI package. The workspace also has
        # methods crates whose build scripts compile RISC Zero guests; package
        # scoping keeps those target-specific builds out of host module builds.
        mkHostFfi =
          {
            package,
            sourceDir,
            header,
          }:
          let
            commonArgs = {
              inherit src;
              strictDeps = true;
              pname = package;
              version = "0.1.0";
              cargoExtraArgs = "-p ${package}";
              doCheck = false;
              # cbindgen runs as a Rust build-dependency from build.rs.
            };
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          in
          craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              postInstall =
                ''
                  mkdir -p $out/include
                  cp ${sourceDir}/include/${header} $out/include/
                ''
                + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
                  # Use an absolute store install-name because the module
                  # builder does not stage external dylibs beside the plugin.
                  if [ -f $out/lib/lib${package}.dylib ]; then
                    install_name_tool -id "$out/lib/lib${package}.dylib" $out/lib/lib${package}.dylib
                  fi
                '';
            }
          );

        ammFfi = mkHostFfi {
          package = "amm_ffi";
          sourceDir = "modules/amm/ffi";
          header = "amm_ffi.h";
        };

        tokenFfi = mkHostFfi {
          package = "token_ffi";
          sourceDir = "modules/token/ffi";
          header = "token_ffi.h";
        };
      in
      {
        packages.default = ammFfi;
        packages.amm_ffi = ammFfi;
        packages.token_ffi = tokenFfi;
      }
    );

      # The AMM QML UI module (apps/amm). It links no amm_ffi library of its
      # own — the AMM logic lives in the amm_module core module, which the UI
      # depends on (declared in apps/amm/metadata.json, reached via
      # modules().amm_module in the backend) alongside the lez_core
      # wallet module.
      appOutputs = logos-module-builder.lib.mkLogosQmlModule {
        src = ./apps/amm;
        configFile = ./apps/amm/metadata.json;
        # amm_module isn't a flake input — it's built from this same flake below
        # (ammModuleOutputs) — so inject it into flakeInputs under its dependency
        # name so the builder's dependency resolver finds it (reads its .lidl to
        # generate the modules().amm_module wrapper). The wallet module stays a
        # direct dep too, so both the UI and amm_module resolve the one shared
        # wallet instance.
        flakeInputs = inputs // { amm_module = ammModuleOutputs; };
        # The UI links no external lib of its own — the AMM brain (amm_ffi) is
        # linked by amm_module, which the UI reaches via modules().amm_module.
        externalLibInputs = { };
        # The AMM UI links the shared C++ wallet access lib and bundles the
        # Logos.Wallet QML module (apps/shared/wallet). apps/amm/flake.nix wires
        # these via its `shared_wallet` input; when built from this root flake
        # the source lives in-tree, so point CMake straight at it and stage the
        # built QML module the same way. Keep in sync with apps/amm/flake.nix.
        preConfigure = ''
          cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${./apps/shared/wallet}")
          cmakeFlagsArray+=("-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")
        '';
        postInstall = ''
          walletQmlDescriptor="$(find "$PWD" -type f -path '*/shared-wallet/qml/Logos/Wallet/qmldir' -print -quit)"
          if [ -z "$walletQmlDescriptor" ]; then
            echo "Built Logos.Wallet QML module not found"
            exit 1
          fi
          walletQmlDir="$(dirname "$walletQmlDescriptor")"
          # Stage Logos.Wallet at the plugin ROOT as PURE QML (strip every
          # plugin/resource directive). Same rationale as tokenAppOutputs below:
          # Basecamp rejects `prefer :/qt/qml/...` with "Invalid null URL", and
          # keeping the module at the root (never under qml/) avoids leaking
          # qml/NavBar.qml et al. into Basecamp's shared import path where it
          # would collide with other plugins' identically-named types. Standalone
          # reaches this root module via QML_IMPORT_PATH (see the amm-ui app
          # wrapper). Keep in sync with tokenAppOutputs / apps/amm/flake.nix.
          walletQmlInstallDir="$out/lib/Logos/Wallet"
          mkdir -p "$walletQmlInstallDir"
          cp -r "$walletQmlDir/." "$walletQmlInstallDir/"
          grep -vE '^(linktarget|optional plugin|plugin|classname|typeinfo|prefer)([[:space:]]|$)' \
            "$walletQmlInstallDir/qmldir" > "$walletQmlInstallDir/qmldir.pureqml"
          mv "$walletQmlInstallDir/qmldir.pureqml" "$walletQmlInstallDir/qmldir"
          test -f "$walletQmlInstallDir/qmldir"
        '';
      };

      # Token QML UI (apps/token). This UI consumes the token_module core
      # module through its generated Logos SDK and the reusable shared wallet
      # access/QML module. Keep the output separate so Basecamp can install or
      # run `token-ui` independently from the AMM UI.
      tokenAppOutputs = logos-module-builder.lib.mkLogosQmlModule {
        src = ./apps/token;
        configFile = ./apps/token/metadata.json;
        flakeInputs = inputs // { token_module = tokenModuleOutputs; };
        externalLibInputs = { };
        preConfigure = ''
          cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${./apps/shared/wallet}")
          cmakeFlagsArray+=("-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")
        '';
        postInstall = ''
          walletQmlDescriptor="$(find "$PWD" -type f -path '*/shared-wallet/qml/Logos/Wallet/qmldir' -print -quit)"
          if [ -z "$walletQmlDescriptor" ]; then
            echo "Built Logos.Wallet QML module not found"
            exit 1
          fi
          walletQmlDir="$(dirname "$walletQmlDescriptor")"
          # Stage the shared Logos.Wallet module at the plugin ROOT (NOT under
          # qml/), and strip every plugin/resource directive from its qmldir so
          # it loads as PURE QML from the co-located .qml files. Two reasons:
          #   1. Basecamp's QML sandbox rejects `prefer :/qt/qml/...` (and the
          #      `classname`/`typeinfo`/`linktarget` it pairs with) with
          #      "Invalid null URL" — that compiled resource isn't registered
          #      there. The .qml files are physically present, so the module
          #      resolves without any native plugin; Token drives wallet ops
          #      through TokenUiBackend regardless.
          #   2. Basecamp adds the *containing dir* of every discovered Logos.*
          #      module to a shared QML import path. Keeping Logos.Wallet at the
          #      root means only the root is shared — putting it under qml/ would
          #      also leak qml/NavBar.qml et al. into that shared namespace and
          #      collide with other plugins' identically-named types (e.g. amm's
          #      NavBar). Standalone reaches this root module via QML_IMPORT_PATH
          #      (see the token-ui app wrapper), not a qml/ stub. Keep in sync
          #      with apps/amm.
          walletQmlInstallDir="$out/lib/Logos/Wallet"
          mkdir -p "$walletQmlInstallDir"
          cp -r "$walletQmlDir/." "$walletQmlInstallDir/"
          grep -vE '^(linktarget|optional plugin|plugin|classname|typeinfo|prefer)([[:space:]]|$)' \
            "$walletQmlInstallDir/qmldir" > "$walletQmlInstallDir/qmldir.pureqml"
          mv "$walletQmlInstallDir/qmldir.pureqml" "$walletQmlInstallDir/qmldir"
          test -f "$walletQmlInstallDir/qmldir"
        '';
      };

      # Expose the AMM QML UI as a NAMED app/package (`amm-ui`) rather than
      # `<sys>.default` — the repo is expected to grow more UIs over time,
      # each runnable via `nix run .#<name>`. `appOutputs.apps.<sys>.default`
      # and `appOutputs.packages.<sys>.default` (the UI launcher and its
      # bundle, respectively) are renamed to `amm-ui`; no `default` survives
      # for either attribute set.
      appApps = appOutputs.apps or { };
      appPkgs = appOutputs.packages or { };
      tokenAppApps = tokenAppOutputs.apps or { };
      tokenAppPkgs = tokenAppOutputs.packages or { };

      # Keep the token UI's Basecamp install artifacts addressable after the
      # core token module is merged into the same package set. Both builders
      # expose an `lgx` attribute, so a named alias prevents the core module
      # package from shadowing the UI package.
      tokenUiPackages = builtins.mapAttrs (
        system: attrs:
        (if attrs ? lgx then { token-ui-lgx = attrs.lgx; } else { })
        // (if attrs ? lgx-portable then { token-ui-lgx-portable = attrs.lgx-portable; } else { })
        // (if attrs ? install then { token-ui-install = attrs.install; } else { })
        // (if attrs ? install-portable then { token-ui-install-portable = attrs.install-portable; } else { })
      ) tokenAppPkgs;

      # Same aliasing for the AMM UI's Basecamp install artifacts, so its lgx
      # isn't shadowed by the amm core module's bare `lgx` in the merged set.
      ammUiPackages = builtins.mapAttrs (
        system: attrs:
        (if attrs ? lgx then { amm-ui-lgx = attrs.lgx; } else { })
        // (if attrs ? lgx-portable then { amm-ui-lgx-portable = attrs.lgx-portable; } else { })
        // (if attrs ? install then { amm-ui-install = attrs.install; } else { })
        // (if attrs ? install-portable then { amm-ui-install-portable = attrs.install-portable; } else { })
      ) appPkgs;

      # AMM core module (modules/amm): the AMM business logic as a headless
      # `core` Logos module. It links the amm_ffi crate (the transport-
      # independent AMM brain, resolved via `self`) and depends on the
      # lez_core wallet module (declared in modules/amm/metadata.json,
      # reached via modules().lez_core in the impl). Exposed as the
      # `amm-module` package; no UI/app output.
      ammModuleOutputs = logos-module-builder.lib.mkLogosModule {
        src = ./modules/amm;
        configFile = ./modules/amm/metadata.json;
        flakeInputs = inputs;
        externalLibInputs = {
          amm_ffi = { input = self; packages.default = "amm_ffi"; };
        };
      };
      ammModulePkgs = ammModuleOutputs.packages or { };

      # Alias the AMM core module's Basecamp install artifacts (explicit
      # `amm-module-lgx` / `amm-module-install`), same collision reason as the
      # token module aliases below.
      ammModuleAliases = builtins.mapAttrs (
        system: attrs:
        (if attrs ? lgx then { amm-module-lgx = attrs.lgx; } else { })
        // (if attrs ? install then { amm-module-install = attrs.install; } else { })
      ) ammModulePkgs;

      # Token core module (modules/token): complete Token Program inspection
      # and transaction planning/orchestration surface. The Qt-free universal
      # module links token_ffi and reuses the host-loaded wallet module.
      tokenModuleOutputs = logos-module-builder.lib.mkLogosModule {
        src = ./modules/token;
        configFile = ./modules/token/metadata.json;
        flakeInputs = inputs;
        externalLibInputs = {
          token_ffi = { input = self; packages.default = "token_ffi"; };
        };
        tests = {
          dir = ./modules/token/tests;
          mockCLibs = [ "token_ffi" ];
        };
      };
      tokenModulePkgs = tokenModuleOutputs.packages or { };

      # Alias the token core module's Basecamp install artifacts. The bare
      # `lgx` / `install` attrs collide across builders in the merged package
      # set (last write wins), so expose an explicit `token-module-lgx` /
      # `token-module-install` that resolves unambiguously.
      tokenModuleAliases = builtins.mapAttrs (
        system: attrs:
        (if attrs ? lgx then { token-module-lgx = attrs.lgx; } else { })
        // (if attrs ? install then { token-module-install = attrs.install; } else { })
      ) tokenModulePkgs;

      # Wrap the app launcher to export DYLD_FALLBACK_LIBRARY_PATH pointing at the
      # amm_ffi lib. The logos module builder links the plugin against
      # @rpath/libamm_ffi.dylib but does NOT stage that dylib into the
      # plugin-dir it loads at runtime, so dlopen fails with "Failed to load UI
      # plugin". Adding the crate's store lib dir to DYLD's fallback search path
      # lets the loader find it (the store path stays in the closure).
      # Also prepend the AMM UI module's `lib` dir to QML_IMPORT_PATH so the
      # standalone shell resolves `import Logos.Wallet` from the plugin ROOT
      # (staged there, never under qml/, by the appOutputs postInstall — see the
      # wrapTokenQmlImportPath rationale). logos-standalone-app appends any
      # pre-existing QML_IMPORT_PATH to what it sets.
      wrapWithDyld = system: app:
        let
          pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
          ammFfi = crateOutputs.packages.${system}.amm_ffi;
          moduleDir = appPkgs.${system}.default;
        in
        app // {
          program = "${pkgs.writeShellScript "run-amm-ui" ''
            export DYLD_FALLBACK_LIBRARY_PATH="${ammFfi}/lib''${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
            export QML_IMPORT_PATH="${moduleDir}/lib''${QML_IMPORT_PATH:+:$QML_IMPORT_PATH}"
            exec ${app.program} "$@"
          ''}";
        };

      renamedApps = builtins.mapAttrs (
        system: attrs:
        (builtins.removeAttrs attrs [ "default" ]) // (if attrs ? default then { amm-ui = wrapWithDyld system attrs.default; } else { })
      ) appApps;

      # Prepend the token UI module's `lib` dir to QML_IMPORT_PATH before the
      # standalone shell runs. The shared Logos.Wallet module is staged at the
      # plugin ROOT (lib/Logos/Wallet), NOT under the view dir (lib/qml) — see
      # the tokenAppOutputs postInstall for why (Basecamp shared-import-path
      # collisions). But the standalone shell only searches the view dir for
      # unqualified imports, so `import Logos.Wallet` from lib/qml/Main.qml would
      # not resolve without help. logos-standalone-app appends any pre-existing
      # QML_IMPORT_PATH to the paths it sets, so exporting lib here makes the
      # root module resolvable with no qml/ stub. tokenAppPkgs.<sys>.default is
      # the module derivation whose /lib is the plugin dir the app loads.
      wrapTokenQmlImportPath = system: app:
        let
          pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
          moduleDir = tokenAppPkgs.${system}.default;
        in
        app // {
          program = "${pkgs.writeShellScript "run-token-ui" ''
            export QML_IMPORT_PATH="${moduleDir}/lib''${QML_IMPORT_PATH:+:$QML_IMPORT_PATH}"
            exec ${app.program} "$@"
          ''}";
        };

      renamedTokenApps = builtins.mapAttrs (
        system: attrs:
        (builtins.removeAttrs attrs [ "default" ]) // (if attrs ? default then { token-ui = wrapTokenQmlImportPath system attrs.default; } else { })
      ) tokenAppApps;

      mergedApps = builtins.mapAttrs (
        system: attrs:
        attrs // (renamedTokenApps.${system} or { })
      ) renamedApps;

      mergedPackages = builtins.mapAttrs (
        system: cratePkgs:
        let
          appSysPkgs = appPkgs.${system} or { };
          ammUiSysPkgs = ammUiPackages.${system} or { };
          tokenAppSysPkgs = tokenAppPkgs.${system} or { };
          tokenUiSysPkgs = tokenUiPackages.${system} or { };
          ammModSysPkgs = ammModulePkgs.${system} or { };
          ammModAliasPkgs = ammModuleAliases.${system} or { };
          tokenModSysPkgs = tokenModulePkgs.${system} or { };
          tokenModAliasPkgs = tokenModuleAliases.${system} or { };
        in
        (builtins.removeAttrs cratePkgs [ "default" ])
        // (builtins.removeAttrs appSysPkgs [ "default" ])
        // (if appSysPkgs ? default then { amm-ui = appSysPkgs.default; } else { })
        // ammUiSysPkgs
        // (builtins.removeAttrs tokenAppSysPkgs [ "default" ])
        // (if tokenAppSysPkgs ? default then { token-ui = tokenAppSysPkgs.default; } else { })
        // tokenUiSysPkgs
        // (builtins.removeAttrs ammModSysPkgs [ "default" ])
        // (if ammModSysPkgs ? default then { amm-module = ammModSysPkgs.default; } else { })
        // ammModAliasPkgs
        // (builtins.removeAttrs tokenModSysPkgs [ "default" ])
        // (if tokenModSysPkgs ? default then { token-module = tokenModSysPkgs.default; } else { })
        // tokenModAliasPkgs
      ) crateOutputs.packages;
    in
    (builtins.removeAttrs appOutputs [ "apps" "packages" ])
    // {
      apps = mergedApps;
      packages = mergedPackages;
    };
}
