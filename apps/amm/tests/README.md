# AMM UI tests

UI-driven tests for the AMM app. `swap.mjs` drives the running app through the
QML inspector (framework from
[`logos-co/logos-qt-mcp`](https://github.com/logos-co/logos-qt-mcp)): it selects
two tokens, enters an amount, submits a swap, and verifies the pool reserves
changed **on-chain**.

## Isolation

Test runs never touch your personal setup. Everything test-related lives under
`apps/amm/tests/testnet/` and is git-ignored:

| Concern | Test (isolated) | Your local dev |
| --- | --- | --- |
| Wallet home | `apps/amm/tests/testnet/.wallet/` | `~/.lee/wallet/` |
| Token config | `apps/amm/tests/testnet/amm-tokens.json` | `apps/amm/amm-tokens.json` |

`testnet/setup-amm-testnet.sh` bootstraps an isolated wallet from a **fixed
mnemonic** (`test test … junk`) so the token/holding account ids are
deterministic and reproducible across machines, then deploys the programs,
creates a seeded pool, and writes the test token config.

## Full isolated run

From the **repo root**, with `wallet` / `spel` / `cargo` / `nix` on `PATH`, a
local sequencer running, and the guest binaries built (`make build-programs`):

```bash
# 0. Once: build the JS test framework (symlinked where swap.mjs expects it)
nix build .#test-framework -o apps/amm/result-mcp

# 1. Bootstrap the isolated wallet + deploy programs + create the pool.
#    Writes apps/amm/tests/testnet/{.wallet, amm-tokens.json}. Nothing else is touched.
TEST_SEQUENCER_ADDR=http://127.0.0.1:3040 apps/amm/tests/testnet/setup-amm-testnet.sh

# 2. Terminal 1 — launch the UI against ONLY the isolated wallet + test tokens.
LEE_WALLET_HOME_DIR=$(pwd)/apps/amm/tests/testnet/.wallet \
  AMM_PROGRAM_BIN=$(pwd)/programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
  TOKENS_CONFIG=$(pwd)/apps/amm/tests/testnet/amm-tokens.json \
  nix run .#amm-ui

# 3. Terminal 2 — drive the swap test; watch it click through the live UI.
node apps/amm/tests/swap.mjs
```

Headless CI variant (no window, launches the app itself, pass/fail only):

```bash
nix build .#integration-test -L
```

## Notes

- **Wallet password.** The isolated wallet is restored with password `test`
  (`TEST_WALLET_PASSWORD`). If the UI shows the **Connect** modal instead of
  auto-opening, unlock with `test`.
- **Re-bootstrap.** `FORCE_BOOTSTRAP=1 apps/amm/tests/testnet/setup-amm-testnet.sh`
  re-restores the isolated `.wallet` (rewrites only that directory).
- **Overrides.** `TEST_WALLET_HOME`, `TEST_MNEMONIC`, `TEST_WALLET_PASSWORD`,
  `TEST_WALLET_DEPTH`, `TEST_SEQUENCER_ADDR` — see the script header.
- **Framework location.** `swap.mjs` loads `../result-mcp` by default; override
  with `LOGOS_QT_MCP=/abs/path/to/result-mcp`.
- **Artifacts.** On failure `swap.mjs` prints the `SwapCard` state and saves
  `apps/amm/tests/swap-*.png` (git-ignored) for inspection.

## Files

- `swap.mjs` — the end-to-end swap UI test.
- `testnet/setup-amm-testnet.sh` — isolated testnet + wallet bootstrap.
- `qml/`, `cpp/` — the module's own QML/C++ unit tests.
