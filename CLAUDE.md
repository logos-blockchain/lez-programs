# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

This repo contains essential programs for the **Logos Execution Zone (LEZ)** — a zkVM-based execution environment built on [RISC Zero](https://risczero.com/). Programs run inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf` target) and interact with the LEZ runtime via the `nssa_core` library from `logos-execution-zone`.

Five programs are implemented:
- **token** — Fungible and non-fungible token program (create, mint, burn, transfer, print NFTs)
- **amm** — Automated market maker (constant product AMM with add/remove liquidity and swaps)
- **ata** — Associated Token Account program (derives and initializes deterministic token accounts for a given owner and token definition)
- **stablecoin** — Collateral-backed position program (open positions, repay debt, withdraw collateral)
- **twap_oracle** — TWAP oracle (provides canonical on-chain price accounts consumed by other programs)

## Build Commands

```bash
# Check all workspace crates (skips expensive guest ZK builds)
make clippy

# Run all tests (dev mode skips ZK proof generation)
make test

# Run tests for a single package
RISC0_DEV_MODE=1 cargo test -p token_program
RISC0_DEV_MODE=1 cargo test -p amm_program
RISC0_DEV_MODE=1 cargo test -p ata_program
RISC0_DEV_MODE=1 cargo test -p stablecoin_program
RISC0_DEV_MODE=1 cargo test -p twap_oracle_program

# Format
make fmt

# Build all guest ZK binaries
make build-programs

# Build one guest directly only when debugging a single guest build
cargo risczero build --manifest-path programs/<program>/methods/guest/Cargo.toml
```

`make build-programs` uses `scripts/build-guests.sh` and
`scripts/build-guests.Dockerfile` to compile every guest crate in one Docker
BuildKit build. Cargo git, registry, and target artifacts are shared through
BuildKit cache mounts. Each raw guest ELF is packaged into the deployable RISC
Zero `.bin` format, and only those final program binaries are exported.

Built binaries output to:
`target/guest/<program>.bin`

Use `RISC0_DOCKER_CONTAINER_TAG` to override the guest builder image tag. Use
`RISC0_BUILD_CACHE_ID` to isolate BuildKit caches for clean benchmarks or
parallel experiments. Use `RISC0_DOCKER_BUILD_NETWORK` to opt in to a custom
Docker build network mode, such as `host`, when the default Docker build
network is not sufficient.

## IDL Generation

Using the `idl-gen` crate (no external toolchain required — this is what CI uses):

```bash
make idl
```

Or individually per program:

```bash
cargo run -p idl-gen -- programs/<program>/methods/guest/src/bin/<program>.rs > artifacts/<program>-idl.json
```

Alternatively, using the `spel` CLI (requires the SPEL toolchain):

```bash
spel generate-idl programs/<program>/methods/guest/src/bin/<program>.rs > artifacts/<program>-idl.json
```

Generated IDL files live in `artifacts/`. CI checks that every program under `*/methods/guest/src/bin/` has a corresponding `artifacts/<program>-idl.json` that matches the source.

## Deployment

`wallet` and `spel` are CLI tools that ship with the [SPEL](https://github.com/logos-co/spel.git) toolchain. `wallet` requires `NSSA_WALLET_HOME_DIR` to point to a directory containing the wallet config.

**Note:** `spel` and `wallet` may use different versions of the wallet package. If `spel --idl <IDL> <PROGRAM_FUNCTION> ...` fails, ensure `seq_poll_timeout_millis` is set in the wallet config at `~/.nssa/wallet`.

For testnet and mainnet deployments, build guest binaries with the release profile in each guest manifest:

```toml
[profile.release]
debug = 0
strip = "symbols"
```

That profile is part of program identity. After every release build, inspect the binary and update every value that depends on the ImageID before submitting transactions: deployed program IDs, client/config files, PDA-derived account addresses, AMM `token_program_id` and `twap_oracle_program_id`, and ATA `token_program_id` inputs. Do not mix raw or old ImageIDs with release-profile binaries.

```bash
# Build all guest binaries with the shared BuildKit cache
make build-programs

# Deploy a program binary to the sequencer
wallet deploy-program target/guest/<program>.bin

# Inspect the ProgramId of a built binary
spel inspect target/guest/<program>.bin
```

## Workspace Structure

```
Cargo.toml              # Workspace root (excludes guest crates)
programs/
  token/
    core/src/lib.rs     # Data types & Instruction enum (shared with guest)
    src/                # Program logic: burn, mint, transfer, initialize, new_definition, print_nft
    methods/            # Host-side zkVM method embedding (build.rs uses risc0_build::embed_methods)
    methods/guest/      # Guest binary (separate workspace, riscv32im target)
  amm/
    core/src/lib.rs     # Data types, Instruction enum, PDA computation helpers
    src/                # Program logic: add, remove, swap, new_definition
    methods/            # Host-side zkVM method embedding
    methods/guest/      # Guest binary (separate workspace)
  ata/
    core/src/lib.rs     # Data types & Instruction enum
    src/                # Program logic: create, burn, transfer
    methods/            # Host-side zkVM method embedding
    methods/guest/      # Guest binary (separate workspace)
  stablecoin/
    core/src/lib.rs     # Data types & Instruction enum
    src/                # Program logic: open_position, repay_debt, withdraw_collateral
    methods/            # Host-side zkVM method embedding
    methods/guest/      # Guest binary (separate workspace)
  twap_oracle/
    core/src/lib.rs     # Data types & Instruction enum
    src/                # Program logic: noop (price account initialization)
    methods/            # Host-side zkVM method embedding
    methods/guest/      # Guest binary (separate workspace)
  integration_tests/
    tests/              # End-to-end tests through the zkVM (token, amm, ata)
apps/
  amm/                  # QML-based UI for the AMM program (Nix flake)
```

## Architecture

Each program follows a layered pattern:

1. **`*_core` crate** — shared types (Instructions, account data structs) serialized with Borsh for on-chain storage, serde for instruction passing. Also contains PDA seed computation (amm_core).

2. **Program crate** — pure functions that take `AccountWithMetadata` inputs and return `Vec<AccountPostState>` (and `Vec<ChainedCall>` for AMM). No I/O or state — all state transitions are deterministic and testable without the zkVM.

3. **`methods/guest`** — the guest binary wired to the LEZ framework via `spel-framework` using the `#[lez_program]` and `#[instruction]` proc macros. This is what gets compiled to RISC-V and ZK-proven.

4. **`methods`** — host crate that embeds the guest ELF for use in tests and deployment.

## Key Patterns

**Account data serialization**: On-chain account data uses Borsh (`BorshSerialize`/`BorshDeserialize`). Instructions use serde JSON. Both implement `TryFrom<&Data>` and `From<&T> for Data` for conversion.

**Program-Derived Addresses (PDAs)**: The AMM uses SHA-256-based PDAs (`compute_pool_pda`, `compute_vault_pda`, `compute_liquidity_token_pda` in `amm_core`) to derive deterministic account addresses for pools, vaults, and liquidity tokens.

**Chained calls**: The AMM's swap and liquidity operations compose with the token program via `ChainedCall` — the AMM instructs the token program to execute transfers as part of the same atomic operation.

**Testing**: Tests call program functions directly (no zkVM overhead). Set `RISC0_DEV_MODE=1` to skip ZK proof generation when running integration tests that go through the zkVM. The Rust toolchain version is pinned in `rust-toolchain.toml`.
