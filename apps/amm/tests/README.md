# AMM UI tests

UI-driven tests for the AMM app, driving the running app through the QML
inspector (framework from
[`logos-co/logos-qt-mcp`](https://github.com/logos-co/logos-qt-mcp)):

- `swap.mjs` selects two tokens, enters an amount, submits a swap, and verifies
  the **A/B** pool reserves changed **on-chain**.
- `create-pool.mjs` selects the **A/C** pair (which the setup script leaves
  unseeded — only A/B is created), submits a pool creation, and verifies the A/C
  pool now exists **on-chain**.
- `add-liquidity.mjs` selects the seeded **A/B** pair, asserts the CTA stays
  disabled until deposit amounts are entered, submits an add, and verifies the
  A/B pool reserves grew **on-chain**.

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

# 3. Terminal 2 — drive a test; watch it click through the live UI.
node apps/amm/tests/swap.mjs          # swap against the seeded A/B pool
node apps/amm/tests/create-pool.mjs   # create the (unseeded) A/C pool
node apps/amm/tests/add-liquidity.mjs # add liquidity to the seeded A/B pool
```

Headless CI variant (no window, launches the app itself, pass/fail only):

```bash
nix build .#integration-test -L
```

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
- **Overrides.** `TEST_WALLET_HOME`, `TEST_MNEMONIC`, `TEST_WALLET_PASSWORD`,
  `TEST_WALLET_DEPTH`, `TEST_SEQUENCER_ADDR` — see the script header.
- **Framework location.** `swap.mjs` loads `../result-mcp` by default; override
  with `LOGOS_QT_MCP=/abs/path/to/result-mcp`.
- **Artifacts.** On failure `swap.mjs` prints the `SwapCard` state and saves
  `apps/amm/tests/swap-*.png` (git-ignored) for inspection.

## Files

- `swap.mjs` — the end-to-end swap UI test (A/B pool).
- `create-pool.mjs` — the end-to-end create-pool UI test (creates the A/C pool).
- `add-liquidity.mjs` — the end-to-end add-liquidity UI test (adds to the A/B pool).
- `testnet/setup-amm-testnet.sh` — isolated testnet + wallet bootstrap (TKA/TKB/TKC,
  seeds the A/B pool only).
- `qml/`, `cpp/` — the module's own QML/C++ unit tests.
