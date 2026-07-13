{
  description = "LEZ programs — AMM client FFI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # Honor the repo's pinned toolchain (rust-toolchain.toml -> 1.94.0).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Whole workspace: crane needs Cargo.lock + all path deps (amm_core,
        # twap_oracle_core, token_core, ...) to resolve `-p amm_client_ffi`.
        src = ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "amm_client_ffi";
          version = "0.1.0";
          # CRITICAL: scope to ONLY this crate. The workspace also contains
          # `amm/methods` etc. whose build.rs compiles the risc0 guest (which
          # WOULD invoke Metal on darwin). `-p amm_client_ffi` never builds
          # those crates or their build scripts.
          cargoExtraArgs = "-p amm_client_ffi";
          doCheck = false;
          # NOTE: cbindgen is used here as a Cargo *build-dependency*
          # (invoked from build.rs via its Rust library API), not as the
          # standalone CLI package — so no nativeBuildInputs entry is
          # needed; crane builds it as part of the normal cargo graph.
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        ammClientFfi = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            postInstall =
              ''
                mkdir -p $out/include
                cp programs/amm/client-ffi/amm_client_ffi.h $out/include/
              ''
              + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
                if [ -f $out/lib/libamm_client_ffi.dylib ]; then
                  install_name_tool -id @rpath/libamm_client_ffi.dylib $out/lib/libamm_client_ffi.dylib
                fi
              '';
          }
        );
      in
      {
        packages.default = ammClientFfi;
        packages.amm_client_ffi = ammClientFfi;
      }
    );
}
