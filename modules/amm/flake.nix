{
  description = "Logos AMM core module — headless AMM business logic (pool resolution + swaps)";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module dependency. The input name must match the
    # metadata.json `dependencies` entry so the builder resolves it as a module
    # dependency. Same fork the repo-root flake pins (the QtRO byte-string
    # `instruction` fix for send_generic_public_transaction).
    logos_execution_zone.url = "github:gravityblast/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr";
  };

  # NOTE: like apps/amm, this flake is NOT built standalone. The amm_client
  # crate this module links (the Rust JSON-FFI brain) lives in the repo-root
  # flake, and referencing it from here would require a hardcoded `git+file://`
  # path or a `path:../..` input — the latter fails flake evaluation because this
  # dir is copied into the Nix store as its own flake root, so `../..` can't
  # escape it. Instead, the repo-root flake.nix builds this module directly
  # (src = ./modules/amm) and resolves amm_client via `self`. Build it from
  # the repo root:
  #   nix build .#amm-module
  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
