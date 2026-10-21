{
  description = "Logos Stablecoin core module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    # Same lez_core 0.4.3 rev as the repo-root flake; see there for the rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=673d1153563d730768d546f4d4635629c4dec1bd";
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
