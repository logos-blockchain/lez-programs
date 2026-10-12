# AMM and stablecoin on testnet — spel runbook

End-to-end steps to deploy the programs, initialize the AMM, and create a pool using
`spel`, in the order they must happen. Follow the AMM steps top to bottom. The
[stablecoin bootstrap](#15-stablecoin-bootstrap) can also run independently.

> **Live addresses.** The current testnet deployment's ProgramIds, PDAs, token
> definitions, and the faucet mint-authority PDA are recorded in
> [DEPLOYMENTS.md](../DEPLOYMENTS.md) — use those when interacting with the live
> instance instead of re-deriving.

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
- [13. (Admin) Withdraw protocol fees](#13-admin-withdraw-protocol-fees)
- [14. Faucet: mint additional tokens](#14-faucet-mint-additional-tokens-token-mint-authority)
- [15. Stablecoin bootstrap](#15-stablecoin-bootstrap)
- [16. Standalone sequencer module devnet](#16-standalone-sequencer-module-devnet)
- [Gotchas](#gotchas-we-hit-and-how-to-avoid-them)

---

## 0. Prerequisites

- **Docker running** (guest builds cross-compile through it).
- **`spel` / `wallet`** built from the revisions selected by this workspace:
  ```bash
  make setup-workspace-tools
  source scripts/workspace-env.sh
  ```
  This builds the compatible commands under `target/debug` and puts them first on
  `PATH`. Source the environment helper again when switching checkouts.
- **Python 3** for deployment scripts and JSON manifests.
- **Wallet home** exported in every shell you use (deploy *and* spel must point at the
  same wallet/network):
  ```bash
  export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
  ```
- Wallet configured to reach your sequencer (`wallet_config.json`), with the accounts you
  need created (`wallet account ...`). The current config stores connections in its
  `sequencers` list. The stablecoin bootstrap also accepts `SEQUENCER_ADDR` for a
  temporary connection override.

### Argument formats (used throughout)

| Kind | Accepted forms |
|---|---|
| **account id** (PDAs, holdings, authority) | base58 (e.g. `9qbX…`) **or** `0x`-prefixed 32-byte hex. **No** `account_id( … )` wrapper. |
| **program id** | 8 comma-separated u32 limbs, a bare 64-char ImageID hex, a `0x`-prefixed ImageID hex, **or** a base58 ImageID. Use `spel --format hex -- program-id <binary>` to print ImageID hex. |

---

## 1. Build & deploy the programs

**Note: Only do this if you want to deploy the protocols yourself!**

Build and deploy **token**, **twap_oracle**, and **amm** (order doesn't matter for deploy,
but you need all three before initializing the AMM):

```bash
make build-programs
# Repeat deployment for: token, twap_oracle, amm.
wallet deploy-program target/guest/<prog>.bin
```

Binary path convention: `target/guest/<prog>.bin`

### Record the ProgramIds

```bash
spel --format hex -- program-id target/guest/<prog>.bin
```

Record each ProgramId in whatever form you'll pass it — the forms are interchangeable
(see the arg-format table above). `--format hex` selects the exact 64-character
ImageID used in manifests; the default display format is different. Use `program-id`
for binary identity and `inspect <account> --type <type>` for account data. Fill in:

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
     --program target/guest/token.bin \
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
     --program target/guest/token.bin \
     -- initialize-account \
     --definition-account <DEF> \
     --account-to-initialize <NEW_HOLDING>
```

Then transfer (`amount-to-transfer` is in **base units** — e.g. `1000000000000000000` = one
whole token at 18 decimals):

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
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
| stablecoin | `cargo run -q -p stablecoin_program --example stablecoin_pdas -- <stablecoin_pid>` | protocol_parameters, stability_fee_accumulator, redemption_price_state, stablecoin_definition, stablecoin_master_holding, clock |
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
`config` is the PDA from step 4; `token`/`twap` are the ProgramIds from step 1.

```bash
spel --idl artifacts/amm-idl.json \
     --program target/guest/amm.bin \
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
     --program target/guest/amm.bin \
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
     --program target/guest/amm.bin \
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
     --program target/guest/amm.bin \
     -- swap-exact-input \
     --config <CONFIG_PDA> \
     --pool <POOL_PDA> \
     --vault-a <VAULT_A_PDA> \
     --vault-b <VAULT_B_PDA> \
     --user-holding-a <USER_HOLDING_A> \
     --user-holding-b <USER_HOLDING_B> \
     --current-tick-account <CURRENT_TICK_PDA> \
     --clock 4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU \
     --protocol-fee-holding <PROTOCOL_FEE_PDA_FOR_INPUT_TOKEN> \
     --swap-amount-in <AMOUNT_IN> \
     --min-amount-out <MIN_OUT> \
     --token-definition-id-in <DEF_OF_INPUT_TOKEN> \
     --deadline 18446744073709551615
```

- **`--token-definition-id-in`**: `<DEF_A>` ⇒ A→B; `<DEF_B>` ⇒ B→A.
- **`--protocol-fee-holding`**: the protocol-fee PDA **for the input token** — `protocol_fee_a`
  when spending A, `protocol_fee_b` when spending B (from step 4's `amm_pdas` output). The swap
  diverts `protocol_fee_bps` of the fee here (creating it on first use); pass it even when the
  instance's protocol fee is `0` (the account is still required and simply left untouched).
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
     --program target/guest/twap_oracle.bin \
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
     --program target/guest/amm.bin \
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
     --program target/guest/twap_oracle.bin \
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
     --program target/guest/amm.bin \
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

> The live testnet faucet PDA, the deployed faucet ImageID, and the faucet-mintable token
> definitions are in [DEPLOYMENTS.md](../DEPLOYMENTS.md). The steps below are the generic
> flow; when using the live instance, take the concrete ids from there.

### 14.1 Deploy the faucet + derive its PDA

Build and deploy like the other programs, then derive its mint-authority PDA **from the
exact binary you deployed** (the PDA is a function of the ImageID):

```bash
make build-programs
wallet deploy-program target/guest/token_mint_authority.bin

cargo run -q -p token_mint_authority_program --example mint_authority -- \
  target/guest/token_mint_authority.bin
# prints the base58 <FAUCET_PDA>
```

> **Build consistency.** Unlike `token`, the faucet's `target/guest` and docker builds have
> **different** ImageIDs — so derive the PDA from, and deploy, the **same** binary, and set
> the tokens' `mint_authority` to that PDA. A mismatch makes every FaucetMint fail. (The
> live testnet faucet is the `target/guest` build — see DEPLOYMENTS.md.)

### 14.2 Make a token faucet-mintable (at creation)

A token is only faucet-mintable if its `mint_authority` was set to `<FAUCET_PDA>` when it
was created (§3). Add `--mint-authority` to `new-fungible-definition`:

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     -- new-fungible-definition \
     --name "TOKEN A" --total-supply 1000000000000000000000 \
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

Submitting the transactions below needs only the **program id** — pass the ImageID hex
(from [DEPLOYMENTS.md](../DEPLOYMENTS.md)) to `--program` and spel skips loading the
binary (the program-id forms are listed in the [arg-format table](#argument-formats-used-throughout)).
So `<FAUCET_PROGRAM_ID>` / `<TOKEN_PROGRAM_ID>` below are ImageIDs, not paths.

The one step that still needs the faucet **binary** is deriving the per-`(recipient,
definition)` `mint_allowance` PDA (the helper reads the bin to get its ImageID; it can't
yet take an ImageID directly, so build it once with `make build-programs` if you don't have
it):

```bash
cargo run -q -p token_mint_authority_program --example faucet_allowance -- \
  <FAUCET_BIN> <RECIPIENT> <DEF_A>
# prints the base58 <MINT_ALLOWANCE>
```

If the recipient's holding is fresh, initialize it first (the faucet won't):

```bash
spel --idl artifacts/token-idl.json \
     --program <TOKEN_PROGRAM_ID> \
     -- initialize-account \
     --definition-account <DEF_A> \
     --account-to-initialize <USER_HOLDING>
```

Then mint:

```bash
spel --idl artifacts/token_mint_authority-idl.json \
     --program <FAUCET_PROGRAM_ID> \
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

---

## 15. Stablecoin bootstrap

### Standalone deployment

Use an existing wallet configured for the destination sequencer. The bootstrap uses
its keys without restoring or replacing the wallet:

```bash
make setup-workspace-tools
source scripts/workspace-env.sh
export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
make deploy-stablecoin

# Optional endpoint override applies to this invocation only:
SEQUENCER_ADDR=http://127.0.0.1:3040 make deploy-stablecoin
```

The script builds and inspects release binaries when program IDs have not been
selected, reuses matching deployed programs, derives the stablecoin global PDAs,
creates dedicated collateral and a seed oracle, then initializes and verifies the
protocol. The oracle must exist before initialization; its base asset is the
deterministic stablecoin definition PDA that initialization will create.

Explicit `TOKEN_PROGRAM_ID`, `TWAP_ORACLE_PROGRAM_ID`, and `STABLECOIN_PROGRAM_ID`
select already deployed programs compatible with the repository IDLs. Supplying all
three avoids guest builds. `COLLATERAL_DEFINITION_ID` or `MARKET_PRICE_ORACLE_ID`
select **already initialized** assets, whose ownership and bindings must match.
Omit them to generate fresh assets. `ADMIN_ID`, `ORACLE_SOURCE_ID`, and any fresh
collateral accounts must belong to the selected wallet. `COLLATERAL_MINT_AUTHORITY_ID`
defaults to the admin and can instead be a faucet PDA.

Default settings are 1.0 stability fee per millisecond (no interest), zero controller
gains, a 150% minimum collateralization ratio, 1.0 redemption price, 300,000 ms between
rate updates, and a 900,000 ms maximum oracle age. Collateral supply is `10^21` units
and the oracle window is 300,000 ms. Protocol values use `10^27` fixed point; the
oracle seed uses Q64.64 (`2^64` = 1.0). Nonzero controller gains are rejected because
the protocol does not yet convert the oracle's price units for its controller.

This is bootstrap state, not a live market feed. The seed becomes stale after the
configured maximum age, and debt generation then fails. A production deployment
needs a suitable price source, ongoing publication, and the price-unit conversion
resolved before enabling the controller. Display every supported override with:

```bash
apps/stablecoin/tests/testnet/setup-stablecoin-testnet.sh --help
```

### Deployment records and recovery

Successful runs print paths to `deployment.json` and `deployment.env`, by default
under `target/deployments/stablecoin/<channel-id>/`. JSON records program IDs, account
IDs, submitted transaction hashes, settings, and verified on-chain state. The shell
file exports the same IDs for subsequent commands:

```bash
source target/deployments/stablecoin/<channel-id>/deployment.env
spel --idl artifacts/stablecoin-idl.json inspect "$PROTOCOL_PARAMETERS_ID" --type ProtocolParameters
```

Repeat the same bootstrap command to resume an interrupted deployment or verify a
completed one. It confirms saved submissions before proceeding and does not repeat a
confirmed collateral mint. A pending transaction requires confirmation before the
run can continue. Overrides that conflict with recorded bindings or bootstrap
settings fail rather than silently reconfigure the protocol. `DEPLOYMENT_DIR` selects
a different record directory; a directory from another channel is rejected.
Changing this directory does not create another protocol: global accounts are
singletons derived from the stablecoin program ID. Reusing that ID on the same
channel adopts the same on-chain protocol, whose bindings must still match.
Adopted protocols record current verified state without inventing historical settings.

### Integrated AMM and faucet setup

The isolated AMM test setup enables stablecoin bootstrap by default:

```bash
make setup-workspace-tools
source scripts/workspace-env.sh
make build-programs
TEST_SEQUENCER_ADDR=http://127.0.0.1:3040 \
  apps/amm/tests/testnet/setup-amm-testnet.sh

# Prepare only AMM state:
DEPLOY_STABLECOIN=0 TEST_SEQUENCER_ADDR=http://127.0.0.1:3040 \
  apps/amm/tests/testnet/setup-amm-testnet.sh
```

This setup uses a public deterministic test mnemonic. Use it only with disposable
test funds. Its wallet defaults to `apps/amm/tests/testnet/.wallet`; it does not use
the standalone wallet. The AMM setup is intended for a fresh test deployment, while
the delegated stablecoin bootstrap supports resuming its own recorded work.

Token and oracle program IDs come from the AMM deployment. Stablecoin gets dedicated
collateral, leaving token A–D balances intact, and its mint authority is the same
faucet PDA used by the AMM tokens. Existing AMM account indexes remain unchanged;
stablecoin admin/oracle roles are appended only when enabled. The bootstrap retains
the channel-scoped output directory unless `STABLECOIN_DEPLOYMENT_DIR` is supplied.
Both entry points write the same manifest schema and print its paths.

When stablecoin is enabled, the AMM setup first saves the four selected AMM/faucet
binaries under `target/deployments/amm-testnet/binaries/<content-hash>/`. Deployment,
PDA derivation, the faucet manifest, and the printed AMM launch command all use these
snapshots. A later stablecoin build can replace shared guest outputs or remove
legacy build directories without changing the binaries recorded for this deployment.

To mint more collateral into the bootstrap's existing holding, use that deployment's
environment file and the faucet manifest produced by the same AMM run:

```bash
export LEE_WALLET_HOME_DIR="$PWD/apps/amm/tests/testnet/.wallet"
source target/deployments/stablecoin/<channel-id>/deployment.env
FAUCET_MANIFEST=apps/amm/tests/testnet/faucet.json
FAUCET_PROGRAM_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["faucetProgramId"])' "$FAUCET_MANIFEST")
FAUCET_BIN=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["faucetBin"])' "$FAUCET_MANIFEST")
MINT_ALLOWANCE_ID=$(RISC0_SKIP_BUILD=1 cargo run -q -p token_mint_authority_program \
  --example faucet_allowance -- "$FAUCET_BIN" "$ADMIN_ID" "$COLLATERAL_DEFINITION_ID" \
  | awk '/^base58:/ {print $2}')

spel --idl artifacts/token_mint_authority-idl.json --program-id "$FAUCET_PROGRAM_ID" faucet-mint \
  --recipient "$ADMIN_ID" --mint-allowance "$MINT_ALLOWANCE_ID" \
  --user-holding "$COLLATERAL_HOLDING_ID" --token-definition "$COLLATERAL_DEFINITION_ID" \
  --mint-authority "$COLLATERAL_MINT_AUTHORITY_ID" --clock "$CLOCK_ID"
```

The admin signs; the distinct collateral holding already exists. The faucet's
per-recipient/token cooldown still applies. For another recipient/holding, initialize
its collateral holding first and derive the matching allowance as in §14.3.

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


---

## 16. Standalone sequencer module devnet

This runbook builds and deploys the token, TWAP oracle, and stablecoin programs,
then exercises the stablecoin stack through the `logoscore` binary from
`logos-logoscore-cli`, `lez_core`, `token_module`, and `stablecoin_module`.
The sequencer runs with its `standalone` build feature and `RISC0_DEV_MODE=1`.
No Logos blockchain node, Bedrock, or indexer process is required.

The commands below are verified on Linux x86_64. Run them from the repository
root in Bash. You need Docker/BuildKit, Nix with flakes, Python 3, Git, Rust
1.94.0, and the repository's RISC Zero toolchain. Install `jq` for the manual
examples. The module builds use the wallet-module revision in `flake.lock`;
the wallet/SPEL/sequencer commands use revisions resolved through Cargo.

### 16.1. Build the compatible commands and guests

```bash
make setup-devnet-tools
source scripts/workspace-env.sh
make build-programs

mkdir -p target/devnet-build
nix build --accept-flake-config \
  'github:logos-co/logos-logoscore-cli/ba065cc335aa3ad8167642bbf6f448186b8d4e37#cli' \
  --out-link target/devnet-logoscore
target/devnet-logoscore/bin/logoscore --version
```

`setup-devnet-tools` also builds `sequencer_service --features standalone`.
The runtime command is named `logoscore`, not `logoscore-cli`. Use the Nix
**dev** CLI: a portable/AppImage runtime rejects the `linux-amd64-dev`
variants produced by these module builds.

### 16.2. Include unstaged and untracked source in the module builds

A regular Git-backed Nix build excludes new untracked files. Make a small
source snapshot from Git's tracked and nonignored untracked files. This keeps
build output, wallets, runtime data, and local evidence out of Nix's input.
Use a fresh snapshot after changing source.

```bash
export DEVNET_SOURCE="$(mktemp -d "$PWD/target/devnet-source.XXXXXX")"
python3 - <<'PY'
import os, shutil, subprocess
from pathlib import Path
root = Path.cwd()
snapshot = Path(os.environ["DEVNET_SOURCE"])
paths = subprocess.check_output([
    "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"
]).decode().split("\0")
for name in paths:
    if not name:
        continue
    source = root / name
    if source.is_file():
        target = snapshot / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
PY

nix build --accept-flake-config "path:$DEVNET_SOURCE#stablecoin-module" \
  --out-link target/devnet-build/stablecoin-module
nix build --accept-flake-config "path:$DEVNET_SOURCE#token-module" \
  --out-link target/devnet-build/token-module
nix build --accept-flake-config --impure --expr '
  let workspace = builtins.getFlake (builtins.getEnv "DEVNET_SOURCE");
  in workspace.inputs.lez_core.packages.${builtins.currentSystem}.default
' --out-link target/devnet-build/lez-core
nix build --accept-flake-config "path:$DEVNET_SOURCE#stablecoin-module-tests" \
  --out-link target/devnet-build/stablecoin-module-tests
```

The pinned wallet module's `open` method takes **three** paths: config,
storage, and statistics. Older two-path examples do not match this pin.

### 16.3. Execute the full live test

```bash
export DEVNET_STATE="$PWD/target/stablecoin-devnet/full"
python3 scripts/test_stablecoin_devnet.py \
  --state-dir "$DEVNET_STATE" \
  --port 3140 \
  --logoscore-bin "$PWD/target/devnet-logoscore/bin/logoscore" \
  --wallet-module "$(readlink -f target/devnet-build/lez-core)" \
  --token-module "$(readlink -f target/devnet-build/token-module)" \
  --stablecoin-module "$(readlink -f target/devnet-build/stablecoin-module)" \
  --keep-running
```

Choose a new state directory for each run. The test refuses existing populated
state and an occupied RPC port. Omitting `--keep-running` stops the test's
sequencer and Logos daemon after success. Failures also stop those processes
while preserving logs and state for inspection.

The test performs these operations:

1. Start only a standalone sequencer on `127.0.0.1:3140`, with 500 ms blocks,
   disabled cross-zone watchers, and disabled metrics. Its configuration keeps
   the required Bedrock fields, but the standalone mock publisher does not use
   their URL.
2. Create a disposable wallet, run the deployment script, and verify a second
   deployment run adds no transactions. The bootstrap initializes the fresh
   admin identity before its first protocol transaction, deploys the three
   programs, creates collateral and the oracle seed, and initializes globals.
3. Stage and load the pinned wallet module and the current token/stablecoin
   modules in an isolated Logos configuration directory. Open and sync that
   wallet through `lez_core`.
4. Exercise all 26 stablecoin methods exposed by this stack. `initializeProgram`
   is checked for duplicate rejection after the script's initial bootstrap.
5. Open a position, deposit, borrow, check exact health/holding/supply values,
   withdraw, freeze, repay, unfreeze, withdraw the remaining collateral, and
   close. Confirm every submitted transaction through `getTransaction`.
6. Exercise maintenance and all parameter setters, expire the oracle through a
   timing change, verify strict-update rejection and best-effort fee refresh,
   then restore the freshness limit.
7. Initialize another identity, rotate admin and freeze authority, prove the
   old identities lose their permissions, and restore the original authorities.

Inspect the machine-readable result and individual module calls:

```bash
cat "$DEVNET_STATE/report.json"
jq '.[] | select(has("includedInBlock")) |
    {module, method, includedInBlock}' "$DEVNET_STATE/calls.json"
cat "$DEVNET_STATE/deployment/deployment.json"
```

`calls.json` stores public arguments/results and inclusion blocks. The test
captures and discards initial wallet-creation output containing a recovery
phrase; it uses a dedicated wallet rather than an existing operator wallet.

### 16.4. Connect manually to the running modules

```bash
source "$DEVNET_STATE/runtime.env"

lc() {
  "$PWD/target/devnet-logoscore/bin/logoscore" \
    --config-dir "$LOGOSCORE_CONFIG_DIR" --json "$@"
}

lc status
lc list-modules
lc module-info stablecoin_module
lc call stablecoin_module programInfo
lc call stablecoin_module protocolParameters
lc call stablecoin_module stabilityFeeAccumulator
lc call stablecoin_module redemptionPriceState
lc call stablecoin_module currentGlobalState
lc call stablecoin_module redemptionRateUpdateQuote
```

The runner already opened the wallet. After restarting the daemon, load the
modules and open it with all three paths:

```bash
lc load-module lez_core
lc load-module token_module
lc load-module stablecoin_module
lc call lez_core open \
  "$LEE_WALLET_HOME_DIR/wallet_config.json" \
  "$LEE_WALLET_HOME_DIR/storage.json" \
  "$LEE_WALLET_HOME_DIR/statistics.json"
```

Pass request objects with `json:`. Pass `u128`, `i128`, and nonce values inside
those objects as decimal strings. A module call returns an outer CLI envelope;
the module payload is in `.result`. A transaction hash means submission;
confirm inclusion and read resulting state before continuing.

### 16.5. Helpers for synchronized, confirmed manual transactions

```bash
sync_wallet() {
  local block
  block="$(lc call lez_core get_current_block_height | jq -er '.result')"
  lc call lez_core sync_to_block "json:$block" | jq -e '.result == 0' >/dev/null
}

confirm_tx() {
  python3 - "$SEQUENCER_ADDR" "$1" <<'PY'
import sys, time
sys.path.insert(0, "scripts")
from deploy_stablecoin import Rpc, id_bytes
rpc = Rpc({"sequencer_addr": sys.argv[1]})
transaction = id_bytes(sys.argv[2]).hex()
deadline = time.monotonic() + 120
while time.monotonic() < deadline:
    receipt = rpc.call("getTransaction", [transaction])
    if receipt is not None:
        print("Included in block", receipt[1])
        break
    time.sleep(0.2)
else:
    raise SystemExit("Transaction not included; inspect sequencer.log before retrying")
PY
}

module_tx() {
  local response transaction
  sync_wallet || return
  response="$(lc call "$@")" || return
  printf '%s\n' "$response" | jq -e '.result.status == "ok"' >/dev/null || {
    printf '%s\n' "$response"
    return 1
  }
  transaction="$(printf '%s\n' "$response" | jq -er '.result.transactionId')" || return
  confirm_tx "$transaction"
}
```

### 16.6. Execute another position lifecycle

The automated test settled and closed nonce `0`. Use nonce `1` for this new
position: clearing a position does not release its vault or allow nonce reuse.
The fixture exports its initialized collateral and stablecoin holdings.

```bash
module_tx stablecoin_module openPosition \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userCollateralHoldingId\":\"$COLLATERAL_HOLDING_ID\",\"initialCollateralAmount\":\"1000\"}"
module_tx stablecoin_module depositCollateral \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userCollateralHoldingId\":\"$COLLATERAL_HOLDING_ID\",\"amount\":\"500\"}"
module_tx stablecoin_module generateDebt \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userStablecoinHoldingId\":\"$STABLECOIN_HOLDING_ID\",\"amount\":\"100\"}"
lc call stablecoin_module positionHealth \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\"}"
lc call token_module inspectHolding "$STABLECOIN_HOLDING_ID"
lc call token_module inspectDefinition "$STABLECOIN_DEFINITION_ID"

module_tx stablecoin_module withdrawCollateral \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userCollateralHoldingId\":\"$COLLATERAL_HOLDING_ID\",\"amount\":\"100\"}"
module_tx stablecoin_module freeze "json:{\"freezeAuthorityId\":\"$FREEZE_AUTHORITY_ID\"}"
module_tx stablecoin_module repayDebt \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userStablecoinHoldingId\":\"$STABLECOIN_HOLDING_ID\",\"amount\":\"100\"}"
module_tx stablecoin_module unfreeze "json:{\"freezeAuthorityId\":\"$FREEZE_AUTHORITY_ID\"}"
module_tx stablecoin_module withdrawCollateral \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\",\"userCollateralHoldingId\":\"$COLLATERAL_HOLDING_ID\",\"amount\":\"1400\"}"
module_tx stablecoin_module closePosition \
  "json:{\"ownerId\":\"$ADMIN_ID\",\"positionNonce\":\"1\"}"

module_tx stablecoin_module accrueStabilityFee "$ADMIN_ID"
module_tx stablecoin_module updateRedemptionRate "$ADMIN_ID"
module_tx stablecoin_module refreshGlobals "$ADMIN_ID"
lc call lez_core save
```

Expect 1500 collateral and 100 nominal debt after borrowing; the final stablecoin
holding balance and total supply return to zero. Repayment remains available
while frozen. The static oracle seed expires after 15 minutes; if strict
maintenance/borrowing is blocked by `oracle_stale`, use a fresh fixture for this
walkthrough. `refreshGlobals` can still accrue fees with a stale oracle.

### 16.7. Admin commands and identity requirements

Use `module_tx` for each setter. The live test already executes them and checks
actual role rotation. These examples preserve the fixture's zero-gain,
zero-interest configuration:

```bash
module_tx stablecoin_module setStabilityFeePerMillisecond \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newRate\":\"1000000000000000000000000000\"}"
module_tx stablecoin_module setMinimumCollateralizationRatio \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newRatio\":\"1500000000000000000000000000\"}"
module_tx stablecoin_module setControllerGains \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newProportionalGain\":\"0\",\"newIntegralGain\":\"0\"}"
module_tx stablecoin_module setTimingParameters \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newMinimumMillisecondsBetweenRateUpdates\":\"1\",\"newMaximumOraclePriceAgeMilliseconds\":\"900000\"}"
module_tx stablecoin_module setMarketPriceOracle \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newOracleId\":\"$MARKET_PRICE_ORACLE_ID\"}"
module_tx stablecoin_module setFreezeAuthority \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newFreezeAuthorityId\":\"$FREEZE_AUTHORITY_ID\"}"
module_tx stablecoin_module setAdmin \
  "json:{\"adminId\":\"$ADMIN_ID\",\"newAdminId\":\"$ADMIN_ID\"}"
```

Before using another freshly created public key as an identity, initialize it
through `lez_core.register_public_account`. Confirm that transaction before
the identity's first other signed operation. The built-in initialization
requires a completely fresh account. A previously used default-owner account
with a nonzero nonce cannot be initialized with that instruction; the bootstrap
rejects such an admin before deploying programs. Existing program-owned admin
identities are reused. Fresh token-holding targets follow `token_module`'s
own initialization path rather than this identity-registration path.

This matters for the pinned SPEL version: its guest-output filter omits
nonempty default-owner accounts, which makes later signed calls fail the
sequencer's declared-account coverage check. The bootstrap initializes its
fresh admin before first use; the runner initializes replacement identities
before rotating roles. Read-only adoption of an existing deployment stays
read-only.

### 16.8. Restart, resume, and stop

The deployment manifest and binary snapshots live under
`$DEVNET_STATE/deployment`. To verify deployment resume against the running
sequencer, use a fresh shell so the exported program IDs do not change the
manifest's managed/reused program interpretation:

```bash
(
  unset TOKEN_PROGRAM_ID TWAP_ORACLE_PROGRAM_ID STABLECOIN_PROGRAM_ID
  unset TOKEN_PROGRAM_BIN TWAP_ORACLE_PROGRAM_BIN STABLECOIN_PROGRAM_BIN
  export MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES=1
  make deploy-stablecoin
)
```

Preserve the wallet, sequencer database, deployment directory, and exact binary
snapshots together. The runner's deployment manifests are bound to its channel.
For a full reset, choose a fresh state directory and rerun the test rather than
reusing wallet synchronization state against another chain.

To stop the two processes left by `--keep-running`:

```bash
lc stop
python3 - "$DEVNET_STATE/pids.json" <<'PY'
import json, os, signal, sys
# The first PID belongs to this fixture's sequencer. The CLI stop above shuts
# down the Logos daemon and its module hosts.
sequencer = json.load(open(sys.argv[1]))[0]
os.kill(sequencer, signal.SIGTERM)
PY
```

To restart this fixture manually, use its saved sequencer configuration and
module directory, then run the load/open/sync commands in section 16.4:

```bash
RISC0_DEV_MODE=1 target/debug/sequencer_service \
  "$DEVNET_STATE/sequencer_config.json" --port 3140 --listen-address 127.0.0.1
# In another shell, source runtime.env and start the daemon with the same IDs:
RISC0_DEV_MODE=1 target/devnet-logoscore/bin/logoscore \
  --config-dir "$LOGOSCORE_CONFIG_DIR" -D -m "$DEVNET_STATE/modules" \
  --persistence-path "$DEVNET_STATE/module-data"
```

The fixture deliberately keeps controller gains at zero: the oracle uses
Q64.64 prices while the controller expects `10^27` fixed point. This validates
deployment, account/transaction contracts, and module operations in development
execution mode. Enabling a live feedback controller requires the separate
price-conversion and keeper work.
