{
  description = "Logos Token QML UI — create and inspect Token Program assets";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Shared C++ wallet access and Logos.Wallet QML sources.
    shared_wallet = {
      url = "path:../shared/wallet";
      flake = false;
    };

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve it
    # as a module dependency. Same rev the repo-root flake and the token_module
    # flake pin: the 0.4.1-interim build (byte-string fix on a v0.2.4 wallet-ffi).
    # See the root flake.nix for the full rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf";

    # The token core module, resolved as the metadata.json `token_module`
    # dependency (the builder reads its .lidl to generate modules().token_module).
    # Built from the sibling flake; force its wallet to the SAME lez_core so the
    # UI and the core module resolve one shared wallet instance.
    token_module = {
      url = "path:../../modules/token";
      inputs.lez_core.follows = "lez_core";
    };
  };

  # Self-contained so the release CI can build it as its own module
  # (module_path=apps/token, `nix build .#lgx-portable`). The UI links no
  # external lib of its own — the Token Program logic lives in token_ffi, linked
  # by the token_module core module, which the UI reaches via
  # modules().token_module. Kept in sync with the repo-root flake's
  # tokenAppOutputs (preConfigure + the Basecamp-safe wallet staging in
  # postInstall).
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
