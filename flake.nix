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
            version = "0.1.0";
            strictDeps = true;
          };
          ammClientArgs = commonArgs // {
            pname = "amm_client";
            cargoExtraArgs = "-p amm_client";
          };
          ammClientArtifacts = craneLib.buildDepsOnly ammClientArgs;
          ammClient = craneLib.buildPackage (ammClientArgs // {
            cargoArtifacts = ammClientArtifacts;
            doCheck = false;
            postInstall = ''
              install -Dm644 ${./apps/amm/client/include/amm_client.h} \
                $out/include/amm_client.h
            '';
          });
          walletDecoderArgs = commonArgs // {
            pname = "wallet-idl-decoder";
            cargoExtraArgs = "-p wallet-idl-decoder";
          };
          walletDecoderArtifacts = craneLib.buildDepsOnly walletDecoderArgs;
          walletDecoder = craneLib.buildPackage (walletDecoderArgs // {
            cargoArtifacts = walletDecoderArtifacts;
            doCheck = false;
            postInstall = ''
              install -Dm644 ${./tools/wallet-idl-decoder/include/wallet_idl_decoder.h} \
                $out/include/wallet_idl_decoder.h
            '';
          });
        in {
          default = ammClient;
          amm_client = ammClient;
          wallet_idl_decoder = walletDecoder;
        });
    };
}
