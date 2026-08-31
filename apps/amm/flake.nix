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
    # match the metadata.json `dependencies` entry so the builder can resolve it
    # as a module dependency. Same ref the repo-root flake and the amm_module
    # flake pin (the QtRO byte-string `instruction` fix).
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr";

    # The AMM core module, resolved as the metadata.json `amm_module` dependency
    # (the builder reads its .lidl to generate modules().amm_module). Built from
    # the sibling flake; force its wallet to the SAME lez_core so the UI and the
    # core module resolve one shared wallet instance.
    amm_module = {
      url = "path:../../modules/amm";
      inputs.lez_core.follows = "lez_core";
    };
  };

  # Self-contained so the release CI can build it as its own module
  # (module_path=apps/amm, `nix build .#lgx-portable`). The UI links no external
  # lib of its own — the AMM logic lives in amm_ffi, linked by the amm_module
  # core module, which the UI reaches via modules().amm_module. Kept in sync with
  # the repo-root flake's appOutputs (preConfigure + the Basecamp-safe wallet
  # staging in postInstall).
  outputs = inputs@{ logos-module-builder, shared_wallet, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = { };
      preConfigure = ''
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
