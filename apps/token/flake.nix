{
  description = "Logos Token QML UI — create and manage tokens on the LEZ token program";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This rev pins LEZ (lssa) at fb8cbac4, which
    # includes the macOS Metal-build fix, so no `--override-input` is needed.
    logos_execution_zone.url = "github:logos-blockchain/logos-execution-zone-module?rev=d2e9400ac06c3cdbfc2405b4f153fff9841a453c";

    # Shared wallet UI, consumed as a plain source tree — not a flake:
    #   - src/{AccountModel,WalletBackendLogic} — C++ overlaid onto this app's
    #     source below (the backend is compiled into this app's plugin).
    #   - qml/Logos/Wallet — the importable `Logos.Wallet` QML module, installed
    #     into the app output at lib/Logos/Wallet by postInstall (the standalone
    #     puts the app's plugin dir on the QML import path, so `import
    #     Logos.Wallet` resolves at runtime).
    wallet_ui = {
      url = "path:../common/wallet-ui";
      flake = false;
    };

    # Build the merge with the exact nixpkgs the module builder uses, so there is
    # no second nixpkgs to download and no version skew.
    nixpkgs.follows = "logos-module-builder/nixpkgs";
  };

  outputs = inputs@{ self, logos-module-builder, wallet_ui, nixpkgs, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;

      # Overlay the shared wallet UI onto this app's own source. Built per system
      # so cross-platform builds stay pure (the module builder takes a single
      # `src`, so we produce a system-matched merged tree for each).
      mergedSrcFor = system:
        let pkgs = import nixpkgs { inherit system; };
        in pkgs.runCommand "token-ui-src" { } ''
          cp -r ${self}/. $out
          chmod -R u+w $out
          mkdir -p $out/src $out/qml
          # Shared C++ (account model + CRTP wallet backend logic).
          cp ${wallet_ui}/src/AccountModel.h      $out/src/AccountModel.h
          cp ${wallet_ui}/src/AccountModel.cpp    $out/src/AccountModel.cpp
          cp ${wallet_ui}/src/WalletBackendLogic.h $out/src/WalletBackendLogic.h
          # Shared Logos.Wallet QML module, placed under the view dir so it ships
          # at <pluginDir>/qml/Logos/Wallet and NavBar can `import "Logos/Wallet"`.
          # (The ui-host only searches the runtime's own QML import path, not the
          # app dir, so a bare `import Logos.Wallet` URI would not resolve; a
          # relative import against the view dir does. See the shared README.)
          rm -rf $out/qml/Logos
          cp -r ${wallet_ui}/qml/Logos $out/qml/Logos
        '';

      # Call the builder once per system with that system's merged src, then keep
      # only that system's outputs. (mkLogosQmlModule iterates all systems
      # internally; feeding each call the matching source keeps the diagonal
      # correct and everything off it lazily unevaluated.)
      moduleFor = system: logos-module-builder.lib.mkLogosQmlModule {
        src = mergedSrcFor system;
        configFile = ./metadata.json;
        flakeInputs = inputs;
      };
    in {
      packages  = forAllSystems (system: (moduleFor system).packages.${system});
      apps      = forAllSystems (system: (moduleFor system).apps.${system});
      devShells = forAllSystems (system: (moduleFor system).devShells.${system});
    };
}
