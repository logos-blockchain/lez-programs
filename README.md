# lez-programs

Essential programs for the **Logos Execution Zone (LEZ)** — a zkVM-based execution environment built on [RISC Zero](https://risczero.com/). Programs run inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf` target) and interact with the LEZ runtime via the `nssa_core` library.

## Prerequisites

- **Rust** — install via [rustup](https://rustup.rs/). The pinned toolchain version is `1.91.1` (set in `rust-toolchain.toml`).
- **RISC Zero toolchain** — required to build guest ZK binaries:

  ```bash
  cargo install cargo-risczero
  cargo risczero install
  ```
- **SPEL toolchain** — provides `spel-cli` tools. Install from [logos-co/spel](https://github.com/logos-co/spel).
- **LEZ** — provides `wallet` CLI. Install from [logos-blockchain/logos-execution-zone](https://github.com/logos-blockchain/logos-execution-zone)

## Build & Test

```bash
# Lint the entire workspace (skips expensive guest ZK builds)
RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --all

# Run unit tests for all programs (no zkVM, no ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p token_program -p amm_program -p ata_program

# Run integration tests (dev mode skips ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p integration_tests

# Run all tests
RISC0_DEV_MODE=1 cargo test --workspace
```

Integration tests live in `integration_tests/tests/` and cover `token`, `amm`, and `ata` programs end-to-end through the zkVM using `RISC0_DEV_MODE=1` to skip proof generation. Each test file corresponds to a program:

- `integration_tests/tests/token.rs`
- `integration_tests/tests/amm.rs`
- `integration_tests/tests/ata.rs`

## Compile Guest Binaries

The guest binaries are compiled to the `riscv32im-risc0-zkvm-elf` target. This requires the RISC Zero toolchain.

```bash
cargo risczero build --manifest-path <PROGRAM>/methods/guest/Cargo.toml
```

Binaries are output to:

```
<PROGRAM>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<PROGRAM>.bin
```

## Deployment

```bash
# Deploy a program binary to the sequencer
wallet deploy-program <path-to-binary>

# Example
wallet deploy-program token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin
wallet deploy-program amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin
```

To inspect the `ProgramId` of a built binary:

```bash
spel-cli inspect <path-to-binary>
```

## Interacting with Programs via `spel-cli`

### Generate an IDL

The IDL describes the program's instructions and can be used to interact with a deployed program.

```bash
# Example
spel-cli generate-idl token/methods/guest/src/bin/token.rs > token/token-idl.json
spel-cli generate-idl amm/methods/guest/src/bin/amm.rs > amm/amm-idl.json
```

### Invoke Instructions

Use `spel-cli --idl <IDL> <INSTRUCTION> [ARGS...]` to call a deployed program instruction:

```bash
spel-cli --idl token/token-idl.json <instruction> [args...]
spel-cli --idl amm/amm-idl.json <instruction> [args...]
```
