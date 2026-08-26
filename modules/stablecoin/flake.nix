{
  description = "Logos Stablecoin core module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr";
  };

  # stablecoin_ffi lives in the repository root flake. Build this module from
  # the repository root with `nix build .#stablecoin-module`.
  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
