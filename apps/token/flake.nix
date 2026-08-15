{
  description = "Logos Token QML UI — create and inspect Token Program assets";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    shared_wallet = {
      url = "path:../shared/wallet";
      flake = false;
    };
  };

  # The repository root is the supported build for this UI because it injects
  # the in-tree token_module core module. Keep this file useful for local QML
  # iteration and consistent with the AMM UI's standalone source layout.
  outputs = inputs@{ logos-module-builder, shared_wallet, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      preConfigure = ''
        cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${shared_wallet}")
        cmakeFlagsArray+=("-DLOGOS_WALLET_GENERATED_DIR=$PWD/generated_code/include")
      '';
      externalLibInputs = { };
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
        # Basecamp ui_qml views run in a sandboxed QML host and cannot load
        # native plugins from an imported QML module. Token already exposes
        # wallet operations through TokenUiBackend, so the shared wallet
        # controls only need their pure-QML files here.
        sed -i -E '/^(linktarget|optional plugin|classname|typeinfo|prefer)/d' \
          "$walletQmlInstallDir/qmldir"
        find "$walletQmlInstallDir" -maxdepth 1 -type f \
          \( -name 'liblogos_wallet_qml*' -o -name 'plugins.qmltypes' \
             -o -name '*module_dir_map.qrc' \) -delete
        test -f "$walletQmlInstallDir/qmldir"
        grep -q '^module Logos.Wallet$' "$walletQmlInstallDir/qmldir"
        grep -q '^WalletControl 1.0 WalletControl.qml$' "$walletQmlInstallDir/qmldir"
      '';
    };
}
