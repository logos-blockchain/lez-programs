# AMM core module

`amm_module` — the AMM business logic as a **headless Logos `core` module**
(`interface: "universal"`). It exposes the AMM's on-chain operations so they can
be driven identically from the QML UI (`apps/amm`, via the generated
`modules().amm_module` caller) and from the CLI (`logoscore call amm_module …`).

See the [Logos developer guide](https://github.com/logos-co/logos-tutorial/blob/master/logos-developer-guide.md)
for the module framework.

## What it does

The impl class `AmmModuleImpl` (`src/amm_module_impl.{h,cpp}`) is a **transport
adapter**: the AMM domain math lives in the Rust `amm_ffi` crate (a
transport-independent JSON FFI), and this module sequences those pure ops with
chain I/O delegated to the `lez_core` wallet module. Its public
methods (the module API is generated from the header) are:

**Reads**
- `resolvePoolAccount(defAHex, defBHex)` — derives the pool PDA and reads/decodes
  the pool account (reserves in the caller's token order). On success
  `{ status: "ok", error: "", poolId, defAHex, defBHex, vaultAId, vaultBId,
  lpDefinitionId, reserveA, reserveB, liquiditySupply, feeBps }`. The module reads
  `feeBps` from the instance config and `liquiditySupply` from the LP token
  definition so direct LP burns are reflected in pool shares. An absent /
  uninitialized pool or one with no liquidity is `{ status: "error", error:
  "no_pool", poolId }` (other codes: `no_program_bin`, `amm_not_initialized`,
  `bad_config`, `lp_supply_unavailable`, `pool_read_failed`). A failed read or an
  occupied foreign-owned pool address is not treated as a creatable pool. Every
  method that reads a pool reports these two conditions with the same codes;
  the swap submits, which return an empty string on failure, stop before planning.
- `configAccount()` — decodes the AMM instance's config (authority, the
  token/oracle program ids it was initialized with, `swapFeeBps` — the
  instance-wide swap fee — and `protocolFeeBps` — the fraction of that swap fee
  diverted to the protocol, `0` = none).
- `feeTiers()` — **legacy.** Historical fee tiers `[1, 5, 30, 100]`; fees are now
  instance-wide (set at `initialize`, any value below 100%), so this is unused.
- `tokenHoldings(walletOpen)` — the connected wallet's fungible token holdings.
- `resolveTokens(request, walletOpen)` — resolves an app-provided set of token
  ids into selector rows (definition + wallet holding per id). The app owns the
  id set, so there is no network envelope or process-cached wallet state here.

**Quotes** (read-only pricing; no submit)
- `swapExactInQuote` / `swapExactOutQuote` — price a swap.
- `createPoolQuote` / `addLiquidityQuote` / `removeLiquidityQuote` — price a
  liquidity op.

**Submits** (on-chain transactions; return `{ status, error, transactionId }`,
except the two swaps which return a bare tx hash)
- `swapExactInput` / `swapExactOutput` (defA = token in, defB = token out).
- `createPool` / `addLiquidity` / `removeLiquidity` — the create/add paths take a
  fresh `lpHoldingId` the app supplies (the module never creates wallet accounts).
- `syncReserves` — permissionless keeper op refreshing stored reserves + TWAP tick.
- `createPriceObservations` / `createOraclePriceAccount` — seed a pool's TWAP feed.
- `transferOwnership` — admin-only `UpdateConfig` handing over the authority.
- `withdrawProtocolFees` — admin-only; drains one token's accrued protocol fees
  (the per-`(config, token)` PDA) to a destination holding of that token. Takes
  `{ tokenDefinitionId, destinationId, amount }`.

Every submit names its **fee payer** (LEZ charges public transactions): a funded
public account in the connected wallet, passed as `feePayerId` in the request
map, or as the trailing argument of the two swaps. It may be one of the signers
or a separate account that co-signs the fee; it is never inferred, because the
signers can all be unfunded. A missing payer, or one that takes part without
signing, fails with `invalid_fee_payer` (an empty hash for the swaps).

The module header lists all public methods and their request and response fields.
[AMM module API](src/amm_module_impl.h)

## How it fits together

```
QML  ──modules().amm_module──┐        ┌── amm_ffi (Rust cdylib, JSON FFI):
CLI  ──logoscore call────────┤        │   PDA derivation, account decode,
                             ▼        │   quote/plan math, instruction encoding
                        amm_module ───┤   — transport-independent (external_libraries)
                             │        │
                             │        └── lez_core (dependency):
                             ▼            chain reads + tx submit + base58,
                                          via modules().lez_core.*
```

The `amm_ffi` crate is deliberately I/O-free — each op takes the account data
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

**Account ids accept base58 or hex.** The `*Hex` args and request-map ids are
normalized at each method's boundary (via `lez_core.account_id_from_base58`
for base58 inputs), so the wallet/runbook's base58 ids can be passed directly.

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

Built from the **repo-root** flake (which provides the `amm_ffi` library it
links):

```bash
nix build .#amm-module
# output: result/lib/amm_module_plugin.dylib (+ libamm_ffi.dylib)
```

## Runtime configuration

These env vars are set on the **process that hosts the module** (the
`logoscore` daemon, or Basecamp) — not on the `call`. An app that selects a
registry network passes the same ids through `setAmmProgramId` / `setConfigId`
instead:

- `AMM_PROGRAM_ID` — the deployed AMM program's address (base58 or hex): the
  account its `program_loader` header was deployed to. Required without a
  registry selection; it determines every derived PDA. Since LEZ v0.2.5 it can't
  be derived from `amm.bin`, so `AMM_PROGRAM_BIN` is no longer used. Without it,
  `resolvePoolAccount` returns `{ status: "error", error: "no_program_bin" }`.
- `AMM_CONFIG_ID` — the AMM instance's config account.

(The token list config `TOKENS_CONFIG` is an **app** concern now — the module no
longer reads it; see `apps/amm/README.md`.)

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

3. **The AMM and wallet module packages**. From the repository root, build
   both for the local platform. The `amm_lez_core` input selects the actor-aware
   `lez_core` 0.6.0 revision from the root lockfile.

   ```bash
   nix build '.#amm-module-lgx' --out-link result-amm
   nix build --inputs-from . 'amm_lez_core#lgx' --out-link result-wallet
   ```

4. **The Logos package manager**, for staging both packages:

   ```bash
   nix profile install 'github:logos-co/logos-package-manager#cli'
   ```

5. **The deployed AMM program id** for `AMM_PROGRAM_ID` (and its config id for
   `AMM_CONFIG_ID`) — the header account the AMM was deployed to on your target
   sequencer. Use the IDs from that deployment or its network registry.

6. **A wallet** at `~/.lee/wallet` (`wallet_config.json` pointing at your
   sequencer, plus `storage.json` with your accounts), and a
   **running sequencer** with the AMM initialized and a pool holding liquidity.
   Every op (including `resolvePoolAccount`) reads on-chain through the wallet
   module's `get_account_public`, which needs the wallet **open** (the handle is
   null until `open`/`create_new`); `swapExactInput` additionally needs it
   **synced** (see below).

### Staging the modules directory

Install both packages in a directory for the daemon. Each package includes
the plugin, libraries and manifest for the selected variant.

```bash
mkdir -p ./runtime/modules
lgpm --modules-dir "$PWD/runtime/modules" install --file result-wallet/*.lgx
lgpm --modules-dir "$PWD/runtime/modules" install --file result-amm/*.lgx
```

Start the daemon **with the env vars set on it**, then load the dependency
first, then the module:

```bash
AMM_PROGRAM_ID=<deployed-amm-program-id> \
AMM_CONFIG_ID=<amm-config-id> \
logoscore -D -m ./runtime/modules --persistence-path ./runtime/data

logoscore load-module lez_core     # dependency first
logoscore load-module amm_module
```

Every other op reads on-chain through the wallet module's `get_account_public`,
which fails on a null wallet handle (surfacing as an absent pool), so open the
wallet first — `resolvePoolAccount` then works:

```bash
wallet_dir="${LEE_WALLET_HOME_DIR:-$HOME/.lee/wallet}"
read -rs -p 'Wallet password (empty for unencrypted storage): ' wallet_password
echo
logoscore call lez_core open "$wallet_dir/wallet_config.json" \
  "$wallet_dir/storage.json" "$wallet_password" true
unset wallet_password
logoscore call amm_module resolvePoolAccount <defA_hex> <defB_hex>
```

The final `true` tells `lez_core` to connect to the RPC sequencer in the wallet
config. `swapExactInput` reuses that open wallet but additionally needs it **synced**
(nothing opens/syncs it for you headlessly). Note the amount/deadline
conventions above — small amount bare, deadline (and any big amount) quoted:

```bash
logoscore call amm_module swapExactInput \
  <defA_hex> <defB_hex> <inputHolding_hex> <outputHolding_hex> \
  1000 1 '"32503680000000"' <feePayer_hex>
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
wallet's synced state may refer to the previous chain. For a fresh test chain,
use a wallet initialized for that chain and call `sync_to_block <height>`
before submitting. `resolvePoolAccount` is a **live** sequencer read, so the stale cursor
doesn't affect it — but it still needs the wallet **open**: the read goes through the
wallet's sequencer connection (not its private keys), which only exists once the
wallet is opened.

## Install into Basecamp

`amm_module` is a **core** module (installed into `modules/`, alongside the
wallet module), not a UI plugin. Build its `.lgx` from the root flake and
install with `lgpm --modules-dir …` (see `apps/amm/README.md` for the full
three-package flow: `lez_core` + `amm_module` + the `amm_ui` UI
plugin).

## QtRO / byte-string note

`send_generic_public_transaction` takes `message` as a byte string
(`std::vector<uint8_t>`). This module passes the Borsh bytes directly, together
with a root mention index and the deployed program ID for each actor mention.
It requires the actor-aware wallet pinned by the module flake (`lez_core` 0.6.0).

## LEZ 0.4 actor interface

The module uses LEZ `v0.4.0-rc3` and the actor-aware `lez_core` 0.6.0 wallet
interface. Existing app operations keep their names. Rust transaction plans now
return `message` (Borsh `amm_core::Message` bytes), `rootMention`, `accountIds`,
`actorProgramIds`, and `signingRequirements`. The three arrays are aligned;
account IDs may repeat for different programs on the same account. Each mention
of a signing account carries the same signing requirement. The wallet
receives all actor mentions, including token recipients needed for immediate
public settlement. Creation routes to the config actor, pool operations to the
pool actor, and administrative operations to the signed authority's AMM actor.

Account reads use `{ nonce, actor_states: [{ program, data }] }`. The wallet returns
the nonce as a decimal string and actor program IDs and data as hex strings.
An `AccountRead` also carries `programId` to select the AMM actor for pool
resolution and fee quotes. Token lookup selects the
configured token deployment independently, so unrelated actor states on the same
account do not hide its holdings. Pool snapshots contain the tagged
`AmmState::Pool` encoding, not an untagged `PoolDefinition`.

Pool resolution and liquidity quotes use the current supply from the LP token
definition. Direct `amm_resolve_pool` callers must supply this account read as
`lpDefinition` for an existing pool. A missing or unreadable definition returns
`lp_supply_unavailable`. Quotes can become stale before execution.

The AMM program still uses the recording Oracle mock contract. Oracle setup
messages target the pool's Oracle actor; they do not create legacy per-window
Oracle PDAs or provide production TWAP prices. This port supports public wallet
transactions. Private transaction planning is outside this module's scope.
