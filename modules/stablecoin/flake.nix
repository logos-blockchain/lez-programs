{
  description = "Logos Stablecoin core module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    # Pinned to the same rev as the root and modules/amm flakes rather than to a
    # branch: this is the module's `main`, the first lez_core that speaks LEZ
    # v0.3.0, and it subsumes the `byte-string-fix` branch this used to track.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=b89e5d24df5babc2490d2e5d47756fb1f33435dd";
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
