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

## Use local wallet settings

Set `WALLET_MODE=local` to deploy against the wallet home's existing network
and accounts. The script leaves `LEE_WALLET_HOME_DIR` and
`NSSA_WALLET_HOME_DIR` unchanged, never restores keys, never creates wallet
accounts, and does not change the sequencer configuration.

Supply the five role accounts as account labels, BIP-32 paths, or account IDs.
The two definition accounts must be suitable for creating new token definitions;
the two token holdings and LP holding must be controlled by the local wallet.

```bash
WALLET_MODE=local \
  LOCAL_TOKEN_A_DEF_ACCOUNT=token-a-def \
  LOCAL_TOKEN_A_HOLDING_ACCOUNT=token-a-holding \
  LOCAL_TOKEN_B_DEF_ACCOUNT=token-b-def \
  LOCAL_TOKEN_B_HOLDING_ACCOUNT=token-b-holding \
  LOCAL_LP_HOLDING_ACCOUNT=lp-holding \
  TOKENS_CONFIG_OUT=apps/amm/amm-tokens.json \
  apps/amm/tests/testnet/setup-amm-testnet.sh
```

`TOKENS_CONFIG_OUT` defaults to the isolated test config. Set it explicitly to
the ignored local `apps/amm/amm-tokens.json` only when overwriting that file is
intended. `TEST_SEQUENCER_ADDR` and `FORCE_BOOTSTRAP` are rejected in local mode
so the script cannot alter or reset the local wallet.

## Notes

- **Wallet password.** The wallet's key storage is encrypted with a password —
  the one you enter to **unlock** it (the UI's **Connect** modal) so the app can
  sign transactions. First-time setup reads a throwaway password (the script
  feeds it via stdin, so it doesn't block), then `restore-keys` rebuilds the
  wallet from the test mnemonic and sets the **real** password to
  `TEST_WALLET_PASSWORD` (default `test`). So **unlock the UI with `test`**.
  Prefer no password? Run with `TEST_WALLET_PASSWORD=""` and unlock with an empty
  password.
- **Re-bootstrap.** `FORCE_BOOTSTRAP=1 apps/amm/tests/testnet/setup-amm-testnet.sh`
  re-restores the isolated `.wallet` (rewrites only that directory).
- **Overrides.** Isolated mode accepts `TEST_WALLET_HOME`, `TEST_MNEMONIC`,
  `TEST_WALLET_PASSWORD`, `TEST_WALLET_DEPTH`, `TEST_SEQUENCER_ADDR`, and
  `TOKENS_CONFIG_OUT`. Local mode requires the five `LOCAL_*_ACCOUNT` variables
  and accepts `TOKENS_CONFIG_OUT`.
- **Framework location.** `swap.mjs` loads `../result-mcp` by default; override
  with `LOGOS_QT_MCP=/abs/path/to/result-mcp`.
- **Artifacts.** On failure `swap.mjs` prints the `SwapCard` state and saves
  `apps/amm/tests/swap-*.png` (git-ignored) for inspection.

## Files

- `swap.mjs` — the end-to-end swap UI test.
- `testnet/setup-amm-testnet.sh` — isolated testnet + wallet bootstrap.
- `qml/`, `cpp/` — the module's own QML/C++ unit tests.
