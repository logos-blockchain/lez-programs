{
  description = "Logos AMM QML UI — trade and provide liquidity on the LEZ AMM";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder";

    # Core wallet module (the LEZ wallet FFI Qt plugin). The input name must
    # match the metadata.json `dependencies` entry so the builder can resolve
    # it as a module dependency. This rev pins LEZ (lssa) at fb8cbac4, which
    # includes the macOS Metal-build fix, so no `--override-input` is needed.
    logos_execution_zone.url = "github:logos-blockchain/logos-execution-zone-module?rev=d2e9400ac06c3cdbfc2405b4f153fff9841a453c";
  };

  # NOTE: this flake is no longer built standalone. The amm_client_ffi crate
  # (the Rust C FFI library the AmmUiBackend C++ code links against) lives in
  # the repo-root flake, and referencing it from here would require either a
  # hardcoded absolute `git+file://` path or a `path:../..` input — the latter
  # fails flake evaluation because the app directory is copied into the Nix
  # store as its own flake root, so `../..` can't escape it there. Instead,
  # the repo-root flake.nix builds this module directly (src = ./apps/amm)
  # and resolves amm_client_ffi via `self`. Build/run the app from the repo
  # root, e.g. `nix build .#packages.<system>.default` or `nix run .`.
  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
