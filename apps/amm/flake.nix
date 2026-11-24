{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.3";

    # Shared C++ wallet access and Logos.Wallet QML sources.
    shared_wallet = {
      url = "path:../shared/wallet";
      flake = false;
    };

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve it
    # as a module dependency. Same rev the repo-root flake and the amm_module
    # flake pin: the module `main` build against a v0.2.5-rc2 wallet-ffi.
    # See the root flake.nix for why not the newer adapt-ffi-program-kind branch.
    # See the root flake.nix for the full rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=825d2a41262b9882aa0f9ca837cb03635f7980c2";
    # LEZ v0.2.5-rc2 plus the risc0 zkr download-URL fix; see the root flake.nix.
    lez_core.inputs.logos-execution-zone.url = "github:logos-blockchain/logos-execution-zone?rev=6477ce33c311f82b962daae1583fb647b9894b39";

    # The AMM core module, resolved as the metadata.json `amm_module` dependency
    # (the builder reads its .lidl to generate modules().amm_module). Built from
    # the sibling flake; force its wallet to the SAME lez_core so the UI and the
    # core module resolve one shared wallet instance.
    amm_module = {
      url = "path:../../modules/amm";
      inputs.lez_core.follows = "lez_core";
    };
    lez_programs.follows = "amm_module/lez_programs";
  };

  # The standalone release build links the shared wallet decoder. The amm_module
  # dependency supplies the on-chain program operations.
  outputs = inputs@{ logos-module-builder, shared_wallet, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        wallet_idl_decoder = { input = inputs.lez_programs; packages.default = "wallet_idl_decoder"; };
      };
      preConfigure = ''
        cmakeFlagsArray+=("-DLEZ_IDL_ARTIFACTS_DIR=${inputs.lez_programs}/artifacts")
        cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${shared_wallet}")
        cmakeFlagsArray+=("-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")
      '';
      # Stage Logos.Wallet at the plugin ROOT as PURE QML (strip every
      # plugin/resource directive) so Basecamp's QML sandbox accepts it — it
      # rejects `prefer :/qt/qml/...` with "Invalid null URL", and keeping the
      # module at the root (never under qml/) avoids leaking qml/NavBar.qml et al.
      # into Basecamp's shared import path where it would collide with other
      # plugins' identically-named types. Keep in sync with the repo-root flake.
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
        grep -vE '^(linktarget|optional plugin|plugin|classname|typeinfo|prefer)([[:space:]]|$)' \
          "$walletQmlInstallDir/qmldir" > "$walletQmlInstallDir/qmldir.pureqml"
        mv "$walletQmlInstallDir/qmldir.pureqml" "$walletQmlInstallDir/qmldir"
        test -f "$walletQmlInstallDir/qmldir"
      '';
    };
}
