# AMM core module

`amm_module` — the AMM business logic as a **headless Logos `core` module**
(`interface: "universal"`). It exposes the AMM's on-chain operations so they can
be driven identically from the QML UI (`apps/amm`, via the generated
`modules().amm_module` caller) and from the CLI (`logoscore call amm_module …`).

See the [Logos developer guide](https://github.com/logos-co/logos-tutorial/blob/master/logos-developer-guide.md)
for the module framework.

## What it does

The impl class `AmmModuleImpl` (`src/amm_module_impl.{h,cpp}`) is a **transport
adapter**: the AMM domain math lives in the Rust `amm_client` crate (a
transport-independent JSON FFI), and this module sequences those pure ops with
chain I/O delegated to the `logos_execution_zone` wallet module. Its public
methods (the module API is generated from the header) are:

- `resolvePool(defAHex, defBHex)` — derives the pool PDAs
  (config/pool/vaults/current-tick) and reads the pool's on-chain reserves.
  Returns `{ exists: false, error }` when the AMM isn't configured/initialized or
  the pool has no liquidity.
- `swapExactInput(defAHex, defBHex, userInputHoldingHex, userOutputHoldingHex, amountIn, minOut, deadline)`
  — submits an on-chain `SwapExactInput` transaction (defA = token in,
  defB = token out); returns the tx hash (or empty on failure). See
  **Amount / id conventions** below.
- `tokenList()` — reads the `TOKENS_CONFIG` JSON array and returns it with
  `definitionId`/`holding` normalized to hex.
- `newPositionContext(request, walletOpen, refreshWalletAccounts)` — the
  add-liquidity view state (available tokens, fee tiers, warnings) as a
  `new-position.v1` map.
- `quoteNewPosition(request, walletOpen)` — prices an add-liquidity request
  against current on-chain state (read-only).
- `submitNewPosition(request, quoteHash, walletOpen, freshLpId)` — submits an
  add-liquidity transaction. When the quote needs a fresh LP holding and
  `freshLpId` is empty, returns `{ status: "requires_fresh_lp" }` **without**
  submitting: the caller (the app backend, which owns the wallet keyset) creates
  the account and calls again with its id. Headless callers pre-create an LP
  holding and pass it.

## How it fits together

```
QML  ──modules().amm_module──┐        ┌── amm_client (Rust cdylib, JSON FFI):
CLI  ──logoscore call────────┤        │   PDA derivation, account decode,
                             ▼        │   quote/plan math, instruction encoding
                        amm_module ───┤   — transport-independent (external_libraries)
                             │        │
                             │        └── logos_execution_zone (dependency):
                             ▼            chain reads + tx submit + base58,
                                          via modules().logos_execution_zone.*
```

The `amm_client` crate is deliberately I/O-free — each op takes the account data
it needs as JSON input and returns a JSON result. This module is the transport
adapter the crate is designed to require: it fetches accounts through the wallet
module (`get_account_public`, `list_accounts`), hands them to the pure Rust op,
and submits the plan the op returns (`send_generic_public_transaction`). It reads
the **same** shared wallet instance the UI opened (Basecamp loads core modules as
singletons; standalone the LogosAPI client cache dedups the connection), so it
never opens a second wallet.

The impl is deliberately **Qt-free** (`std::string` / `LogosMap` / `LogosList` /
`nlohmann::json`), as the universal authoring model requires.

## Amount / id conventions

**Account ids are hex**, not base58. The `*Hex` args are parsed as 32-byte hex;
a base58 id (what the wallet/runbook display) fails that parse. Convert with
`tokenList()` (it emits hex) or `logos_execution_zone.account_id_from_base58 <base58>`.

**Amounts (`amountIn`/`minOut`, u128) and `deadline` (u64 unix-ms)** are declared
`nlohmann::json`, so each accepts **either a JSON number or a decimal string**:

- **small integer** → pass it bare: `1000`
- **big value** (an amount above the JSON/int64 range, e.g. `1e18` base units for
  an 18-decimal token, **and** the unix-ms deadline, which is always large) →
  pass it as a **quote-wrapped string**: `'"1000000000000000000"'`

Why the split: `logoscore` promotes any bare number past ~2³¹ to a JSON *double*,
which can't hold a large integer exactly. The module therefore **rejects JSON
floats** (rather than submit a silently-rounded amount) and requires big values
as strings, which are bit-exact. A quoted CLI arg (`'"…"'`) reaches the module
with the quotes folded into the value; the string branch strips that wrapper.
The UI passes `QString` (→ `QVariant` → string branch) and is unaffected.

## Build

Built from the **repo-root** flake (which provides the `amm_client` library it
links):

```bash
nix build .#amm-module
# output: result/lib/amm_module_plugin.dylib (+ libamm_client.dylib)
```

## Runtime configuration

Both are absolute-path env vars set on the **process that hosts the module**
(the `logoscore` daemon, or Basecamp) — not on the `call`:

- `AMM_PROGRAM_BIN` — the deployed `amm.bin`. Required; its ELF determines the
  program id and every derived PDA. Without it, `resolvePool` returns
  `{ exists: false, error: "no_program_bin" }`.
- `TOKENS_CONFIG` — JSON array of `{ symbol, name, definitionId, holding, decimals }`
  consumed by `tokenList()`.

## Headless usage with `logoscore`

### Prerequisites

Have all of the following in place before staging the modules dir:

1. **Nix** with flakes enabled (same as the rest of the repo).

2. **The runtime CLIs** (from their own flakes, per the developer guide):

   ```bash
   nix profile install 'github:logos-co/logos-logoscore-cli'   # logoscore (daemon + client)
   nix build 'github:logos-co/logos-module#lm'                 # lm (static plugin inspector, optional)
   ```

   `lm` introspects a built plugin without running it — handy to confirm the API
   (`lm result/lib/amm_module_plugin.dylib` shows methods, signatures, deps).

3. **This module, built** (produces `amm_module_plugin.dylib` + `libamm_client.dylib`):

   ```bash
   nix build .#amm-module        # from the repo root; output under result/lib/
   ```

4. **The wallet module it depends on, built** — `logos_execution_zone` is a
   *separate repo*, not part of this tree. Build the **same rev** this module
   pins as its `logos_execution_zone` flake input (mismatched revs = ABI/ImageID
   drift), producing `logos_execution_zone_plugin.dylib` + `libwallet_ffi.dylib`:

   ```bash
   nix build 'github:gravityblast/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr'
   # output under result/lib/ — copy it aside before building amm-module (both use ./result)
   ```

5. **The deployed `amm.bin`** for `AMM_PROGRAM_BIN` — the exact binary running on
   your target sequencer (its ELF fixes the program id and every PDA). See
   `apps/amm/README.md` and the testnet runbook.

6. **A tokens config** for `TOKENS_CONFIG` — a JSON array of
   `{ symbol, name, definitionId, holding, decimals }` (e.g. the repo's
   `amm-tokens.json`).

7. **A wallet** at `~/.lee/wallet` (`wallet_config.json` with `sequencer_addr`
   pointing at your sequencer, plus `storage.json` with your accounts), and a
   **running sequencer** with the AMM initialized and a pool holding liquidity.
   `tokenList` reads `TOKENS_CONFIG` from disk and needs no wallet, but every
   other op (including `resolvePool`) reads on-chain through the wallet module's
   `get_account_public`, which needs the wallet **open** (the handle is null
   until `open`/`create_new`); `swapExactInput` additionally needs it **synced**
   (see below).

### Staging the modules directory

**Core modules get no `.lgx` from the builder** (only UI modules do), so stage a
modules directory by hand — one subdir per module, each with `manifest.json` +
`variant` + the plugin dylib (and its sibling FFI dylib, since rpath is
`@loader_path`). The daemon discovers modules from this layout; it does **not**
verify the manifest hashes at load time.

```
modules/
  amm_module/
    amm_module_plugin.dylib
    libamm_client.dylib
    variant                     # one line: darwin-arm64-dev
    manifest.json
  logos_execution_zone/
    logos_execution_zone_plugin.dylib
    libwallet_ffi.dylib
    variant
    manifest.json
```

`amm_module/manifest.json`:

```json
{
  "name": "amm_module", "type": "core", "version": "0.1.0",
  "manifestVersion": "0.2.0", "dependencies": ["logos_execution_zone"],
  "main": { "darwin-arm64-dev": "amm_module_plugin.dylib" }
}
```

Start the daemon **with the env vars set on it**, then load the dependency
first, then the module:

```bash
AMM_PROGRAM_BIN=/abs/path/to/amm.bin \
TOKENS_CONFIG=/abs/path/to/amm-tokens.json \
logoscore -D -m ./modules --persistence-path ./data

logoscore load-module logos_execution_zone     # dependency first
logoscore load-module amm_module
```

`tokenList` reads `TOKENS_CONFIG` from disk — no wallet needed:

```bash
logoscore call amm_module tokenList
```

Every other op reads on-chain through the wallet module's `get_account_public`,
which fails on a null wallet handle (surfacing as an absent pool), so open the
wallet first — `resolvePool` then works:

```bash
logoscore call logos_execution_zone open ~/.lee/wallet/wallet_config.json ~/.lee/wallet/storage.json
logoscore call amm_module resolvePool <defA_hex> <defB_hex>
```

`swapExactInput` reuses that open wallet but additionally needs it **synced**
(nothing opens/syncs it for you headlessly). Note the amount/deadline
conventions above — small amount bare, deadline (and any big amount) quoted:

```bash
logoscore call amm_module swapExactInput \
  <defA_hex> <defB_hex> <inputHolding_hex> <outputHolding_hex> \
  1000 1 '"32503680000000"'
# big amount: replace 1000 with '"1000000000000000000"'
```

### Debugging

Set `AMM_DEBUG=1` on the daemon to trace every `swapExactInput` step (parsed
args, the assembled account list, and the raw `send_generic_public_transaction`
reply) to the module host's stderr, which the daemon captures in its log. A
failed swap returns an empty tx hash; the trace shows the reason.

### Wallet sync gotcha

`get_balance` reads the wallet's *local synced state*, and `swapExactInput`
builds transactions against it. If you reset/reinitialize the sequencer, the
wallet's `storage.json` may keep a stale `last_synced_block` ahead of the new
chain — transactions then reference dead state and the sequencer rejects them
(reserves don't move). Reset the cursor (`last_synced_block: 0`, keep
`key_chain`/`labels`) and re-`open` + `sync_to_block <height>` to re-sync from
genesis. `resolvePool` is a **live** sequencer read, so the stale cursor doesn't
affect it — but it still needs the wallet **open**: the read goes through the
wallet's sequencer connection (not its private keys), which only exists once the
wallet is opened.

## Install into Basecamp

`amm_module` is a **core** module (installed into `modules/`, alongside the
wallet module), not a UI plugin. Build its `.lgx` from the root flake and
install with `lgpm --modules-dir …` (see `apps/amm/README.md` for the full
three-package flow: `logos_execution_zone` + `amm_module` + the `amm_ui` UI
plugin).

## QtRO / byte-string note

`send_generic_public_transaction` takes `instruction` as a byte string
(`std::vector<uint8_t>`). This module sends the plan's u32 words as their
little-endian bytes and references the program by its id hex (not the raw ELF).
It requires the wallet module built with the byte-string `instruction` param —
the fork pinned as the `logos_execution_zone` input. See
`docs/amm-swap-qtro-serialization-bug.md`.

## Known follow-ups

- **`swapExactOutput` is not exposed yet.** The on-chain program supports it
  (`amm_core::Instruction::SwapExactOutput`, identical account layout to
  `SwapExactInput`), but the client path was only ever built for exact-input:
  `amm_client` has no exact-output op and neither the UI nor this module has a
  `swapExactOutput` method. Adding it is a near-copy of the exact-input path — an
  `amm_swap_exact_output_*` op in the crate plus a `swapExactOutput` method here.
