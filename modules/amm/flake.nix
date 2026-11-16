{
  description = "Logos AMM core module — headless AMM business logic (pool resolution + swaps)";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.3";

    # Actor-aware wallet API for LEZ 0.4 (lez_core 0.6.0).
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=cc73ebfe0857705cfc1c3b29743fb6c8c6aebd82";

    # The repo-root flake supplies the amm_ffi crate. amm_ffi is a Cargo
    # workspace member (it path-depends on amm_core / token_core under programs/*), so it can only be built with the whole
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
