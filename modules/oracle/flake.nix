{
  description = "Logos Oracle core module — TWAP oracle discovery and transaction transport";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.3";

    # Core wallet module dependency. The input name must match the
    # metadata.json `dependencies` entry so the builder resolves it as a module
    # dependency. Same rev the repo-root flake and the other module flakes pin:
    # lez_core 0.4.2 (v0.2.5-rc2 wallet-ffi). See the root flake.nix for the full
    # rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=825d2a41262b9882aa0f9ca837cb03635f7980c2";

    # The repo-root flake supplies the oracle_ffi crate. oracle_ffi is a Cargo
    # workspace member (it path-depends on twap_oracle_core under programs/*),
    # so it can only be built with the whole workspace as source — which the
    # root flake does via `self`. Reference the root flake here and pull
    # oracle_ffi from it, so this module builds standalone
    # (module_path=modules/oracle) under the release CI's `nix build .#lgx-portable`.
    lez_programs.url = "path:../..";
  };

  outputs = inputs@{ logos-module-builder, lez_programs, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        oracle_ffi = { input = lez_programs; packages.default = "oracle_ffi"; };
      };
      # Mirrors the repo-root flake's oracleModuleOutputs so the module's own
      # test suite stays buildable from this flake too.
      tests = {
        dir = ./tests;
        mockCLibs = [ "oracle_ffi" ];
      };
    };
}
