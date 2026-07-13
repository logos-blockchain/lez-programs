{
  description = "LEZ programs — AMM client FFI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # The AMM QML UI module (apps/amm) is built from this same flake so it can
    # reference the amm_client_ffi crate package via `self` — no filesystem
    # path or git-remote reference to this repo is needed (see apps/amm/flake.nix
    # history: a `git+file://` URL pointing at a local checkout is
    # machine-specific and not portable).
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This rev pins LEZ (lssa) at fb8cbac4, which
    # includes the macOS Metal-build fix, so no `--override-input` is needed.
    logos_execution_zone.url = "github:logos-blockchain/logos-execution-zone-module?rev=d2e9400ac06c3cdbfc2405b4f153fff9841a453c";
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

        # Whole workspace: crane needs Cargo.lock + all path deps (amm_core,
        # twap_oracle_core, token_core, ...) to resolve `-p amm_client_ffi`.
        src = ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "amm_client_ffi";
          version = "0.1.0";
          # CRITICAL: scope to ONLY this crate. The workspace also contains
          # `amm/methods` etc. whose build.rs compiles the risc0 guest (which
          # WOULD invoke Metal on darwin). `-p amm_client_ffi` never builds
          # those crates or their build scripts.
          cargoExtraArgs = "-p amm_client_ffi";
          doCheck = false;
          # NOTE: cbindgen is used here as a Cargo *build-dependency*
          # (invoked from build.rs via its Rust library API), not as the
          # standalone CLI package — so no nativeBuildInputs entry is
          # needed; crane builds it as part of the normal cargo graph.
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        ammClientFfi = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            postInstall =
              ''
                mkdir -p $out/include
                cp programs/amm/client-ffi/amm_client_ffi.h $out/include/
              ''
              + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
                if [ -f $out/lib/libamm_client_ffi.dylib ]; then
                  install_name_tool -id @rpath/libamm_client_ffi.dylib $out/lib/libamm_client_ffi.dylib
                fi
              '';
          }
        );
      in
      {
        packages.default = ammClientFfi;
        packages.amm_client_ffi = ammClientFfi;
      }
    );

      # The AMM QML UI module (apps/amm). Its external_libraries entry
      # (amm_client_ffi) is resolved to `self.packages.${system}.amm_client_ffi`
      # above — the module builder's resolveExtInput reads
      # `flakeInput.packages.${system}.${pkgName}`, and passing `self` here
      # works because `self` is this very flake, which already exposes that
      # package per crateOutputs above.
      appOutputs = logos-module-builder.lib.mkLogosQmlModule {
        src = ./apps/amm;
        configFile = ./apps/amm/metadata.json;
        flakeInputs = inputs;
        externalLibInputs = {
          amm_client_ffi = { input = self; packages.default = "amm_client_ffi"; };
        };
      };

      # Expose the AMM QML UI as a NAMED app/package (`amm-ui`) rather than
      # `<sys>.default` — the repo is expected to grow more UIs over time,
      # each runnable via `nix run .#<name>`. `appOutputs.apps.<sys>.default`
      # and `appOutputs.packages.<sys>.default` (the UI launcher and its
      # bundle, respectively) are renamed to `amm-ui`; no `default` survives
      # for either attribute set.
      appApps = appOutputs.apps or { };
      appPkgs = appOutputs.packages or { };

      renamedApps = builtins.mapAttrs (
        system: attrs:
        (builtins.removeAttrs attrs [ "default" ]) // (if attrs ? default then { amm-ui = attrs.default; } else { })
      ) appApps;

      mergedPackages = builtins.mapAttrs (
        system: cratePkgs:
        let
          appSysPkgs = appPkgs.${system} or { };
        in
        (builtins.removeAttrs cratePkgs [ "default" ])
        // (builtins.removeAttrs appSysPkgs [ "default" ])
        // (if appSysPkgs ? default then { amm-ui = appSysPkgs.default; } else { })
      ) crateOutputs.packages;
    in
    (builtins.removeAttrs appOutputs [ "apps" "packages" ])
    // {
      apps = renamedApps;
      packages = mergedPackages;
    };
}
