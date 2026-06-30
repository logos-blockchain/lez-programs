{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    nixpkgs.follows = "logos-module-builder/nixpkgs";
    logos-standalone-app.follows = "logos-module-builder/logos-standalone-app";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This rev pins LEZ (lssa) at d2e9400, which
    # includes the macOS Metal-build fix, so no `--override-input` is needed.
    logos_execution_zone.url = "github:logos-blockchain/logos-execution-zone-module?rev=d2e9400ac06c3cdbfc2405b4f153fff9841a453c";
  };

  outputs = inputs@{ logos-module-builder, logos-standalone-app, nixpkgs, ... }:
  let
    systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
    forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));

    fixedStandalonePackages = forAllSystems (pkgs:
      let
        base = logos-standalone-app.packages.${pkgs.system};
        qtLibPath = pkgs.lib.makeLibraryPath ([
          pkgs.qt6.qtbase
          pkgs.qt6.qtremoteobjects
          pkgs.qt6.qtdeclarative
          pkgs.qt6.qtwebview
          pkgs.zstd
          pkgs.krb5
          pkgs.zlib
          pkgs.glib
          pkgs.stdenv.cc.cc
          pkgs.freetype
          pkgs.fontconfig
          pkgs.boost
          pkgs.openssl
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.libglvnd
          pkgs.mesa
          pkgs.xorg.libX11
          pkgs.xorg.libXext
          pkgs.xorg.libXrender
          pkgs.xorg.libXrandr
          pkgs.xorg.libXcursor
          pkgs.xorg.libXi
          pkgs.xorg.libXfixes
          pkgs.xorg.libxcb
        ]);
        fixedApp = base.default.overrideAttrs (old: {
          inherit qtLibPath;
          qtWrapperArgs = (old.qtWrapperArgs or [ ]) ++ [
            "--prefix" "LD_LIBRARY_PATH" ":" qtLibPath
            "--prefix" "QT_PLUGIN_PATH" ":" old.qtPluginPath
            "--prefix" "QML_IMPORT_PATH" ":" old.qmlImportPath
            "--prefix" "QML2_IMPORT_PATH" ":" old.qmlImportPath
          ];
        });
      in base // {
        app = fixedApp;
        default = fixedApp;
      });

    fixedStandalone = logos-standalone-app // {
      packages = fixedStandalonePackages;
    };
  in
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      logosStandalone = fixedStandalone;
      preConfigure = ''
        export AMM_UI_PROGRAM_DIR=${inputs.logos_execution_zone.inputs."logos-execution-zone"}/artifacts/program_methods
      '';
      postInstall = ''
        mkdir -p "$out/lib/config"
        cp -r ${./config}/. "$out/lib/config/"
      '';
    };
}
