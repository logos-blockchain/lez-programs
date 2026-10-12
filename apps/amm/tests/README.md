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
- `remove-liquidity.mjs` opens the wallet's seeded **A/B** position from the
  positions view, uses the Manage dropdown to open the remove sheet, withdraws
  **50%** (the slider default), submits, and verifies the A/B pool reserves shrank
  **on-chain**.
- `custom-token.mjs` pastes token **D**'s id (created on-chain by the setup but
  deliberately **absent** from the token config) into a Liquidity token slot and
  verifies the app resolves it, selects it, and **persists** it to the custom-token
  store — the "add an unlisted token by id" path. No pool / submit involved.
- `faucet-swap.mjs` funds a **brand-new** account ("Token A Holder 2") that starts
  with no token A: it `initialize_account`s the account's token A holding, then
  submits a **FaucetMint** through the token-mint-authority program (both via `spel`,
  out of band from the UI) to mint token A into it — the faucet only mints into an
  **existing** holding, so the init must come first — then **refreshes** the UI so
  the newly-funded holding appears in the sell picker, selects it, and swaps **A→B**,
  verifying the pool reserves changed **on-chain**. Reads `testnet/faucet.json`
  (written by the setup) and needs `spel` on `PATH` plus `LEE_WALLET_HOME_DIR`
  pointing at the isolated test wallet.

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

> **Rebuild BOTH `spel` and `wallet` against LEZ v0.3.0 first.** They share the
> wallet storage format, so a mismatched pair fails with `Failed to parse storage
> file: invalid type: integer 0, expected a string` — and neither reports its LEZ
> version (`wallet --version` prints only `0.1.0`), so compare build dates if in
> doubt. `spel` additionally needs struct instruction arguments to encode `--facts`;
> an older one refuses it with *"only enum arguments are supported"*.
>
> ```bash
> cargo install --path <spel>/spel-cli --bin spel --locked     # v0.3.0-compat branch
> cargo install --git https://github.com/logos-blockchain/logos-execution-zone.git \
>   --tag v0.3.0 -p wallet --locked
> ```
>
> The setup script then re-restores `testnet/.wallet` by itself if its `storage.json`
> is in the pre-v0.3.0 format (it moves the old file aside).

```bash
# 0. Once: build the JS test framework (symlinked where swap.mjs expects it)
nix build .#test-framework -o apps/amm/result-mcp

# 1. Bootstrap the isolated wallet + deploy programs + create the pool.
#    Writes apps/amm/tests/testnet/{.wallet, amm-tokens.json}. Nothing else is touched.
TEST_SEQUENCER_ADDR=http://127.0.0.1:3040 apps/amm/tests/testnet/setup-amm-testnet.sh

# 2. Terminal 1 — launch the UI against ONLY the isolated wallet + test tokens.
#    CUSTOM_TOKEN_CONFIG (where the app persists tokens added by id) defaults to the
#    per-user store; set it to an isolated path so custom-token.mjs controls the store
#    (it clears this file before + after running). Required for custom-token.mjs to
#    avoid touching your real custom-token store.
LEE_WALLET_HOME_DIR=$(pwd)/apps/amm/tests/testnet/.wallet \
  AMM_PROGRAM_BIN=$(pwd)/programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
  TOKENS_CONFIG=$(pwd)/apps/amm/tests/testnet/amm-tokens.json \
  CUSTOM_TOKEN_CONFIG=$(pwd)/apps/amm/tests/testnet/custom-tokens.json \
  nix run .#amm-ui

# 3. Terminal 2 — drive a test; watch it click through the live UI.
node apps/amm/tests/swap.mjs          # swap against the seeded A/B pool
node apps/amm/tests/create-pool.mjs   # create the (unseeded) A/C pool
node apps/amm/tests/add-liquidity.mjs # add liquidity to the seeded A/B pool
node apps/amm/tests/remove-liquidity.mjs # remove 50% from the seeded A/B pool
node apps/amm/tests/custom-token.mjs  # add token D (unlisted) by id

# faucet-swap needs spel on PATH + the isolated wallet home (so it can sign the mint)
LEE_WALLET_HOME_DIR=$(pwd)/apps/amm/tests/testnet/.wallet \
  node apps/amm/tests/faucet-swap.mjs # faucet-mint TKA to holder2, refresh, then swap
```

Headless CI variant (no window, launches the app itself, pass/fail only):

```bash
nix build .#integration-test -L
```

## LEZ v0.3.0 notes

A plan phase can no longer read a shard, so values a program used to look up now
travel in the instruction and are pinned on chain by a guard on the shard they came
from. That reaches the tests in four places:

- **`setup-amm-testnet.sh`'s `new-definition`** declares `--facts` (read back from
  the config account, the way `amm_ffi`'s `load_config` does),
  `--definition-token-a/b-id`, and `--now`.
- **`now` is pinned exactly** for the AMM, and the clock advances every block, so a
  read-then-submit races it. `run_tx_at_clock` re-reads the clock and resubmits —
  the oracle's own docs call this "the keeper's problem, not a correctness one".
  `faucet-swap.mjs` needs no retry: the faucet's guard is deliberately *bounded*
  (`now <= timestamp`), so one read stays true.
- **`spel inspect` requires `--program`**: an account holds one shard per program,
  so there is no single "the account's data" to decode.
- **The native balance became a shard**, so `wallet account get` prints
  `Balance <n>, nonce <m>`; the `"balance"` JSON key the funding checks used to grep
  is gone.

The five UI-driven tests (`swap`, `create-pool`, `add-liquidity`,
`remove-liquidity`, `custom-token`) needed no changes: they read and submit through
the app's own backend, so the AMM module's FFI supplies every declared value.
`faucet-swap.mjs` is the one that calls `spel` directly, hence the edits above plus
`--kind Fungible` on `initialize-account`.

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
- `remove-liquidity.mjs` — the end-to-end remove-liquidity UI test (removes 50% from the A/B pool).
- `faucet-swap.mjs` — faucet-mint token A to a fresh account, refresh the UI, then swap A→B.
- `testnet/setup-amm-testnet.sh` — isolated testnet + wallet bootstrap (TKA/TKB/TKC,
  seeds the A/B pool only; also deploys the faucet, sets every token's mint authority
  to the faucet PDA, and writes `testnet/faucet.json` for faucet-swap.mjs).
- `qml/`, `cpp/` — the module's own QML/C++ unit tests.
