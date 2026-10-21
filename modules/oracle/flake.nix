{
  description = "Logos Oracle core module";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=825d2a41262b9882aa0f9ca837cb03635f7980c2";
  };

  # oracle_ffi lives in the repository root flake. Build this module from
  # the repository root with `nix build .#oracle-module`.
  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
