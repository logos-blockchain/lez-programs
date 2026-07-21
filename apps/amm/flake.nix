{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11-small";
    crane.url = "github:ipetkov/crane/v0.23.4";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. Public testnet still exposes the v0.2.0-rc6
    # wallet RPC contract.
    logos_execution_zone = {
      url = "github:logos-blockchain/logos-execution-zone-module?rev=d70225ced646934d2294fd9e8f8b03615c104b80";
      inputs.logos-execution-zone.url =
        "github:logos-blockchain/logos-execution-zone?rev=e37876a64028a335eb693198a1ed6a0e875ec5b4";
    };
  };

  outputs = inputs@{
    logos-module-builder,
    logos_execution_zone,
    nixpkgs,
    crane,
    ...
  }:
    let
      # Preserve the macOS Metal workaround without moving the wallet past the
      # RPC contract currently deployed on public testnet. Crane builds the
      # dependency artifacts separately, so both derivations need the setting.
      withXcrunNoCache = package:
        package.overrideAttrs (previous:
          {
            xcrun_nocache = "1";
          }
          // (if previous ? cargoArtifacts && previous.cargoArtifacts ? overrideAttrs
            then {
              cargoArtifacts = previous.cargoArtifacts.overrideAttrs (_: {
                xcrun_nocache = "1";
              });
            }
            else { }));

      executionZone = logos_execution_zone.inputs.logos-execution-zone;
      testnetExecutionZone = executionZone // {
        packages = builtins.mapAttrs
          (system: packages:
            if builtins.match ".*-darwin" system == null then packages
            else
              let
                wallet = withXcrunNoCache packages.wallet;
              in
              packages // {
                inherit wallet;
                default = wallet;
              })
          executionZone.packages;
      };

      # Evaluate the pinned module source with the testnet-compatible wallet.
      # This is ordinary flake composition, not a relative flake input, keeping
      # Nix 2.25 lock and build behavior portable.
      logosExecutionZoneModule =
        (import "${logos_execution_zone.outPath}/flake.nix").outputs {
          logos-module-builder = logos_execution_zone.inputs.logos-module-builder;
          logos-execution-zone = testnetExecutionZone;
          nix-bundle-lgx = logos_execution_zone.inputs.nix-bundle-lgx;
        };

      # Name must match metadata.json so the builder resolves the core module.
      moduleInputs = inputs // {
        logos_execution_zone = logosExecutionZoneModule;
      };

      clientLibrariesInput = (import ../../flake.nix).outputs {
        inherit nixpkgs crane;
      };
      moduleBuild = logos-module-builder.lib.mkLogosQmlModule {
        src = ./.;
        configFile = ./metadata.json;
        flakeInputs = moduleInputs;
        preConfigure = ''
          cmakeFlagsArray+=("-DLOGOS_WALLET_SOURCE_DIR=${../shared/wallet}")
        '';
        externalLibInputs = {
          amm_client = clientLibrariesInput;
          wallet_idl_decoder = {
            input = clientLibrariesInput;
            packages.default = "wallet_idl_decoder";
          };
        };
        postInstall = ''
          # Basecamp sandboxes a ui_qml view and deliberately blocks native QML
          # plugins. The wallet controls are QML-only, so package their source
          # files with a plain qmldir rather than the generated descriptor that
          # points at logos_wallet_qmlplugin.
          test -f ${./config/LogosWallet.qmldir}

          walletQmlDir="shared-wallet/qml/Logos/Wallet"
          if [ ! -d "$walletQmlDir" ]; then
            echo "Built Logos.Wallet QML module not found"
            exit 1
          fi
          walletQmlInstallDir="$out/lib/Logos/Wallet"
          mkdir -p "$walletQmlInstallDir"
          cp "$walletQmlDir"/*.qml "$walletQmlInstallDir/"
          cp -r "$walletQmlDir/icons" "$walletQmlInstallDir/"
          install -m 0644 ${./config/LogosWallet.qmldir} "$walletQmlInstallDir/qmldir"
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
                system moduleInputs moduleBuild.config.dependencies;
          };
        }
      );

      publicPackages = builtins.mapAttrs
        (system: packages: packages // {
          logos_execution_zone-lgx =
            logosExecutionZoneModule.packages.${system}.lgx;
          logos_execution_zone-lgx-portable =
            logosExecutionZoneModule.packages.${system}.lgx-portable;
        })
        moduleBuild.packages;

      linuxChecks =
        let
          pkgs = nixpkgs.legacyPackages.x86_64-linux;
          walletModule = logosExecutionZoneModule.packages.x86_64-linux.lib;
        in
        {
          wallet-rpc-contract = pkgs.runCommand "amm-wallet-rpc-contract" { } ''
            wallet="${walletModule}/lib/libwallet_ffi.so"
            test -f "$wallet"
            ${pkgs.binutils}/bin/strings "$wallet" > methods
            ${pkgs.gnugrep}/bin/grep -Fq sendTransaction methods
            ${pkgs.gnugrep}/bin/grep -Fq getProofForCommitment methods
            if ${pkgs.gnugrep}/bin/grep -Fq getProofsAndRoot methods; then
              echo "wallet uses unsupported testnet RPC method getProofsAndRoot"
              exit 1
            fi
            touch "$out"
          '';
        };
    in
    moduleBuild // {
      apps = publicApps;
      packages = publicPackages;
      checks = (moduleBuild.checks or { }) // {
        x86_64-linux = (moduleBuild.checks.x86_64-linux or { }) // linuxChecks;
      };
    };
}
