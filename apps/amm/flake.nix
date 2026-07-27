{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Shared C++ wallet access and Logos.Wallet QML sources.
    shared_wallet = {
      url = "path:../shared/wallet";
      flake = false;
    };

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This revision exposes generic transaction
    # submission by deployed program ID.
    logos_execution_zone = {
      url = "github:logos-blockchain/logos-execution-zone-module?rev=d70225ced646934d2294fd9e8f8b03615c104b80";

      # The module pins the monorepo at v0.2.0-rc6 (e37876a), which owns the
      # xcrun wrapper in its flake.nix. Override that transitive input to the
      # head of PR #629 (fix(macos): nuke xcrun cache) so the Metal/xcrun build
      # works on macOS. Note: #629 branches off `dev`, so this also pulls dev's
      # drift from rc6 — if the wallet_ffi ABI mismatches the module build, fall
      # back to cherry-picking #629's one line onto e37876a and pin that commit.
      inputs.logos-execution-zone.url =
        "github:logos-blockchain/logos-execution-zone?rev=a7e06a660940a00093b1760560d37ff84aff5a05";
    };

    amm_client.url = "path:../..";
  };

  # NOTE: this flake is no longer built standalone. The amm_client crate
  # (the Rust C FFI library the AmmUiBackend C++ code links against) lives in
  # the repo-root flake, and referencing it from here would require either a
  # hardcoded absolute `git+file://` path or a `path:../..` input — the latter
  # fails flake evaluation because the app directory is copied into the Nix
  # store as its own flake root, so `../..` can't escape it there. Instead,
  # the repo-root flake.nix builds this module directly (src = ./apps/amm)
  # and resolves amm_client via `self`. The repo-root flake exposes the UI
  # as a named attribute (there is no bare `default`): run it from the repo root
  # with `nix run .#amm-ui`, and build just the FFI crate with
  # `nix build .#amm_client`.
  outputs = inputs@{ logos-module-builder, shared_wallet, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      preConfigure = ''
        cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${shared_wallet}")
      '';
      externalLibInputs = {
        amm_client = {
          input = inputs.amm_client;
          packages.default = "amm_client";
        };
      };
      postInstall = ''
        # The builder installs the view under lib/qml after this hook. Its
        # import descriptor points back to this compiled shared QML module.
        test -f ${./qml}/Logos/Wallet/qmldir

        walletQmlDir="shared-wallet/qml/Logos/Wallet"
        if [ ! -d "$walletQmlDir" ]; then
          echo "Built Logos.Wallet QML module not found"
          exit 1
        fi
        walletQmlInstallDir="$out/lib/Logos/Wallet"
        mkdir -p "$walletQmlInstallDir"
        cp -r "$walletQmlDir/." "$walletQmlInstallDir/"
        test -f "$walletQmlInstallDir/qmldir"
      '';
    };
}
