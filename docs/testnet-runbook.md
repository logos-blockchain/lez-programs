# AMM on testnet — spel runbook

End-to-end steps to deploy the programs, initialize the AMM, and create a pool using
`spel`, in the order they must happen. Follow top to bottom; nothing here can be skipped.

> **LEZ version.** This runbook targets **LEZ v0.2.5** (programs deployed through the
> `program_loader`, fees on every transaction). The live `testnet-legacy` sequencer in
> [DEPLOYMENTS.md](../DEPLOYMENTS.md) still runs **v0.2.4**, and its addresses are
> ImageID-based; for that network use the v0.2.4 runbook from git history
> (`git log -- docs/testnet-runbook.md`).

> **Golden rule:** a program's id is the **header account** it was deployed to, and
> **every PDA is derived from that id** (config, pool, vaults, LP def, lp lock, current
> tick, …). Recompiling a guest does not change the id of an already-deployed program, but
> every **redeploy** goes to a fresh header — a new id, so new PDAs. After a redeploy,
> recompute that program's PDAs and re-run its initialization (e.g. the AMM's
> `initialize`). Treat program ids and PDAs as deployment-specific — never reuse old values.

---

## Contents

- [0. Prerequisites](#0-prerequisites)
  - [Argument formats](#argument-formats-used-throughout)
  - [Fees](#fees-used-throughout)
- [1. Build & deploy the programs](#1-build--deploy-the-programs)
  - [Record the program ids](#record-the-program-ids)
- [2. Wallet CLI basics](#2-wallet-cli-basics)
- [3. Create two token definitions](#3-create-two-token-definitions)
  - [(Optional) Transfer tokens](#optional-transfer-tokens)
- [4. Derive the AMM PDAs](#4-derive-the-amm-pdas)
  - [All PDA helpers](#all-pda-helpers)
- [5. Initialize the AMM](#5-initialize-the-amm)
- [6. Create a pool (new-definition)](#6-create-a-pool-new-definition)
- [7. Verify](#7-verify)
- [8. (Optional) Create a TWAP price-observations account](#8-optional-create-a-twap-price-observations-account)
- [9. Swap](#9-swap)
- [10. Record a tick](#10-record-a-tick)
- [11. Create the oracle price account](#11-create-the-oracle-price-account)
- [12. Publish a price](#12-publish-a-price)
- [13. (Admin) Withdraw protocol fees](#13-admin-withdraw-protocol-fees)
- [14. Faucet: mint additional tokens](#14-faucet-mint-additional-tokens-token-mint-authority)
- [15. Stablecoin](#15-stablecoin)
- [Gotchas](#gotchas-we-hit-and-how-to-avoid-them)

---

## 0. Prerequisites

- **Docker running** (guest builds cross-compile through it).
- **`spel` / `wallet`** built from [`logos-co/spel`](https://github.com/logos-co/spel) at
  the revision the guests pin (`spel-framework` `rev` in
  `programs/*/methods/guest/Cargo.toml`), so the CLI matches the LEZ version the programs
  are built against.
- **Wallet home** exported in every shell you use (deploy *and* spel must point at the
  same wallet/network):
  ```bash
  export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
  ```
- Wallet configured to reach your sequencer, with the accounts you need created
  (`wallet account ...`). The sequencer lives in the `sequencers` list of
  `$LEE_WALLET_HOME_DIR/wallet_config.json` (the old top-level `sequencer_addr` key is
  gone, so `wallet config set sequencer_addr …` no longer applies):
  ```json
  "sequencers": [
    { "sequencer_addr": "https://testnet-legacy.lez.logos.co/" }
  ]
  ```
  Use a dedicated wallet home per network: synced block state from one sequencer does not
  carry over to another.
- A **funded public account** to pay fees, `<FEE_PAYER>` below. Every transaction is
  charged, and the sequencer reserves the worst-case fee up front (at the wallet's default
  2,000,000 gas limit and the minimum base fees that is ~16,000,008 units), so the payer
  needs at least that much on top of what it spends.

### Argument formats (used throughout)

| Kind | Accepted forms |
|---|---|
| **account id** (PDAs, holdings, authority) | base58 (e.g. `9qbX…`) **or** `0x`-prefixed 32-byte hex. **No** `account_id( … )` wrapper. |
| **program id** | The base58 id of the program's **header account** (step 1) — an ordinary account id. `spel --program` rejects a binary path: the id cannot be derived from the binary. |
| **wallet account** (`wallet … --header/--segments/--payer`, `wallet account get -a`) | A label, or `Public/<base58-id>`. A bare id is taken as a label. |

### Fees (used throughout)

Every `spel` transaction below passes `--fee-payer <FEE_PAYER>`. Without it, spel makes the
**first signer** pay — which fails whenever the signers are freshly created accounts that
hold nothing (definition targets, holdings, PDAs-to-be). Instructions with **no** signers
(e.g. `create-price-observations`, `record-tick`, `publish-price`) need `--fee-payer`, or
spel refuses to build them. The fee payer joins the transaction's signers, so your wallet
must hold its key.

---

## 1. Build & deploy the programs

**Note: Only do this if you want to deploy the protocols yourself!**

Build and deploy **token**, **twap_oracle**, and **amm** (order doesn't matter for deploy,
but you need all three before initializing the AMM):

```bash
# repeat for: token, twap_oracle, amm
cargo risczero build --manifest-path programs/<prog>/methods/guest/Cargo.toml
```

Binary path convention: `programs/<prog>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<prog>.bin`

A program is deployed through the `program_loader`: the binary is uploaded as a chain of
write-once **segment** accounts (one per 96 KiB chunk), and a **header** account points at
that chain. The wallet does not create these accounts itself, so create them first — all
fresh. Header and segments start empty, so `<FEE_PAYER>` pays every deploy transaction:

```bash
PROG=token   # repeat for: token, twap_oracle, amm
BIN=programs/$PROG/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/$PROG.bin
N=$(( ($(wc -c < "$BIN") + 98303) / 98304 ))   # 96 KiB segments; the count must match exactly

wallet account new public --label "$PROG-header"
for i in $(seq 1 "$N"); do wallet account new public --label "$PROG-seg-$i"; done

wallet program-loader deploy \
  --elf "$BIN" \
  --header "$PROG-header" \
  --segments $(for i in $(seq 1 "$N"); do printf '%s-seg-%s ' "$PROG" "$i"; done) \
  --payer <FEE_PAYER>
```

- `--header`, `--segments` and `--payer` take a wallet **label** or `Public/<base58-id>`; a
  bare id is mistaken for a label.
- Segments are write-once: if a deploy fails partway, use **fresh** segment accounts (and a
  fresh header) for the next attempt.
- `--immutable` makes the program self-declare as immutable (not protocol-enforced).
- `apps/amm/tests/testnet/setup-amm-testnet.sh` scripts this flow for every program.

### Record the program ids

The program's id is its **header account's** id:

```bash
wallet account id --account-id "$PROG-header"
```

It is a plain base58 account id: pass it to `spel --program`, to the `*_pdas` helpers, and
wherever an instruction takes another program's id (e.g. the AMM's `--token-program-id`).
Fill in:

| program | program id (base58 header account id) |
|---|---|
| token | `<TOKEN_PROGRAM_ID>` |
| twap_oracle | `<TWAP_PROGRAM_ID>` |
| amm | `<AMM_PROGRAM_ID>` |

(`spel -- program-id <bin>` still prints a binary's ImageID, which identifies the code but
is no longer an address.)

## 2. Wallet CLI basics

The `wallet` CLI manages the accounts you'll pass to `spel` (definition targets, holdings,
authority, LP holding). Every command needs `LEE_WALLET_HOME_DIR` set (see Prerequisites).

Create a **public** account, giving it a `--label` you can recognize later:

```bash
wallet account new public --label token-a-def
```

List the accounts the wallet owns (`ls` is an alias; add `-l`/`--long` for full details):

```bash
wallet account list
```

The listing shows each account's label and base58 id. Use those ids as the `<DEF_*>`,
`<HOLDING_*>`, `<AUTHORITY>`, and `<USER_HOLDING_LP>` arguments in the steps below.

## 3. Create two token definitions

You need **two different** fungible tokens. For each, create two wallet accounts:

```bash
wallet account new public --label "Token A Definition"
wallet account new public --label "Token A Holding"
wallet account new public --label "Token B Definition"
wallet account new public --label "Token B Holding"
```

Confirm with (your addresses will be different):

```bash
wallet account list

/1 Public/4T69U868K6UzX8zbesU5wyr36gxaU7wb91Q45yedP4Rb [Token A Holding]
/0 Public/CER21z16YgmWr3aN8FEHsrmfm2iRfQiwZTac3FQa21US [Token A Definition]
/0/0 Public/EW2eoxcQyRDffrr94LkmuyByhaXx8emNzfDSAp9q29m5 [Token B Definition]
/2 Public/EcWWrBekMaER4JRAzzP4rpB8TFD2eHvyYJxScQPwzmpE [Token B Holding]
```

Next, create the token definitions and holdings:

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     --fee-payer <FEE_PAYER> \
     -- new-fungible-definition \
     --name "Token A" --total-supply 1000000000000000000000 \
     --definition-target-account <DEF_A> \
     --holding-target-account <HOLDING_A>
```

- `<DEF_A>` becomes the token-definition account; `<HOLDING_A>` receives `total_supply`.
- Repeat for token B (`<DEF_B>`, `<HOLDING_B>`).
- These `<DEF_*>` ids are what feed all the AMM pool PDAs.

Inspect to confirm / find a holding's definition id:

```bash
spel --idl artifacts/token-idl.json inspect <HOLDING_A> --type TokenHolding
spel --idl artifacts/token-idl.json inspect <DEF_A>     --type TokenDefinition
```

### (Optional) Transfer tokens

Move fungible tokens between holdings with the **token** program's `transfer`. Standalone —
usable any time, independent of the AMM. spel signs the **sender** (you must own its key) and
credits the **recipient**; both must be holdings of the **same** token definition.

The recipient must be an **already-initialized** holding of that token. (`transfer` *can*
credit an empty account by claiming a fresh holding, but that claim needs the recipient's
signature, which `transfer` doesn't collect — so initialize the holding first.) Create one
with `initialize-account` (the new holding is a **signer** — your wallet must hold its key):

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     --fee-payer <FEE_PAYER> \
     -- initialize-account \
     --definition-account <DEF> \
     --account-to-initialize <NEW_HOLDING>
```

Then transfer (`amount-to-transfer` is in **base units** — e.g. `1000000000000000000` = one
whole token at 18 decimals):

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     --fee-payer <FEE_PAYER> \
     -- transfer \
     --sender <SENDER_HOLDING> \
     --recipient <RECIPIENT_HOLDING> \
     --amount-to-transfer <AMOUNT>
```

- `<SENDER_HOLDING>` — a holding you own (spel signs it; balance must be ≥ `<AMOUNT>`).
- `<RECIPIENT_HOLDING>` — an existing holding of the **same** `<DEF>` (initialize it first, above).
- Both holdings must share the same definition, or the transfer aborts with
  `Sender and recipient definition id mismatch`.

Verify the recipient's new balance:
```bash
spel --idl artifacts/token-idl.json inspect <RECIPIENT_HOLDING> --type TokenHolding
```

## 4. Derive the AMM PDAs

Program PDAs use a **SHA-256 seed scheme** (in each `*_core`), which is **not** what
`spel pda` computes (that pads raw bytes) — so `spel pda` will give wrong addresses. Each
program ships a committed `examples/*_pdas` helper that derives its PDAs with its own
`*_core` functions, guaranteeing they match the guest. They take program ids (header
account ids, step 1) and account ids, all as base58.

AMM instances are namespaced by `(owner, nonce)`, so pick the `<OWNER>` account that will
sign `initialize` in step 5 now — the config PDA (and every pool PDA under it) depends on
it. The helper derives the default instance (all-zero nonce).

```bash
# config only (for step 5):
cargo run -q -p amm_program --example amm_pdas -- "<AMM_PROGRAM_ID>" "<OWNER>"

# config + all pool PDAs (for step 6):
cargo run -q -p amm_program --example amm_pdas -- \
  "<AMM_PROGRAM_ID>" "<OWNER>" "<TWAP_PROGRAM_ID>" "<DEF_A>" "<DEF_B>"
```

The fixed **clock** account never changes: `4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU`
(`/LEZ/ClockProgramAccount/0000001`).

### All PDA helpers

| program | command | prints |
|---|---|---|
| amm | `cargo run -q -p amm_program --example amm_pdas -- <amm_pid> <owner> [<twap_pid> <defA> <defB>]` | config; + pool, vault_a/b, pool_definition_lp, lp_lock_holding, current_tick_account, protocol_fee_a/b |
| twap_oracle | `cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- <oracle_pid> <price_source> [<window_duration>]` | current_tick_account; + price_observations, oracle_price_account (with a window) |
| ata | `cargo run -q -p ata_program --example ata_pdas -- <ata_pid> <token_pid> <owner> <definition>` | the ATA address |
| stablecoin | `cargo run -q -p stablecoin_program --example stablecoin_pdas -- <stablecoin_pid> globals` | protocol_parameters, stability_fee_accumulator, redemption_price_state, stablecoin_definition, stablecoin_master_holding |
| stablecoin | `cargo run -q -p stablecoin_program --example stablecoin_pdas -- <stablecoin_pid> <owner> <position_nonce>` | position, position_vault |

(The token program has no PDAs.)

## 5. Initialize the AMM

Pick an `<OWNER>` account you control that **signs** `initialize` (its id + the
`<NONCE>` form the instance namespace; `[0;32]` / 64 zero hex is the default instance),
an `<AUTHORITY>` (the admin who may later `update_config` or withdraw protocol fees), the
instance-wide `<SWAP_FEE_BPS>` (basis points, any value below 10000 = 100%; e.g. `1`), and
`<PROTOCOL_FEE_BPS>` (the fraction *of that swap fee* diverted to the protocol, in bps of
the swap fee; `0` = none, up to `10000` = 100% of the swap fee). Both fees are set **once
here** and apply to every pool in the namespace — they are no longer per-pool values.
`config` is the PDA from step 4; `token`/`twap` are the program ids (header account ids)
from step 1.

```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- initialize \
     --owner <OWNER> \
     --config <CONFIG_PDA> \
     --nonce 0000000000000000000000000000000000000000000000000000000000000000 \
     --token-program-id <TOKEN_PROGRAM_ID> \
     --twap-oracle-program-id <TWAP_PROGRAM_ID> \
     --authority <AUTHORITY> \
     --swap-fee-bps <SWAP_FEE_BPS> \
     --protocol-fee-bps <PROTOCOL_FEE_BPS>
```

> `owner` signs (squat-proofs the namespace) and is claimed by the AMM on first use, so
> use a fresh dedicated account. `config` is `init` but **not** a signer — the guest claims
> it as a PDA. Run once per `(owner, nonce)` instance. (Add `--dry-run` to preview.)

## 6. Create a pool (`new-definition`)

Provide three holding accounts you own:
- `<USER_HOLDING_A>` / `<USER_HOLDING_B>` — your holdings of token A / B (**signers**;
  must hold ≥ the deposit amounts).
- `<USER_HOLDING_LP>` — a **fresh wallet account whose key you hold** (now a **signer**;
  it must be a normal keypair account, **not** an ATA). Receives the LP tokens.

Amounts must satisfy `isqrt(token_a_amount * token_b_amount) > 1000` (MINIMUM_LIQUIDITY).
`new-definition` takes **no fee argument** — the swap fee is instance-wide (set at
`initialize`). `deadline` is a future ms timestamp (or `u64::MAX` =
`18446744073709551615` to ignore).

```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- new-definition \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --vault-a <VAULT_A_PDA> \
     --vault-b <VAULT_B_PDA> \
     --pool-definition-lp <POOL_DEFINITION_LP_PDA> \
     --lp-lock-holding <LP_LOCK_HOLDING_PDA> \
     --user-holding-a <USER_HOLDING_A> \
     --user-holding-b <USER_HOLDING_B> \
     --user-holding-lp <USER_HOLDING_LP> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --token-a-amount <AMOUNT_A> \
     --token-b-amount <AMOUNT_B> \
     --deadline 18446744073709551615
```

> spel signs `user_holding_a`, `user_holding_b`, and `user_holding_lp` (the IDL `signer`
> accounts) — your wallet must hold all three keys. No `--bin-*` is needed: the chained
> token/oracle calls are resolved on-chain from the program ids stored in `config`.
> Pair `<DEF_A>` with `<USER_HOLDING_A>`/`<AMOUNT_A>` consistently (and B with B).

## 7. Verify

```bash
spel --idl artifacts/amm-idl.json inspect <POOL_PDA> --type PoolDefinition
```

Check `reserve_a`/`reserve_b`, `liquidity_pool_supply`, and `fees`.

## 8. (Optional) Create a TWAP price-observations account

A TWAP feed for the pool over a time window. `window_duration` is in **milliseconds**
(e.g. 24h = `86400000`). The observations account is a PDA of `(twap_pid, pool, window)`,
so each window has its own account.

Derive it (the `current_tick_account` is the same one from the pool):
```bash
cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- \
  "<TWAP_PROGRAM_ID>" <POOL_PDA> <WINDOW_DURATION>
# prints current_tick_account, price_observations, oracle_price_account
```

```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- create-price-observations \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --price-observations <PRICE_OBSERVATIONS_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --window-duration <WINDOW_DURATION>
```

> **No signers** — this instruction is permissionless: the AMM authorizes the new
> observations PDA via the pool's PDA seed in a chained call to the TWAP oracle, so spel
> submits no signature besides the fee payer's. Prereqs: the pool and its `current_tick_account` must
> already exist (the latter was created during `new_definition`); the oracle seeds the first
> observation from `current_tick_account`, so the price can't be forged. No `--bin-*` needed
> (the TWAP program id is read from `config`). The `price_observations` PDA must not already
> exist (`init`).

Verify:
```bash
spel --idl artifacts/twap_oracle-idl.json inspect <PRICE_OBSERVATIONS_PDA> --type PriceObservations
```

## 9. Swap

Swap exact input → output. Uses the pool PDAs from step 4 plus your two token holdings.
The direction comes from which holding you pass as input: the AMM reads the input
holding's token definition and debits that side of the pool.

```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- swap-exact-input \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --vault-a <VAULT_A_PDA> \
     --vault-b <VAULT_B_PDA> \
     --user-input-holding <HOLDING_OF_TOKEN_SPENT> \
     --user-output-holding <HOLDING_OF_TOKEN_RECEIVED> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --protocol-fee-holding <PROTOCOL_FEE_PDA_FOR_INPUT_TOKEN> \
     --swap-amount-in <AMOUNT_IN> \
     --min-amount-out <MIN_OUT> \
     --deadline 18446744073709551615
```

- **Direction**: input = `<USER_HOLDING_A>`, output = `<USER_HOLDING_B>` ⇒ A→B; swap them
  for B→A.
- **`--protocol-fee-holding`**: the protocol-fee PDA **for the input token** — `protocol_fee_a`
  when spending A, `protocol_fee_b` when spending B (from step 4's `amm_pdas` output). The swap
  diverts `protocol_fee_bps` of the fee here (creating it on first use); pass it even when the
  instance's protocol fee is `0` (the account is still required and simply left untouched).
- **`--swap-amount-in`** must be ≤ the input holding's balance; **`--min-amount-out`** is the
  slippage floor (`1` accepts any nonzero output).
- Only `user_input_holding` signs (it is the token `Transfer` sender). The output holding
  must already exist and hold the output token.
- No `--bin-*` needed (token program id read from `config`). `swap-exact-output` is the same
  account set with `--exact-amount-out` / `--max-amount-in` instead.

Verify (input reserve up by the input, output reserve down by the output):
```bash
spel --idl artifacts/amm-idl.json inspect <POOL_PDA> --type PoolDefinition
```

## 10. Record a tick

`record_tick` appends an observation to a window's `price_observations` account from the
current tick. It's a **direct TWAP oracle call** (not AMM-chained), so it uses the
twap_oracle program id + IDL. **No signers** — it's permissionless: it reads
`current_tick_account` + `clock` and writes `price_observations`.

Run a swap (step 9) first so `current_tick_account` holds a fresh tick, then:

```bash
spel --idl artifacts/twap_oracle-idl.json \
     --program <TWAP_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- record-tick \
     --price-observations <PRICE_OBSERVATIONS_PDA> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --price-source-id <POOL_PDA> \
     --window-duration <WINDOW_DURATION>
```

- `--price-source-id` is the **pool** (an `account_id` arg, not a listed account).
- `--window-duration` must **match** the value the `price_observations` account was created
  with (step 8) — it's part of that PDA's seeds, so a mismatch points at a different/missing
  account.
- Prereqs: the observations account exists (step 8) and `current_tick_account` holds a fresh
  tick (run step 9 first).

Verify:
```bash
spel --idl artifacts/twap_oracle-idl.json inspect <PRICE_OBSERVATIONS_PDA> --type PriceObservations
```

## 11. Create the oracle price account

`publish_price` (step 12) writes into an `oracle_price_account` that must **already exist**,
so create it first. The oracle's own `create_oracle_price_account` requires the `price_source`
(the pool) to **sign**, which a PDA can't do — so the **AMM** creates it on the pool's behalf
via a chained, PDA-authorized call. **Permissionless** (no signers) and `init`, so run it
**once** per `(pool, window)`.

Derive `oracle_price_account` (same twap helper as step 8, with the window):

```bash
cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- \
  "<TWAP_PROGRAM_ID>" <POOL_PDA> <WINDOW_DURATION>
# prints current_tick_account, price_observations, oracle_price_account
```

```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- create-oracle-price-account \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --oracle-price-account <ORACLE_PRICE_ACCOUNT_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --window-duration <WINDOW_DURATION>
```

> `--window-duration` must match the observations account's window (step 8) — it seeds the
> `oracle_price_account` PDA. `init`, so skip this step if the account already exists.

## 12. Publish a price

Computes the TWAP from the observations ring buffer (extended to *now* using the current
tick) and writes it to the `oracle_price_account`. **Direct TWAP oracle call, permissionless.**

> **Needs at least two observations.** With fewer than two, `publish_price` is a silent
> **no-op** — it returns every account unchanged and writes nothing. The first observation is
> seeded when you create the observations account (step 8); the second comes from `record_tick`
> (step 10). That's why we swap (step 9) to move the tick and then record it before publishing.

```bash
spel --idl artifacts/twap_oracle-idl.json \
     --program <TWAP_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- publish-price \
     --price-observations <PRICE_OBSERVATIONS_PDA> \
     --oracle-price-account <ORACLE_PRICE_ACCOUNT_PDA> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --price-source-id <POOL_PDA> \
     --window-duration <WINDOW_DURATION>
```

- `--price-source-id` is the **pool** (an `account_id` arg, not a listed account).
- Re-runnable any time to refresh the published TWAP (unlike step 11's one-time `init`).
- `--window-duration` must match steps 8 and 11 — it seeds all three related PDAs.

Verify (check `price` and the publication timestamp):
```bash
spel --idl artifacts/twap_oracle-idl.json inspect <ORACLE_PRICE_ACCOUNT_PDA> --type OraclePriceAccount
```

---

## 13. (Admin) Withdraw protocol fees

If the instance was initialized with `--protocol-fee-bps > 0`, each swap diverts that fraction
of the swap fee (in the **input** token) into the per-`(config, token)` protocol-fee PDA
(`protocol_fee_a` / `protocol_fee_b` from step 4). The config's **`authority`** (set at
`initialize`) is the only account that can move them out, via `withdraw-protocol-fees`.

Inspect the accrued balance first (it is an ordinary token holding):
```bash
spel --idl artifacts/token-idl.json inspect <PROTOCOL_FEE_PDA> --type TokenHolding
```

Withdraw `<AMOUNT>` (raw base units) of that token to `<DESTINATION>` — an **already-initialized**
holding of the **same** token:
```bash
spel --idl artifacts/amm-idl.json \
     --program <AMM_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- withdraw-protocol-fees \
     --config <CONFIG_PDA> \
     --protocol-fee-holding <PROTOCOL_FEE_PDA> \
     --destination <DESTINATION> \
     --authority <AUTHORITY> \
     --amount <AMOUNT>
```

- `authority` **signs** and must equal the config's stored `authority`; the wallet must control it.
- `protocol-fee-holding` must be the protocol-fee PDA for the token being withdrawn (its own token
  definition selects the token), so one call drains one token — repeat per token.
- `destination` must already exist (a credit needs no signature, but a fresh holding's claim would);
  initialize it first with `initialize-account` (step 3) if needed.

See [AMM protocol fees](./amm-protocol-fees.md) for the full model.

---

## 14. Faucet: mint additional tokens (`token-mint-authority`)

The **token-mint-authority** program is a permissionless faucet: once a token's
`mint_authority` is set to the faucet's singleton PDA, anyone can mint a fixed amount of
that token to themselves — rate-limited to **once per 24h per `(recipient, token
definition)`**. Use it to top up a test token after the initial supply is spent.

> The steps below are the generic flow. [DEPLOYMENTS.md](../DEPLOYMENTS.md) lists the
> faucet on the v0.2.4 `testnet-legacy` network, whose ImageID-based ids do not carry over to
> a v0.2.5 deployment.

### 14.1 Deploy the faucet + derive its PDA

Build and deploy it like the other programs (step 1, with `PROG=token_mint_authority`);
its header account id is `<FAUCET_PROGRAM_ID>`. Then derive its mint-authority PDA from
that id:

```bash
cargo run -q -p token_mint_authority_program --example mint_authority -- <FAUCET_PROGRAM_ID>
# prints the base58 <FAUCET_PDA>
```

The PDA depends only on the program id, so it changes when the faucet is redeployed (new
header), not when it is rebuilt. Set the tokens' `mint_authority` to the PDA of the faucet
you actually call; a mismatch makes every FaucetMint fail.

### 14.2 Make a token faucet-mintable (at creation)

A token is only faucet-mintable if its `mint_authority` was set to `<FAUCET_PDA>` when it
was created (§3). Add `--mint-authority` to `new-fungible-definition`:

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- new-fungible-definition \
     --name "Token A" --total-supply 1000000000000000000000 \
     --definition-target-account <DEF_A> \
     --holding-target-account <HOLDING_A> \
     --mint-authority <FAUCET_PDA>
```

The initial supply is still minted to `<HOLDING_A>` at creation; the `mint_authority` only
governs who can mint **more** later. (The live testnet tokens were already created this way
— see DEPLOYMENTS.md.)

### 14.3 FaucetMint

`FaucetMint` takes 6 accounts (no value args):

| account | what |
|---|---|
| `recipient` | signs; the rate-limit subject. A plain account you control, **different** from `user_holding`. |
| `mint_allowance` | the per-`(recipient, definition)` PDA (derived below). |
| `user_holding` | recipient's Token Holding for the faucet token — the mint target. Must already be **initialized** (`initialize-account`, §3); the faucet writes into an existing holding, it won't create one. |
| `token_definition` | the faucet token's `<DEF>`. |
| `mint_authority` | `<FAUCET_PDA>`. |
| `clock` | the canonical system clock `4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU`. |

`<FAUCET_PROGRAM_ID>` / `<TOKEN_PROGRAM_ID>` below are header account ids (step 1); no
binary is needed. Derive the per-`(recipient, definition)` `mint_allowance` PDA:

```bash
cargo run -q -p token_mint_authority_program --example faucet_allowance -- \
  <FAUCET_PROGRAM_ID> <RECIPIENT> <DEF_A>
# prints the base58 <MINT_ALLOWANCE>
```

If the recipient's holding is fresh, initialize it first (the faucet won't):

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- initialize-account \
     --definition-account <DEF_A> \
     --account-to-initialize <USER_HOLDING>
```

Then mint:

```bash
spel --idl artifacts/token_mint_authority-idl.json \
     --program <FAUCET_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- faucet-mint \
     --recipient <RECIPIENT> \
     --mint-allowance <MINT_ALLOWANCE> \
     --user-holding <USER_HOLDING> \
     --token-definition <DEF_A> \
     --mint-authority <FAUCET_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU
```

Mints a fixed **10,000e18** into `<USER_HOLDING>`. A second mint for the same
`(recipient, definition)` within 24h is rejected (`FaucetMint cooldown has not elapsed`) —
use a **different** recipient to mint again sooner. Verify:

```bash
spel --idl artifacts/token-idl.json inspect <USER_HOLDING> --type TokenHolding
```

> The faucet mints into any **existing** holding without an ownership check, so `recipient`
> is only the signer / rate-limit subject — it does not need to own `user_holding`. It must
> just be a distinct, authorized account. A dedicated throwaway `recipient` keeps the
> per-recipient cooldown bookkeeping clean.

## 15. Stablecoin

**One command:** `scripts/deploy-stablecoin.sh` does all of the steps below (options in its
header):

```bash
STABLECOIN_ADMIN=<ADMIN> STABLECOIN_ORACLE_SOURCE=<ORACLE_SOURCE> \
  COLLATERAL_DEFINITION=<COLLATERAL_DEF> COLLATERAL_HOLDING=<COLLATERAL_HOLDING> \
  STABLECOIN_MINIMUM_DEBT_AMOUNT=<MINIMUM_DEBT_ATOMIC_UNITS> \
  STABLECOIN_DEBT_CEILING=<DEBT_CEILING_ATOMIC_UNITS> \
  scripts/deploy-stablecoin.sh
```

- `COLLATERAL_HOLDING` set: creates the collateral token (or reuses it if it already
  exists). Omit it to use an existing collateral token.
- `TOKEN_PROGRAM_ID` / `TWAP_PROGRAM_ID` / `STABLECOIN_PROGRAM_ID`: reuse programs that
  are already deployed (their header account ids) instead of deploying them.
- `FEE_PAYER`: the funded account that pays every fee; defaults to `STABLECOIN_ADMIN`.
- Both debt limits are required operator choices: exact unsigned `u128` decimals
  in raw stablecoin atomic units, with `0 < minimum <= ceiling`. They have no defaults.
- Writes every id and PDA, schema version 2, and both debt limits to
  `target/stablecoin-deployment.json`.

The isolated AMM test setup (`apps/amm/tests/testnet/setup-amm-testnet.sh`) supplies
explicit smoke-test fixtures of 10 and 1,000,000 raw atomic units, respectively.
Override its `STABLECOIN_MINIMUM_DEBT_AMOUNT` and `STABLECOIN_DEBT_CEILING` for
other test scenarios; these fixture amounts are not production recommendations.

**Order matters.** `initialize_program` needs an oracle price account that already quotes
the stablecoin definition as its base asset — but `initialize_program` is what creates that
definition. Its address is a deterministic PDA, so derive it first.

### 15.1 Deploy and derive

Deploy `stablecoin` and `twap_oracle` (step 1), and have a fungible collateral token (step 3).
Every stablecoin PDA derives from the stablecoin's header account id, so a redeploy means
new PDAs and a fresh initialization. Schema version 2 rejects older global accounts
and initializer payloads; keep old deployments on their matching clients. Rebuild
the release binary and IDL and refresh module configuration with the new header ID. Then:

```bash
cargo run -q -p stablecoin_program --example stablecoin_pdas -- "<STABLECOIN_PROGRAM_ID>" globals
# prints protocol_parameters, stability_fee_accumulator, redemption_price_state,
#        stablecoin_definition, stablecoin_master_holding

cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- \
  "<TWAP_PROGRAM_ID>" <ORACLE_SOURCE> <WINDOW_DURATION>
# use oracle_price_account
```

`<ORACLE_SOURCE>` is a wallet account: it signs the next step and owns the price.

### 15.2 Create the market price oracle account

```bash
spel --idl artifacts/twap_oracle-idl.json \
     --program <TWAP_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- create-oracle-price-account \
     --oracle-price-account <ORACLE_PRICE_ACCOUNT_PDA> \
     --price-source <ORACLE_SOURCE> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --base-asset <STABLECOIN_DEFINITION_PDA> \
     --quote-asset <COLLATERAL_DEF> \
     --initial-price 1000000000000000000000000000 \
     --window-duration <WINDOW_DURATION>
```

> `--initial-price` is **27-decimal fixed point** (`10^27` = 1.0 collateral per stablecoin),
> the scale the stablecoin reads. `--window-duration` must be `>= 2048`.

### 15.3 Initialize

```bash
spel --idl artifacts/stablecoin-idl.json \
     --program <STABLECOIN_PROGRAM_ID> \
     --fee-payer <FEE_PAYER> \
     -- initialize-program \
     --admin <ADMIN> \
     --protocol-parameters <PROTOCOL_PARAMETERS_PDA> \
     --stability-fee-accumulator <STABILITY_FEE_ACCUMULATOR_PDA> \
     --redemption-price-state <REDEMPTION_PRICE_STATE_PDA> \
     --stablecoin-definition <STABLECOIN_DEFINITION_PDA> \
     --stablecoin-master-holding <STABLECOIN_MASTER_HOLDING_PDA> \
     --collateral-definition <COLLATERAL_DEF> \
     --market-price-oracle <ORACLE_PRICE_ACCOUNT_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --freeze-authority-account-id <FREEZE_AUTHORITY> \
     --initial-stability-fee-per-millisecond 1000000000001500000000000000 \
     --initial-controller-proportional-gain 1000000000 \
     --initial-controller-integral-gain 0 \
     --initial-minimum-collateralization-ratio 1500000000000000000000000000 \
     --minimum-milliseconds-between-rate-updates 60000 \
     --maximum-oracle-price-age-milliseconds 86400000 \
     --initial-redemption-price 1000000000000000000000000000 \
     --stablecoin-name "LEZ Stablecoin" \
     --minimum-debt-amount <MINIMUM_DEBT_ATOMIC_UNITS> \
     --debt-ceiling <DEBT_CEILING_ATOMIC_UNITS>
```

> Rates, ratios and prices are 27-decimal fixed point. The values above are the script's
> defaults: ~5%/year fee, 150% ratio, 1.0 redemption price. Debt limits use raw atomic
> units, not fixed point, and must be chosen explicitly. `init`, so run it **once**.
> The floor applies when normalized debt changes (zero debt is allowed). The ceiling
> admits new borrowing against aggregate accrued debt; interest may exceed it and
> repayment remains available subject to the floor.

---

## Gotchas we hit (and how to avoid them)

- **Redeploy ⇒ new program id ⇒ new PDAs.** Each deploy goes to a fresh header account, so
  redo steps 4–6 after redeploying the AMM: the config/pool addresses are functions of the
  AMM's program id. (A rebuild alone changes nothing until you deploy it.)
- **`--program` is a header account id, not a binary.** spel rejects a `.bin` path: since
  v0.2.5 the id is chosen at deploy time and cannot be derived from the binary.
- **Pass `--fee-payer`.** Otherwise the first signer pays, and freshly created signers hold
  nothing; instructions without signers fail outright. The payer's balance must also cover
  the up-front fee reserve (~16,000,008 at the wallet defaults), not just the fee charged.
- **Deploy accounts are single-use.** Segments are write-once and the header must be
  unclaimed: retry a failed `wallet program-loader deploy` with fresh accounts, and match the
  segment count to the binary size (one per 96 KiB).
- **"Confirmed" is not "succeeded".** spel prints `Transaction confirmed — included in a
  block` even when the program rejected the instruction and nothing changed. Check the
  result with `spel … inspect`.
- **`account_id` args must be bare base58/`0x`-hex** — strip the wallet's `account_id( … )`
  display wrapper.
- **`spel pda` does NOT match these PDAs.** It uses a padded-bytes seed scheme; the
  `*_core` crates hash with SHA-256. Always use the committed `examples/*_pdas` helpers
  (step 4).
- **`user_holding_lp` is a signer** for `new_definition` (its LP-token definition is created
  in the same tx, so the holding must be a fresh signed keypair, not an ATA). For
  `add_liquidity` the LP definition already exists, so an existing/pre-created holding works
  without a signature.
- **Swaps take input/output holdings, not A/B.** `swap_exact_input`/`swap_exact_output`
  take `user_input_holding` (the only signer) and `user_output_holding`; there is no
  `--user-holding-a/b` or `--token-definition-id-in` any more. Swap the two holdings to
  reverse the direction.
- **Re-creating a pool needs a fresh `user_holding_lp`** — the previous one is bound to the
  old pool's LP token.
- **Same `LEE_WALLET_HOME_DIR` everywhere.** Deploying with one wallet home and running
  spel with another points them at different networks/keys.
- **Stablecoin: oracle account before `initialize_program`.** Initialization rejects a
  missing oracle account, or one whose base asset isn't the stablecoin definition PDA.
- **Stablecoin: never `publish_price` into its oracle account.** The TWAP oracle writes
  Q64.64 (`2^64` = 1.0); the stablecoin reads 27-decimal fixed point (`10^27` = 1.0), so a
  published 1.0 reads as ~1.8e-8.
- **Stablecoin: the oracle price goes stale.** `generate_debt` rejects it once it is older
  than `maximum_oracle_price_age_milliseconds` (at most one day).
