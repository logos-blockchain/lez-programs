{
  description = "LEZ programs — host client modules and FFIs";

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
    # Fork of logos-blockchain/logos-execution-zone-module @ d70225ced with the
    # QtRO serialization fix: send_generic_public_transaction's `instruction` uses
    # a byte-string IPC type so the args survive the cross-process boundary (at
    # d70225ced the API already takes program_id_hex instead of program_elf/deps).
    # See docs/amm-swap-qtro-serialization-bug.md.
    logos_execution_zone = {
      url = "github:gravityblast/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr";

      # Override the module's pinned LEZ monorepo (logos-execution-zone) to the
      # SAME rev the target sequencer runs (415964d7): the wallet client and the
      # sequencer must agree on the JSON-RPC API, or tx submission fails at
      # runtime with `MethodNotFound`. 415964d7's wallet_ffi takes a program id
      # for send_generic_public_transaction, matching the d70225ced module.
      inputs.logos-execution-zone.url =
        "github:logos-blockchain/logos-execution-zone?rev=415964d7f9043a1bfe28da8d0e8b3a6f64abb258";
    };

  };

  outputs =
    inputs@{ self, nixpkgs, flake-utils, crane, rust-overlay, logos-module-builder, logos_execution_zone, ... }:
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
      # modules().amm_module in the backend) alongside the logos_execution_zone
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
          walletQmlInstallDir="$out/lib/Logos/Wallet"
          mkdir -p "$walletQmlInstallDir"
          cp -r "$walletQmlDir/." "$walletQmlInstallDir/"
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
          walletQmlInstallDir="$out/lib/Logos/Wallet"
          mkdir -p "$walletQmlInstallDir"
          cp -r "$walletQmlDir/." "$walletQmlInstallDir/"
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

      # AMM core module (modules/amm): the AMM business logic as a headless
      # `core` Logos module. It links the amm_ffi crate (the transport-
      # independent AMM brain, resolved via `self`) and depends on the
      # logos_execution_zone wallet module (declared in modules/amm/metadata.json,
      # reached via modules().logos_execution_zone in the impl). Exposed as the
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

      # Wrap the app launcher to export DYLD_FALLBACK_LIBRARY_PATH pointing at the
      # amm_ffi lib. The logos module builder links the plugin against
      # @rpath/libamm_ffi.dylib but does NOT stage that dylib into the
      # plugin-dir it loads at runtime, so dlopen fails with "Failed to load UI
      # plugin". Adding the crate's store lib dir to DYLD's fallback search path
      # lets the loader find it (the store path stays in the closure).
      wrapWithDyld = system: app:
        let
          pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
          ammFfi = crateOutputs.packages.${system}.amm_ffi;
        in
        app // {
          program = "${pkgs.writeShellScript "run-amm-ui" ''
            export DYLD_FALLBACK_LIBRARY_PATH="${ammFfi}/lib''${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
            exec ${app.program} "$@"
          ''}";
        };

      renamedApps = builtins.mapAttrs (
        system: attrs:
        (builtins.removeAttrs attrs [ "default" ]) // (if attrs ? default then { amm-ui = wrapWithDyld system attrs.default; } else { })
      ) appApps;

      renamedTokenApps = builtins.mapAttrs (
        system: attrs:
        (builtins.removeAttrs attrs [ "default" ]) // (if attrs ? default then { token-ui = attrs.default; } else { })
      ) tokenAppApps;

      mergedApps = builtins.mapAttrs (
        system: attrs:
        attrs // (renamedTokenApps.${system} or { })
      ) renamedApps;

      mergedPackages = builtins.mapAttrs (
        system: cratePkgs:
        let
          appSysPkgs = appPkgs.${system} or { };
          tokenAppSysPkgs = tokenAppPkgs.${system} or { };
          ammModSysPkgs = ammModulePkgs.${system} or { };
          tokenModSysPkgs = tokenModulePkgs.${system} or { };
        in
        (builtins.removeAttrs cratePkgs [ "default" ])
        // (builtins.removeAttrs appSysPkgs [ "default" ])
        // (if appSysPkgs ? default then { amm-ui = appSysPkgs.default; } else { })
        // (builtins.removeAttrs tokenAppSysPkgs [ "default" ])
        // (if tokenAppSysPkgs ? default then { token-ui = tokenAppSysPkgs.default; } else { })
        // (builtins.removeAttrs ammModSysPkgs [ "default" ])
        // (if ammModSysPkgs ? default then { amm-module = ammModSysPkgs.default; } else { })
        // (builtins.removeAttrs tokenModSysPkgs [ "default" ])
        // (if tokenModSysPkgs ? default then { token-module = tokenModSysPkgs.default; } else { })
      ) crateOutputs.packages;
    in
    (builtins.removeAttrs appOutputs [ "apps" "packages" ])
    // {
      apps = mergedApps;
      packages = mergedPackages;
    };
}
