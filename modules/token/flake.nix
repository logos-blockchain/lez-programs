{
  description = "Logos Token core module — Token Program reads, planning, and submission";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module dependency. The input name must match the
    # metadata.json `dependencies` entry so the builder resolves it as a module
    # dependency. Same rev the repo-root flake and the apps/token flake pin: the
    # 0.4.1-interim build (byte-string fix on a v0.2.4 wallet-ffi). See the root
    # flake.nix for the full rationale.
    lez_core.url = "github:logos-blockchain/logos-execution-zone-module?rev=acf0cd501b262c4c15969e3735e85318297b85bf";

    # The repo-root flake supplies the token_ffi crate. token_ffi is a Cargo
    # workspace member (it path-depends on token_core under programs/*), so it
    # can only be built with the whole workspace as source — which the root
    # flake does via `self`. Reference the root flake here and pull token_ffi
    # from it, so this module builds standalone (module_path=modules/token)
    # under the release CI's `nix build .#lgx-portable`.
    lez_programs.url = "path:../..";
  };

  outputs = inputs@{ logos-module-builder, lez_programs, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        token_ffi = { input = lez_programs; packages.default = "token_ffi"; };
      };
      # Mirrors the repo-root flake's tokenModuleOutputs so the module's own
      # test suite stays buildable from this flake too.
      tests = {
        dir = ./tests;
        mockCLibs = [ "token_ffi" ];
      };
    };
}
