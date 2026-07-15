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
    logos_execution_zone.url = "github:logos-blockchain/logos-execution-zone-module?rev=d70225ced646934d2294fd9e8f8b03615c104b80";
  };

  outputs = inputs@{ logos-module-builder, shared_wallet, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      preConfigure = ''
        cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${shared_wallet}")
      '';
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
