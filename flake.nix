{
  description = "LEZ program client libraries";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11-small";
    crane.url = "github:ipetkov/crane/v0.23.4";
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
            pname = "wallet-idl-decoder";
            version = "0.1.0";
            strictDeps = true;
            cargoExtraArgs = "-p wallet-idl-decoder";
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          decoder = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
            postInstall = ''
              install -Dm644 ${./tools/wallet-idl-decoder/include/wallet_idl_decoder.h} \
                $out/include/wallet_idl_decoder.h
            '';
          });
        in {
          default = decoder;
          wallet_idl_decoder = decoder;
        });
    };
}
