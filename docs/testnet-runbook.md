# AMM on testnet — spel runbook

End-to-end steps to deploy the programs, initialize the AMM, and create a pool using
`spel`, in the order they must happen. Follow top to bottom; nothing here can be skipped.

> **Golden rule:** every time you **recompile** a guest, its **ProgramId changes**, and
> **every PDA derived from that ProgramId changes too** (config, pool, vaults, LP def, lp
> lock, current tick). If you rebuild the AMM, you must recompute all AMM PDAs and
> re-run `initialize`. Treat ProgramIds and PDAs as deployment-specific — re-derive them,
> never reuse old values.

---

## Contents

- [0. Prerequisites](#0-prerequisites)
  - [Argument formats](#argument-formats-used-throughout)
- [1. Build & deploy the programs](#1-build--deploy-the-programs)
  - [Record the ProgramIds](#record-the-programids)
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
- [Gotchas](#gotchas-we-hit-and-how-to-avoid-them)

---

## 0. Prerequisites

- **Docker running** (guest builds cross-compile through it).
- **`spel` / `wallet`** built from the
  [`refactor/lez-v020-compat`](https://github.com/0x-r4bbit/spel/tree/refactor/lez-v020-compat)
  branch (`github.com/0x-r4bbit/spel`). This is required for: the `program-id` command, the
  `--` subcommand separator, the public-tx signing scheme, and the `account_id` argument
  fix.
- **Wallet home** exported in every shell you use (deploy *and* spel must point at the
  same wallet/network):
  ```bash
  export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
  ```
- Wallet configured to reach your sequencer (`wallet_config.json`), with the accounts you
  need created (`wallet account ...`).
  ```bash
  wallet config set sequencer_addr https://testnet.lez.logos.co/
  ```

### Argument formats (used throughout)

| Kind | Accepted forms |
|---|---|
| **account id** (PDAs, holdings, authority) | base58 (e.g. `9qbX…`) **or** `0x`-prefixed 32-byte hex. **No** `account_id( … )` wrapper. |
| **program id** | 8 comma-separated u32 limbs, a bare 64-char ImageID hex, a `0x`-prefixed ImageID hex, **or** a base58 ImageID. (`spel program-id` prints the limbs and the bare hex; base58 is computed by the `*_pdas` helpers / yourself.) |

---

## 1. Build & deploy the programs

**Note: Only do this if you want to deploy the protocols yourself!**

Build and deploy **token**, **twap_oracle**, and **amm** (order doesn't matter for deploy,
but you need all three before initializing the AMM):

```bash
# repeat for: token, twap_oracle, amm
cargo risczero build --manifest-path programs/<prog>/methods/guest/Cargo.toml
wallet deploy-program programs/<prog>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<prog>.bin
```

Binary path convention: `programs/<prog>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<prog>.bin`

### Record the ProgramIds

```bash
spel -- program-id programs/<prog>/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/<prog>.bin
```

Record each ProgramId in whatever form you'll pass it — the forms are interchangeable
(see the arg-format table above). `spel program-id` prints both the **decimal limbs** and
the **64-char ImageID hex**; both are accepted by spel and the `*_pdas` helpers, and the
ImageID hex is usually the easiest to copy. **base58 is also accepted as input**, but
nothing in the toolchain prints it — base58-encode the 32 ImageID bytes yourself if you
want that form. Fill in:

| program | ProgramId (limbs, ImageID hex, or base58) |
|---|---|
| token | `<TOKEN_PROGRAM_ID>` |
| twap_oracle | `<TWAP_PROGRAM_ID>` |
| amm | `<AMM_PROGRAM_ID>` |

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
     --program programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin \
     -- new-fungible-definition \
     --name "TOKEN A" --total-supply 1000000000000000000000 \
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
     --program programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin \
     -- initialize-account \
     --definition-account <DEF> \
     --account-to-initialize <NEW_HOLDING>
```

Then transfer (`amount-to-transfer` is in **base units** — e.g. `1000000000000000000` = one
whole token at 18 decimals):

```bash
spel --idl artifacts/token-idl.json \
     --program programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin \
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
`*_core` functions, guaranteeing they match the guest. They take ProgramIds as comma-
separated u32 limbs, a 64-char ImageID hex, or a base58 ImageID (any form `spel program-id`
prints, plus base58), and account ids as base58.

```bash
# config only (for step 5):
cargo run -q -p amm_program --example amm_pdas -- "<AMM_PROGRAM_ID>"

# config + all pool PDAs (for step 6):
cargo run -q -p amm_program --example amm_pdas -- \
  "<AMM_PROGRAM_ID>" "<TWAP_PROGRAM_ID>" "<DEF_A>" "<DEF_B>"
```

The fixed **clock** account never changes: `4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU`
(`/LEZ/ClockProgramAccount/0000001`).

### All PDA helpers

| program | command | prints |
|---|---|---|
| amm | `cargo run -q -p amm_program --example amm_pdas -- <amm_pid> [<twap_pid> <defA> <defB>]` | config; + pool, vault_a/b, pool_definition_lp, lp_lock_holding, current_tick_account |
| twap_oracle | `cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- <oracle_pid> <price_source> [<window_duration>]` | current_tick_account; + price_observations, oracle_price_account (with a window) |
| ata | `cargo run -q -p ata_program --example ata_pdas -- <ata_pid> <token_pid> <owner> <definition>` | the ATA address |
| stablecoin | `cargo run -q -p stablecoin_program --example stablecoin_pdas -- <stablecoin_pid> <owner> <collateral_definition>` | position, position_vault |

(The token program has no PDAs.)

## 5. Initialize the AMM

Pick an `<AUTHORITY>` account you control (the admin who may later `update_config`).
`config` is the PDA from step 4; `token`/`twap` are the ProgramIds from step 1.

```bash
spel --idl artifacts/amm-idl.json \
     --program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
     -- initialize \
     --config <CONFIG_PDA> \
     --token-program-id <TOKEN_PROGRAM_ID> \
     --twap-oracle-program-id <TWAP_PROGRAM_ID> \
     --authority <AUTHORITY>
```

> `config` is `init` but **not** a signer — spel just lists it; the guest claims it as a PDA.
> Run once per deployment. (Add `--dry-run` to preview.)

## 6. Create a pool (`new-definition`)

Provide three holding accounts you own:
- `<USER_HOLDING_A>` / `<USER_HOLDING_B>` — your holdings of token A / B (**signers**;
  must hold ≥ the deposit amounts).
- `<USER_HOLDING_LP>` — a **fresh wallet account whose key you hold** (now a **signer**;
  it must be a normal keypair account, **not** an ATA). Receives the LP tokens.

Amounts must satisfy `isqrt(token_a_amount * token_b_amount) > 1000` (MINIMUM_LIQUIDITY).
`fees` ∈ {1, 5, 30, 100} (bps). `deadline` is a future ms timestamp (or `u64::MAX` =
`18446744073709551615` to ignore).

```bash
spel --idl artifacts/amm-idl.json \
     --program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
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
     --fees 1 \
     --deadline 18446744073709551615
```

> spel signs `user_holding_a`, `user_holding_b`, and `user_holding_lp` (the IDL `signer`
> accounts) — your wallet must hold all three keys. No `--bin-*` is needed: the chained
> token/oracle calls are resolved on-chain from the ProgramIds stored in `config`.
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
     --program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
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
> submits an empty witness set. Prereqs: the pool and its `current_tick_account` must
> already exist (the latter was created during `new_definition`); the oracle seeds the first
> observation from `current_tick_account`, so the price can't be forged. No `--bin-*` needed
> (the TWAP ProgramId is read from `config`). The `price_observations` PDA must not already
> exist (`init`).

Verify:
```bash
spel --idl artifacts/twap_oracle-idl.json inspect <PRICE_OBSERVATIONS_PDA> --type PriceObservations
```

## 9. Swap

Swap exact input → output. Uses the pool PDAs from step 4 plus your two token holdings.
`--token-definition-id-in` picks the direction (pass the definition id of the token you're
spending).

```bash
spel --idl artifacts/amm-idl.json \
     --program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
     -- swap-exact-input \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --vault-a <VAULT_A_PDA> \
     --vault-b <VAULT_B_PDA> \
     --user-holding-a <USER_HOLDING_A> \
     --user-holding-b <USER_HOLDING_B> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --swap-amount-in <AMOUNT_IN> \
     --min-amount-out <MIN_OUT> \
     --token-definition-id-in <DEF_OF_INPUT_TOKEN> \
     --deadline 18446744073709551615
```

- **`--token-definition-id-in`**: `<DEF_A>` ⇒ A→B; `<DEF_B>` ⇒ B→A.
- **`--swap-amount-in`** must be ≤ the input holding's balance; **`--min-amount-out`** is the
  slippage floor (`1` accepts any nonzero output).
- spel signs **both** `user_holding_a` and `user_holding_b` (both are `signer` in the IDL —
  the input side is dynamic, so both are marked; see the gotcha below). Pass both even though
  only the input side is debited.
- No `--bin-*` needed (token program id read from `config`). `swap-exact-output` is the same
  account set with `--exact-amount-out` / `--max-amount-in` instead.

Verify (input reserve up by the input, output reserve down by the output):
```bash
spel --idl artifacts/amm-idl.json inspect <POOL_PDA> --type PoolDefinition
```

## 10. Record a tick

`record_tick` appends an observation to a window's `price_observations` account from the
current tick. It's a **direct TWAP oracle call** (not AMM-chained), so it uses the
twap_oracle binary + IDL. **No signers** — it's permissionless: it reads
`current_tick_account` + `clock` and writes `price_observations`.

Run a swap (step 9) first so `current_tick_account` holds a fresh tick, then:

```bash
spel --idl artifacts/twap_oracle-idl.json \
     --program programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin \
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
     --program programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin \
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
     --program programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin \
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

## Gotchas we hit (and how to avoid them)

- **Recompile ⇒ new ProgramId ⇒ new PDAs.** Redo steps 4–6 after any AMM rebuild. The
  config/pool addresses are functions of the AMM ProgramId.
- **`account_id` args must be bare base58/`0x`-hex** — strip the wallet's `account_id( … )`
  display wrapper.
- **`spel pda` does NOT match these PDAs.** It uses a padded-bytes seed scheme; the
  `*_core` crates hash with SHA-256. Always use the committed `examples/*_pdas` helpers
  (step 4).
- **`user_holding_lp` is a signer** for `new_definition` (its LP-token definition is created
  in the same tx, so the holding must be a fresh signed keypair, not an ATA). For
  `add_liquidity` the LP definition already exists, so an existing/pre-created holding works
  without a signature.
- **Both swap holdings are signers.** `swap_exact_input`/`swap_exact_output` mark *both*
  `user_holding_a` and `user_holding_b` `signer`, because the input side (a token `Transfer`
  sender, which must be authorized) is chosen at runtime via `--token-definition-id-in`. So
  pass and sign both even though only the input is debited. (Re-creating a pool also needs a
  **fresh** `user_holding_lp` — the previous one is bound to the old pool's LP token.)
- **Same `LEE_WALLET_HOME_DIR` everywhere.** Deploying with one wallet home and running
  spel with another points them at different networks/keys.
