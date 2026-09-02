{
  description = "Logos AMM core module — headless AMM business logic (pool resolution + swaps)";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module dependency. The input name must match the
    # metadata.json `dependencies` entry so the builder resolves it as a module
    # dependency. Same upstream branch the repo-root flake pins (the QtRO byte-string
    # `instruction` fix for send_generic_public_transaction).
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?ref=byte-string-fix";

    # The repo-root flake supplies the amm_ffi crate. amm_ffi is a Cargo
    # workspace member (it path-depends on amm_core / token_core /
    # twap_oracle_core under programs/*), so it can only be built with the whole
    # workspace as source — which the root flake does via `self`. Reference the
    # root flake here and pull amm_ffi from it, so this module builds standalone
    # (module_path=modules/amm) under the release CI's `nix build .#lgx-portable`.
    lez_programs.url = "path:../..";
  };

  outputs = inputs@{ logos-module-builder, lez_programs, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        amm_ffi = { input = lez_programs; packages.default = "amm_ffi"; };
      };
    };
}
