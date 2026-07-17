{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11-small";
    crane.url = "github:ipetkov/crane/v0.23.4";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This revision exposes generic transaction
    # submission by deployed program ID.
    logos_execution_zone = {
      url = "github:logos-blockchain/logos-execution-zone-module?rev=d70225ced646934d2294fd9e8f8b03615c104b80";

      # The module pins the monorepo at v0.2.0-rc6 (e37876a). Override that
      # transitive input with the merged macOS xcrun-cache fix.
      inputs.logos-execution-zone.url =
        "github:logos-blockchain/logos-execution-zone?rev=a7e06a660940a00093b1760560d37ff84aff5a05";
    };
  };

  outputs = inputs@{ logos-module-builder, nixpkgs, crane, ... }:
    let
      ammClientInput = (import ../../flake.nix).outputs {
        inherit nixpkgs crane;
      };
      moduleBuild = logos-module-builder.lib.mkLogosQmlModule {
        src = ./.;
        configFile = ./metadata.json;
        flakeInputs = inputs;
        preConfigure = ''
          cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${../shared/wallet}")
        '';
        externalLibInputs = {
          amm_client = ammClientInput;
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

      publicApps = logos-module-builder.lib.common.forAllSystems nixpkgs ({ system, pkgs }:
        {
          default = logos-module-builder.lib.mkStandaloneApp {
            inherit pkgs;
            standalone =
              logos-module-builder.inputs.logos-standalone-app.packages.${system}.default;
            plugin = moduleBuild.packages.${system}.default;
            metadataFile = ./metadata.json;

            # The qt-plugin layout only copies the plugin and replica factory.
            # Keep the complete lib tree so sibling external libraries such as
            # libamm_client remain available through the plugin's loader path.
            format = "qml";

            moduleDeps =
              logos-module-builder.lib.common.collectAllModuleDeps
                system inputs moduleBuild.config.dependencies;
          };
        }
      );
    in
    moduleBuild // {
      apps = publicApps;
    };
}
