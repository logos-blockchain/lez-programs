{
  description = "Logos Stablecoin core module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.3";
    # Same lez_core 0.4.2 rev as the repo-root flake; see there for the rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=825d2a41262b9882aa0f9ca837cb03635f7980c2";
    # LEZ v0.2.5-rc2 plus the risc0 zkr download-URL fix; see the root flake.nix.
    lez_core.inputs.logos-execution-zone.url = "github:logos-blockchain/logos-execution-zone?rev=6477ce33c311f82b962daae1583fb647b9894b39";
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
