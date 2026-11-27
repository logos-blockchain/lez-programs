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
- **SPEL toolchain** — provides `spel` and `wallet` CLI tools. Install from [logos-co/spel](https://github.com/logos-co/spel).
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

Since LEZ v0.2.5 a program is deployed through the `program_loader`: its binary is
uploaded as a chain of write-once segment accounts (one per 96 KiB chunk), and a header
account points at that chain. The wallet never generates these accounts itself, so create
them first. The header account's id is the program's address from then on.

```bash
BIN=programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin
N=$(( ($(wc -c < "$BIN") + 98303) / 98304 ))   # 96 KiB segments; the count must match exactly

# One header account and N segment accounts, all fresh
wallet account new public --label token-header
for i in $(seq 1 "$N"); do wallet account new public --label "token-seg-$i"; done

# Header and segments start empty, so a separate funded public account pays the fees.
# Accounts are labels or `Public/<base58-id>`; a bare id is mistaken for a label.
wallet program-loader deploy \
  --elf "$BIN" \
  --header token-header \
  --segments $(for i in $(seq 1 "$N"); do printf 'token-seg-%s ' "$i"; done) \
  --payer <funded-account> \
  --immutable   # optional: the program self-declares as immutable (not enforced)

# The program's address
wallet account id --account-id token-header
```

Repeat with each program's binary and its own labels (`amm`, `ata`, `stablecoin`,
`twap_oracle`, `token_mint_authority`). Segments are write-once: if a deploy fails
partway, use fresh segment accounts for the next attempt.
`apps/amm/tests/testnet/setup-amm-testnet.sh` scripts this flow for several programs.

To print the ImageID of a built binary (it identifies the code, but is not the program's
address):

```bash
spel -- program-id <path-to-binary>
```

## Interacting with Programs via `spel`

### Generate an IDL

The IDL describes the program's instructions and can be used to interact with a deployed program.

**Using the `idl-gen` crate** (no external toolchain required — this is what CI uses):

```bash
make idl
```

**Using the `spel` CLI** (requires the SPEL toolchain):

```bash
spel generate-idl programs/token/methods/guest/src/bin/token.rs > artifacts/token-idl.json
spel generate-idl programs/amm/methods/guest/src/bin/amm.rs > artifacts/amm-idl.json
spel generate-idl programs/ata/methods/guest/src/bin/ata.rs > artifacts/ata-idl.json
spel generate-idl programs/stablecoin/core > artifacts/stablecoin-idl.json   # actor model: the core crate, not a guest file
spel generate-idl programs/twap_oracle/methods/guest/src/bin/twap_oracle.rs > artifacts/twap_oracle-idl.json
spel generate-idl programs/token_mint_authority/methods/guest/src/bin/token_mint_authority.rs > artifacts/token_mint_authority-idl.json
```

Generated IDL files are committed under `artifacts/`. CI will fail if a program's IDL is missing or out of date.

### Invoke Instructions

Use `spel --idl <IDL> --program <PROGRAM_ID> --fee-payer <FEE_PAYER> -- <INSTRUCTION> [ARGS...]`
to call a deployed program instruction:

```bash
spel --idl artifacts/token-idl.json --program <TOKEN_PROGRAM_ID> --fee-payer <FEE_PAYER> -- <instruction> [args...]
spel --idl artifacts/amm-idl.json --program <AMM_PROGRAM_ID> --fee-payer <FEE_PAYER> -- <instruction> [args...]
spel --idl artifacts/ata-idl.json --program <ATA_PROGRAM_ID> --fee-payer <FEE_PAYER> -- <instruction> [args...]
spel --idl artifacts/twap_oracle-idl.json --program <TWAP_PROGRAM_ID> --fee-payer <FEE_PAYER> -- <instruction> [args...]
spel --idl artifacts/token_mint_authority-idl.json --program <FAUCET_PROGRAM_ID> --fee-payer <FEE_PAYER> -- <instruction> [args...]
```

The stablecoin runs on LEZ's actor model, where a transaction sends one message to a **root**
actor and must declare every other actor the message reaches. Its commands use `spel send`
(spel's `refactor/lez-v0.4.x` branch; see [`docs/token/README.md`](docs/token/README.md) for the
argument formats and `spel.toml`):

```bash
# The permissionless fee poke: the root is the engine; it reaches the clock and, when fees
# accrued, the stablecoin definition and the surplus holding under the token program.
spel --program stablecoin send --fee-payer <FEE_PAYER> --root <ENGINE_ID> \
  --actor <CLOCK_ACCOUNT_ID>@<CLOCK_PROGRAM> \
  --actor <STABLECOIN_DEFINITION_ID>@<TOKEN_PROGRAM> \
  --actor <SURPLUS_HOLDING_ID>@<TOKEN_PROGRAM> -- \
  accrue-stability-fee
```

[`docs/stablecoin/README.md`](docs/stablecoin/README.md) §9.5 lists the root, the declared
actors and the signers of every stablecoin message, and
`cargo run -p stablecoin_program --example stablecoin_pdas -- <STABLECOIN_PROGRAM_ID>` prints
the engine, stablecoin definition and surplus holding ids.

- `--program` is the program's header account id (see [Deployment](#deployment)); spel
  rejects a binary path.
- `--fee-payer` is a funded account your wallet holds. Without it the first signer pays,
  which fails when the signers are fresh accounts, and instructions with no signers are
  refused.
- spel reports `Transaction confirmed` even when the program rejected the instruction, so
  check the result with `spel --idl <IDL> inspect <ACCOUNT_ID> --type <TYPE>`.

See [docs/testnet-runbook.md](docs/testnet-runbook.md) for complete, ordered examples.
