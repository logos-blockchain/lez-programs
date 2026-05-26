# lez-programs

Essential programs for the **Logos Execution Zone (LEZ)** — a zkVM-based execution environment built on [RISC Zero](https://risczero.com/). Programs run inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf` target) and interact with the LEZ runtime via the `nssa_core` library.

## Programs

| Program | Description |
|---|---|
| **token** | Fungible and non-fungible token program — create definitions, mint/burn tokens, transfer, initialize accounts, print NFTs |
| **amm** | Constant-product AMM — add/remove liquidity and swap via chained calls to the token program |
| **ata** | Associated Token Account program — derives and initializes deterministic token holding accounts for a given owner and token definition |
| **stablecoin** | Collateral-backed position program — open collateral positions as a foundation for stablecoin debt issuance |
| **twap_oracle** | TWAP oracle — provides canonical on-chain price accounts consumed by other programs (e.g. stablecoin) |

## Apps

| App | Description |
|---|---|
| **amm-ui** | QML-based UI for interacting with the AMM program |

## Prerequisites

- **Rust** — install via [rustup](https://rustup.rs/). The pinned toolchain version is `1.91.1` (set in `rust-toolchain.toml`).
- **RISC Zero toolchain** — required to build guest ZK binaries:

  ```bash
  cargo install cargo-risczero
  cargo risczero install
  ```
- **SPEL toolchain** — provides `spel` and `wallet` CLI tools. Install from [logos-co/spel](https://github.com/logos-co/spel).
- **LEZ** — provides `wallet` CLI. Install from [logos-blockchain/logos-execution-zone](https://github.com/logos-blockchain/logos-execution-zone)

## Build & Test

```bash
# Lint the entire workspace (skips expensive guest ZK builds)
RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --all

# Run unit tests for all programs (no zkVM, no ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p token_program -p amm_program -p ata_program -p stablecoin_program -p twap_oracle_program

# Run integration tests (dev mode skips ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p integration_tests

# Run all tests
RISC0_DEV_MODE=1 cargo test --workspace
```

Integration tests live in `programs/integration_tests/tests/` and cover `token`, `amm`, and `ata` programs end-to-end through the zkVM using `RISC0_DEV_MODE=1` to skip proof generation. Each test file corresponds to a program:

- `programs/integration_tests/tests/token.rs`
- `programs/integration_tests/tests/amm.rs`
- `programs/integration_tests/tests/ata.rs`

`stablecoin` and `twap_oracle` are tested via their own unit tests (`cargo test -p stablecoin_program -p twap_oracle_program`).

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
wallet deploy-program programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin
wallet deploy-program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin
wallet deploy-program programs/ata/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/ata.bin
wallet deploy-program programs/stablecoin/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/stablecoin.bin
wallet deploy-program programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin
```

To inspect the `ProgramId` of a built binary:

```bash
spel inspect <path-to-binary>
```

## Interacting with Programs via `spel`

### Generate an IDL

The IDL describes the program's instructions and can be used to interact with a deployed program.

**Using the `idl-gen` crate** (no external toolchain required — this is what CI uses):

```bash
cargo run -p idl-gen -- programs/token/methods/guest/src/bin/token.rs > artifacts/token-idl.json
cargo run -p idl-gen -- programs/amm/methods/guest/src/bin/amm.rs > artifacts/amm-idl.json
cargo run -p idl-gen -- programs/ata/methods/guest/src/bin/ata.rs > artifacts/ata-idl.json
cargo run -p idl-gen -- programs/stablecoin/methods/guest/src/bin/stablecoin.rs > artifacts/stablecoin-idl.json
cargo run -p idl-gen -- programs/twap_oracle/methods/guest/src/bin/twap_oracle.rs > artifacts/twap_oracle-idl.json
```

**Using the `spel` CLI** (requires the SPEL toolchain):

```bash
spel generate-idl programs/token/methods/guest/src/bin/token.rs > artifacts/token-idl.json
spel generate-idl programs/amm/methods/guest/src/bin/amm.rs > artifacts/amm-idl.json
spel generate-idl programs/ata/methods/guest/src/bin/ata.rs > artifacts/ata-idl.json
spel generate-idl programs/stablecoin/methods/guest/src/bin/stablecoin.rs > artifacts/stablecoin-idl.json
spel generate-idl programs/twap_oracle/methods/guest/src/bin/twap_oracle.rs > artifacts/twap_oracle-idl.json
```

Generated IDL files are committed under `artifacts/`. CI will fail if a program's IDL is missing or out of date.

### Invoke Instructions

Use `spel --idl <IDL> <INSTRUCTION> [ARGS...]` to call a deployed program instruction:

```bash
spel --idl artifacts/token-idl.json <instruction> [args...]
spel --idl artifacts/amm-idl.json <instruction> [args...]
spel --idl artifacts/ata-idl.json <instruction> [args...]
spel --idl artifacts/stablecoin-idl.json <instruction> [args...]
spel --idl artifacts/twap_oracle-idl.json <instruction> [args...]
```
