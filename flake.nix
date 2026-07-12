{
  description = "LEZ program client libraries";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { nixpkgs, crane, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          craneLib = crane.mkLib pkgs;
          src = craneLib.cleanCargoSource ./.;
          commonArgs = {
            inherit src;
            pname = "amm_client";
            version = "0.1.0";
            strictDeps = true;
            cargoExtraArgs = "-p amm_client";
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          ammClient = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            postInstall = ''
              install -Dm644 ${./apps/amm/client/include/amm_client.h} \
                $out/include/amm_client.h
            '';
          });
        in {
          default = ammClient;
          amm_client = ammClient;
        });
    };
}
