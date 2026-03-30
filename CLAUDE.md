# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

This repo contains essential programs for the **Logos Execution Zone (LEZ)** — a zkVM-based execution environment built on [RISC Zero](https://risczero.com/). Programs run inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf` target) and interact with the LEZ runtime via the `nssa_core` library from `logos-execution-zone`.

Two programs are implemented:
- **token** — Fungible and non-fungible token program (create, mint, burn, transfer, print NFTs)
- **amm** — Automated market maker (constant product AMM with add/remove liquidity and swaps)

## Build Commands

```bash
# Check all workspace crates (skips expensive guest ZK builds)
RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

# Run all tests (dev mode skips ZK proof generation)
RISC0_DEV_MODE=1 cargo test --workspace

# Run tests for a single package
RISC0_DEV_MODE=1 cargo test -p token_program
RISC0_DEV_MODE=1 cargo test -p amm_program

# Format
cargo fmt --all

# Build the guest ZK binary (requires risc0 toolchain)
cargo risczero build --manifest-path token/methods/guest/Cargo.toml
cargo risczero build --manifest-path amm/methods/guest/Cargo.toml
```

Built binaries output to: `<program>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<program>.bin`

## IDL Generation

```bash
spel generate-idl token/methods/guest/src/bin/token.rs > token/token-idl.json
spel generate-idl amm/methods/guest/src/bin/amm.rs > amm/amm-idl.json
```

## Deployment

`wallet` and `spel` are CLI tools that ship with the [SPEL](https://github.com/logos-co/spel.git) toolchain. `wallet` requires `NSSA_WALLET_HOME_DIR` to point to a directory containing the wallet config.

**Note:** `spel` and `wallet` may use different versions of the wallet package. If `spel --idl <IDL> <PROGRAM_FUNCTION> ...` fails, ensure `seq_poll_timeout_millis` is set in the wallet config at `~/.nssa/wallet`.

```bash
# Deploy a program binary to the sequencer
wallet deploy-program <path-to-binary>

# Inspect the ProgramId of a built binary
spel inspect <path-to-binary>
```

## Workspace Structure

```
Cargo.toml              # Workspace root (excludes guest crates)
token/
  core/src/lib.rs       # Data types & Instruction enum (shared with guest)
  src/                  # Program logic: burn, mint, transfer, initialize, new_definition, print_nft
  methods/              # Host-side zkVM method embedding (build.rs uses risc0_build::embed_methods)
  methods/guest/        # Guest binary (separate workspace, riscv32im target)
amm/
  core/src/lib.rs       # Data types, Instruction enum, PDA computation helpers
  src/                  # Program logic: add, remove, swap, new_definition
  methods/              # Host-side zkVM method embedding
  methods/guest/        # Guest binary (separate workspace)
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

**Testing**: Tests call program functions directly (no zkVM overhead). Set `RISC0_DEV_MODE=1` to skip ZK proof generation when running integration tests that go through the zkVM. The Rust toolchain pinned version is **1.91.1**.
