# lez-programs

Essential programs for the **Logos Execution Zone (LEZ)** — a zkVM-based execution environment built on [RISC Zero](https://risczero.com/). Programs run inside the RISC Zero zkVM (`riscv32im-risc0-zkvm-elf` target) and interact with the LEZ runtime via the `lee_core` library.

## Programs

| Program | Description |
|---|---|
| **token** | Fungible and non-fungible token program — create definitions, mint/burn tokens, transfer, initialize accounts, print NFTs |
| **amm** | Constant-product AMM — add/remove liquidity and swap via chained calls to the token program |
| **ata** | Associated Token Account program — derives and initializes deterministic token holding accounts for a given owner and token definition |
| **stablecoin** | Collateral-backed position program — open collateral positions as a foundation for stablecoin debt issuance |
| **twap_oracle** | TWAP oracle — provides canonical on-chain price accounts consumed by other programs (e.g. stablecoin) |
| **token-mint-authority** | Testnet faucet mint authority — holds the mint authority for faucet token definitions and lets any account self-mint a fixed amount, rate-limited to once per day per account and token, via a chained call to the token program |

## Apps

| App | Description |
|---|---|
| **amm** | QML-based UI for interacting with the AMM program |

## Running Apps

Apps live under `apps/` and are standalone UI applications. Each app has its own `README.md` with full details.

Apps use [Nix](https://nixos.org/) flakes. Enable flakes if you haven't already:

```bash
mkdir -p ~/.config/nix && echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

### Example (`apps/amm`)

The AMM UI is built from the **repository-root** flake (which also provides the
`amm_client_ffi` library it links). Run it from the repo root by its named
attribute — there is no bare `nix run .` default, so future apps are
`nix run .#<name>`:

```bash
# Run the app (from the repo root)
nix run .#amm-ui

# Update pinned dependencies
nix flake update
```

To use the Swap view, also set `AMM_PROGRAM_ID` (the deployed AMM program's
address), `AMM_CONFIG_ID` (its config account) and `TOKENS_CONFIG` (your token
list) — use absolute paths, from the repo root:

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> \
AMM_CONFIG_ID=<amm-config-id> \
TOKENS_CONFIG=$(pwd)/apps/amm/amm-tokens.json \
nix run .#amm-ui
```

See `apps/amm/README.md` for full details on these variables.

## Prerequisites

- **Rust** — install via [rustup](https://rustup.rs/). The pinned toolchain version is set in `rust-toolchain.toml`.
- **RISC Zero toolchain** — required to build guest ZK binaries:

  ```bash
  cargo install cargo-risczero
  cargo risczero install
  ```
- **SPEL CLI** for actor IDLs and messages. [Installation](#interacting-with-actor-programs-via-spel)
- **LEZ** — provides `wallet` CLI. Install from [logos-blockchain/logos-execution-zone](https://github.com/logos-blockchain/logos-execution-zone)

## Build & Test

```bash
# Lint the entire workspace (skips expensive guest ZK builds)
make clippy

# Format check
make fmt

# Run unit tests for all programs (no zkVM, no ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p token_program -p amm_program -p ata_program -p stablecoin_program -p twap_oracle_program -p token_mint_authority_program

# Run integration tests (dev mode skips ZK proof generation)
RISC0_DEV_MODE=1 cargo test -p integration_tests

# Run all tests
make test
```

Integration tests live in `programs/integration_tests/tests/` and cover `token`, `amm`, `ata`, `stablecoin`, and `token-mint-authority` programs end-to-end through the zkVM using `RISC0_DEV_MODE=1` to skip proof generation. Each test file corresponds to a program:

- `programs/integration_tests/tests/token.rs`
- `programs/integration_tests/tests/amm.rs`
- `programs/integration_tests/tests/ata.rs`
- `programs/integration_tests/tests/stablecoin.rs`
- `programs/integration_tests/tests/token_mint_authority.rs`

`twap_oracle` is tested via its own unit tests (`cargo test -p twap_oracle_program`).

## Compile guest binaries

From the repository root, build the release binaries with Docker BuildKit.
Set `PROGRAMS` to the space-separated names of the programs to build.

```bash
make build-programs PROGRAMS=token
```

The build writes each deployable binary to `target/guest/<program>.bin`.

## Deployment

Set `PAYER` to a funded account in your configured LEZ wallet. SPEL creates
the header and segment accounts, uploads the binary, and prints the deployed
header address on stdout. Use that address in subsequent calls.

```bash
TOKEN=$(spel deploy target/guest/token.bin --fee-payer "$PAYER")
```

From a program directory with a `spel.toml`, `spel deploy --fee-payer "$PAYER"`
uses its configured binary. Every deployment gets a fresh address. To inspect
the code's ImageID locally, run `spel program-id target/guest/token.bin` from
the repository root.

## Interacting with actor programs via SPEL

The LEZ v0.4 actor programs use SPEL IDL v2. Install the CLI from the same
revision as the workspace's IDL generator, using SPEL's Rust toolchain.

```bash
cargo +1.98.1 install --locked --git https://github.com/logos-co/spel.git \
  --rev 0d42ddb914ddaf2766e0b94661d97d3a2886792b spel
```

Run `make idl` from the repository root to regenerate the ported programs' IDLs
under `artifacts/`. To print one IDL, give SPEL its core crate. From a program
directory with a `spel.toml`, `spel generate-idl` finds the core crate and
`spel encode` loads the program's IDL. Both commands work offline.

```bash
spel generate-idl programs/token/core
```

To transfer one base unit between two existing public token holdings, set
`TOKEN` to the deployed token program's header address, `FROM` and `TO` to the
holding addresses, and `PAYER` to a funded wallet account. The source holding
receives and signs the message. SPEL declares the destination token actor from
the `to` field.

```bash
spel --idl artifacts/token-idl.json --program "$TOKEN" --fee-payer "$PAYER" \
  send --root "$FROM" --signer "$FROM" -- transfer --to "$TO" --amount 1
spel --idl artifacts/token-idl.json --program "$TOKEN" inspect "$TO"
```

For public sends, exit status 0 means a declared actor changed. Status 2 means
simulation refused the call or the included transaction changed no declared
actor. Status 1 reports an error. The token and NFT guides cover actor calls.
The v0.2.5 runbook covers legacy instruction-based programs.
[Token guide](docs/token/README.md) · [NFT guide](docs/nft/README.md) · [v0.2.5 runbook](docs/testnet-runbook.md)
