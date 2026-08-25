# Token program — spel runbook

How to use the **token** program end to end with `spel`: create tokens, hand out holdings,
transfer, mint, burn, rotate the mint authority, and print NFTs. Each section is
self-contained — unlike the AMM runbook there is no fixed order beyond "create a definition
before you use it".

**Verified against:** token program at `main` (`4363f13`), building on LEZ **v0.2.0**
(`nssa_core`/`nssa` workspace pin) and `spel-framework` **v0.6.0** (guest pin). CLI behaviour
below was exercised with the `spel` CLI **v0.5.0**; where v0.6.0 differs it is called out.

> **Golden rule:** every time you **recompile** the guest, its **ProgramId changes**, and the
> new build is a **different program**. Token accounts record the `program_owner` of the build
> that created them, and `initialize-account`, `mint`, `mint-with-authority`, `set-authority`,
> and `set-authority-with-authority` all assert it (`Token definition must be owned by token
> program`). Tokens created by an older build are **not** manageable by a newer one — recreate
> them. The token program has **no PDAs**, so there is nothing to re-derive; every account is a
> plain wallet account.

---

## Contents

- [0. Prerequisites](#0-prerequisites)
  - [Argument formats](#argument-formats-used-throughout)
- [1. Build & deploy the token program](#1-build--deploy-the-token-program)
- [2. Wallet CLI basics](#2-wallet-cli-basics)
- [3. The account model](#3-the-account-model)
  - [Who signs what](#who-signs-what)
- [4. Create a fungible token](#4-create-a-fungible-token)
- [5. Give someone a holding (`initialize-account`)](#5-give-someone-a-holding-initialize-account)
- [6. Transfer](#6-transfer)
- [7. Mint more supply](#7-mint-more-supply)
- [8. Rotate or renounce the mint authority](#8-rotate-or-renounce-the-mint-authority)
- [9. Burn](#9-burn)
- [10. NFTs (metadata, master, printed copies)](#10-nfts-metadata-master-printed-copies)
- [11. Inspect](#11-inspect)
- [Instruction reference](#instruction-reference)
- [Gotchas](#gotchas)

---

## 0. Prerequisites

- **Docker running** (guest builds cross-compile through it) — only if you build/deploy yourself.
- **`spel` / `wallet`** from the [SPEL](https://github.com/logos-co/spel) toolchain, on `PATH`.
- **Wallet home** exported in every shell you use (deploy *and* `spel` must point at the same
  wallet/network):
  ```bash
  export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
  ```
- Wallet pointed at your sequencer:
  ```bash
  wallet config set sequencer_addr https://testnet.lez.logos.co/
  ```
- The **IDL** at `artifacts/token-idl.json` (regenerate with `make idl`) and the **binary** you
  deployed. Every instruction below is `spel --idl <IDL> --program <BIN> -- <instruction> …`.

### Argument formats (used throughout)

| Kind | Accepted forms |
|---|---|
| **account id** (definitions, holdings, authorities) | base58 (e.g. `9qbX…`) **or** `0x`-prefixed 32-byte hex. **No** `account_id( … )` wrapper. |
| **amount** (`u128`) | plain decimal integer, in **base units**. The token program stores **no decimals field** — decimals are a UI convention. `1000000000000000000` = one whole token if your UI assumes 18. |
| **`Option<account_id>`** (`--mint-authority`, `--new-authority`) | an account id for `Some`, or the literal `none` (`null` also works) for `None`. **The flag itself is always required** — spel has no optional args, so omitting it is an error, not `None`. |

---

## 1. Build & deploy the token program

**Only do this if you want to deploy the program yourself** — otherwise use an existing
deployment's binary + ProgramId.

```bash
make build-programs                       # all guests → target/guest/<program>.bin
wallet deploy-program target/guest/token.bin
```

Building a single guest directly (debug/iteration) puts the binary at
`programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin`:

```bash
cargo risczero build --manifest-path programs/token/methods/guest/Cargo.toml
```

Record the ProgramId — you need it to configure programs that chain into the token program
(the AMM's `--token-program-id`, the ATA's `--token-program-id`):

```bash
spel -- program-id target/guest/token.bin
```

It prints the **decimal limbs** and the **64-char ImageID hex**; both forms (plus `0x`-hex and
base58) are accepted wherever a program id is taken.

> The token program itself is **not** initialized or configured — there is no `config` account
> and no global admin. Deploy it and start creating tokens.

## 2. Wallet CLI basics

Every account you pass below is a wallet account you own. Create one **public** account per
role, with a `--label` you can recognize later:

```bash
wallet account new public --label "Token A Definition"
wallet account new public --label "Token A Holding"
wallet account list        # `ls` is an alias; -l/--long for full details
```

The listing shows each account's label and base58 id:

```
/1   Public/4T69U868K6UzX8zbesU5wyr36gxaU7wb91Q45yedP4Rb [Token A Holding]
/0   Public/CER21z16YgmWr3aN8FEHsrmfm2iRfQiwZTac3FQa21US [Token A Definition]
```

Use those ids as the `<DEF>`, `<HOLDING>`, and `<AUTHORITY>` arguments. **`spel` signs only
accounts whose keys your wallet holds** — every account marked *signer* below must be yours.

## 3. The account model

The token program owns three account types. All are ordinary accounts until an instruction
claims them; the *type* is the shape of the data written into them.

| Account | Created by | Holds |
|---|---|---|
| `TokenDefinition` | `new-fungible-definition`, `new-definition-with-metadata` | name, total/printable supply, optional metadata id, optional mint authority |
| `TokenHolding` | the same two instructions (first holding), `initialize-account`, `print-nft` | `definition_id` + a balance: `Fungible { balance }`, `NftMaster { print_balance }`, or `NftPrintedCopy { owned }` |
| `TokenMetadata` | `new-definition-with-metadata` | `definition_id`, standard (`Simple`/`Expanded`), `uri`, `creators`, `primary_sale_date` |

**A holding is bound to one definition.** Every operation cross-checks `definition_id`;
mismatches abort (`Sender and recipient definition id mismatch`, `Mismatch Token Definition and
Token Holding`).

### Who signs what

Writing into a **fresh** account means *claiming* it, which requires that account's
authorization in the same transaction. Instructions that create accounts therefore mark them
`init, signer` — your wallet must hold the key for each:

| Creates | Account(s) that must sign |
|---|---|
| `new-fungible-definition` | definition target, holding target |
| `new-definition-with-metadata` | definition target, holding target, metadata target |
| `initialize-account` | the new holding |
| `print-nft` | the master holding **and** the fresh printed copy |

Two instructions are the exception: **`transfer`'s recipient and `mint`'s holding are not
signers.** Their handlers can claim an empty account, but the instruction never collects that
account's signature, so over `spel` the fresh-account path is unreachable — **initialize the
destination holding first** (step 5). This is the single most common source of confusion below.

The IDL also has no notion of an *optional account*, which is why authority operations come in
pairs — `mint` / `mint-with-authority`, `set-authority` / `set-authority-with-authority` —
instead of one instruction with an optional authority account. Pick the variant that matches
where the definition's authority currently lives (steps 7 and 8).

## 4. Create a fungible token

Creates the definition **and** its first holding, credited with the full `total_supply`. Both
target accounts must be **fresh** (never written to) and **signers**.

```bash
wallet account new public --label "Token A Definition"
wallet account new public --label "Token A Holding"
```

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- new-fungible-definition \
     --name "TOKEN A" \
     --total-supply 1000000000000000000000 \
     --definition-target-account <DEF> \
     --holding-target-account <HOLDING> \
     --mint-authority <AUTHORITY>
```

`--mint-authority` decides the supply model. It is a required flag, but all three models are
reachable from the CLI:

| Value | Meaning |
|---|---|
| `<DEF>` (the definition's own id) | **self authority** — mint with `mint` (step 7a), signing as the definition account |
| some other account id | **external authority** — mint with `mint-with-authority` (step 7b), signing as that account |
| `none` | **fixed supply** — minting is permanently rejected (`authority revoked, supply is fixed`) |

An all-zero authority id is rejected (`Mint authority must be a valid non-zero account ID`).

> Both target accounts are `init` **and** `signer`: your wallet must hold both keys, and
> re-running with the same accounts fails (`Definition target account must have default values`).

Verify:

```bash
spel --idl artifacts/token-idl.json inspect <DEF>     --type TokenDefinition
spel --idl artifacts/token-idl.json inspect <HOLDING> --type TokenHolding
```

## 5. Give someone a holding (`initialize-account`)

Creates an **empty** holding for an existing definition. This is the prerequisite for receiving
a transfer or a mint. The new holding is a **signer** — your wallet must hold its key.

```bash
wallet account new public --label "Token A Holding (Bob)"
```

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- initialize-account \
     --definition-account <DEF> \
     --account-to-initialize <NEW_HOLDING>
```

- `<DEF>` is read-only here and is **not** signed — anyone can initialize a holding for any
  definition, as long as they sign the new account.
- For a **fungible** definition the result is `Fungible { balance: 0 }`; for a **non-fungible**
  one it is `NftPrintedCopy { owned: false }` — an empty slot ready to receive a printed copy.
  There is no way to initialize an `NftMaster` holding this way.
- The definition must be owned by *this* token build, or the call aborts (`Token definition must
  be owned by token program`).

## 6. Transfer

Moves value between two holdings of the **same** definition. `spel` signs the **sender**; the
recipient is only written to.

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- transfer \
     --sender <SENDER_HOLDING> \
     --recipient <RECIPIENT_HOLDING> \
     --amount-to-transfer <AMOUNT>
```

- `<SENDER_HOLDING>` — a holding you own (signer); balance must be ≥ `<AMOUNT>`, else
  `Insufficient balance`.
- `<RECIPIENT_HOLDING>` — an **already-initialized** holding of the same definition (step 5).
  The recipient is not a signer, so a fresh account cannot be claimed here.
- Same-definition is enforced: `Sender and recipient definition id mismatch`.

Per holding type:

| Holding type | `--amount-to-transfer` | Notes |
|---|---|---|
| `Fungible` | any amount ≤ balance | ordinary balance move |
| `NftPrintedCopy` | must be `1` | sender must own it, recipient slot must be un-owned |
| `NftMaster` | must equal the sender's **entire** `print_balance` | recipient must be an `NftMaster` holding at `0` — see [step 10](#transferring-a-master) |

Verify:

```bash
spel --idl artifacts/token-idl.json inspect <RECIPIENT_HOLDING> --type TokenHolding
```

## 7. Mint more supply

Minting adds to both the holding's balance and the definition's `total_supply`. It is fungible-
only (`Cannot mint additional supply for Non-Fungible Tokens`) and gated on the definition's
stored authority. Which of the two instructions you use depends on *who* that authority is.

The target holding must already exist and belong to `<DEF>` (step 5) — it is not a signer, so a
fresh account cannot be claimed by a mint.

### 7a. Self / PDA authority (`mint`)

Use when the stored authority **is the definition account itself**. The definition signs.

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- mint \
     --definition-account <DEF> \
     --user-holding-account <HOLDING> \
     --amount-to-mint <AMOUNT>
```

> This is also the path programs use: a PDA-owned definition (e.g. the AMM's LP token) is
> authorized under its seeds in a chained call instead of by a key.

### 7b. External authority (`mint-with-authority`)

Use when the stored authority is a **separate account** — the normal case when you passed a
distinct `--mint-authority` at creation, or rotated it later. The authority signs; the
definition is written but does **not** sign.

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- mint-with-authority \
     --definition-account <DEF> \
     --user-holding-account <HOLDING> \
     --authority-account <AUTHORITY> \
     --amount-to-mint <AMOUNT>
```

Failure modes are shared by both: `signer is not the current authority` (wrong account, or the
wrong variant for where the authority lives), `authority revoked, supply is fixed` (authority is
`None`), `Mismatch Token Definition and Token Holding` (holding belongs to another token).

## 8. Rotate or renounce the mint authority

Same split as minting: `set-authority` when the definition itself is the current authority,
`set-authority-with-authority` when a separate account is. Fungible-only.

```bash
# current authority is the definition account
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- set-authority \
     --definition-account <DEF> \
     --new-authority <NEW_AUTHORITY>
```

```bash
# current authority is a separate account (it signs)
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- set-authority-with-authority \
     --definition-account <DEF> \
     --authority-account <CURRENT_AUTHORITY> \
     --new-authority <NEW_AUTHORITY>
```

- `--new-authority none` **permanently renounces** minting — the supply is fixed and no later
  rotation is possible (`SetAuthority failed: authority already revoked`). There is no undo.
- Rotating hands over control: after `--new-authority <BOB>`, only Bob can mint, and only via
  `mint-with-authority` / `set-authority-with-authority`.
- Checks run before any mutation, so a rejected call leaves the previous authority intact.

Confirm the new value in the definition's `authority` field:

```bash
spel --idl artifacts/token-idl.json inspect <DEF> --type TokenDefinition
```

## 9. Burn

Destroys supply from a holding you own. The **holding** signs; the definition is written but not
signed, so any holder can burn their own tokens without the issuer's involvement.

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- burn \
     --definition-account <DEF> \
     --user-holding-account <HOLDING> \
     --amount-to-burn <AMOUNT>
```

| Holding type | Effect |
|---|---|
| `Fungible` | `balance -= amount`, `total_supply -= amount` |
| `NftMaster` | `print_balance -= amount`, `printable_supply -= amount` — burns unprinted capacity |
| `NftPrintedCopy` | amount must be `1`; marks the copy un-owned and decrements `printable_supply` |

Burning more than you hold aborts (`Insufficient balance to burn`). Burning is **not** gated on
the mint authority — a renounced, fixed-supply token can still shrink.

## 10. NFTs (metadata, master, printed copies)

A non-fungible token is a definition with `printable_supply` plus a metadata account. Its first
holding is an **`NftMaster`**, and each print carves an **`NftPrintedCopy`** out of it.
`print_balance` reserves one unit for the master itself, so `printable_supply: N` yields
**N − 1** printable copies.

> **`spel` cannot currently create NFT definitions.** `new-definition-with-metadata` takes two
> structured args (`--new-definition`, `--metadata`); `spel` rejects any value for them with
> `Serialization error: type mismatch: expected Defined { defined: "NewTokenDefinition" }, got
> Raw(…)` — its serializer has no encoding for IDL `defined` types. This holds for **both**
> CLI v0.5.0 (tested) and v0.6.0 (`spel-cli/src/parse.rs` still wraps the value as `Raw`, and
> `serialize.rs` has no `Defined` arm).
> Until that lands, create NFT (and metadata-bearing fungible) definitions programmatically —
> see `token_program::new_definition::new_definition_with_metadata` and the integration tests in
> `programs/integration_tests/tests/token.rs`. Everything else below works over the CLI against
> a definition created that way.

Shape of the two args, for when the CLI supports them:

```jsonc
// --new-definition
{ "NonFungible": { "name": "MY NFT", "printable_supply": 10 } }
// or: { "Fungible": { "name": "TOKEN A", "total_supply": 1000, "mint_authority": null } }

// --metadata
{ "standard": "Simple", "uri": "ipfs://…", "creators": "…" }
```

### Printing a copy

Both accounts are signers; the printed target must be **fresh** (do *not* pre-initialize it —
`print-nft` claims it itself).

```bash
wallet account new public --label "NFT copy #1"
```

```bash
spel --idl artifacts/token-idl.json \
     --program target/guest/token.bin \
     -- print-nft \
     --master-account <NFT_MASTER_HOLDING> \
     --printed-account <NEW_COPY_HOLDING>
```

Each print decrements the master's `print_balance` by 1 and writes
`NftPrintedCopy { owned: true }`. Printing requires `print_balance > 1`, so the last unit can
never be printed (`Insufficient balance to print another NFT copy`).

### Moving a printed copy

Initialize a slot for the recipient (step 5 against the NFT definition — it produces an un-owned
`NftPrintedCopy`), then transfer `1`:

```bash
spel --idl artifacts/token-idl.json --program target/guest/token.bin \
     -- initialize-account --definition-account <NFT_DEF> --account-to-initialize <RECIPIENT_SLOT>

spel --idl artifacts/token-idl.json --program target/guest/token.bin \
     -- transfer --sender <MY_COPY> --recipient <RECIPIENT_SLOT> --amount-to-transfer 1
```

### Transferring a master

Only meaningful into an existing `NftMaster` holding sitting at `print_balance: 0`, and the
amount must be the sender's whole `print_balance`. `initialize-account` cannot produce such a
holding (it always makes a printed-copy slot), and `transfer` cannot claim a fresh account, so
in practice **the master can only move to an account that previously gave one away**. Plan
master ownership at creation time.

## 11. Inspect

Read any token account back — read-only, no signing, no transaction:

```bash
spel --idl artifacts/token-idl.json inspect <DEF>      --type TokenDefinition
spel --idl artifacts/token-idl.json inspect <HOLDING>  --type TokenHolding
spel --idl artifacts/token-idl.json inspect <METADATA> --type TokenMetadata
```

`--type` is required and must match the account's actual shape — decoding a holding as a
definition fails. A holding's `definition_id` is how you find the token it belongs to.

## Instruction reference

`S` = must sign (your wallet needs the key), `init` = must be a fresh, never-written account.

| Instruction | Accounts | Args |
|---|---|---|
| `new-fungible-definition` | `definition-target-account` (S, init), `holding-target-account` (S, init) | `--name`, `--total-supply`, `--mint-authority` |
| `new-definition-with-metadata` | `definition-target-account` (S, init), `holding-target-account` (S, init), `metadata-target-account` (S, init) | `--new-definition`, `--metadata` — *see [step 10](#10-nfts-metadata-master-printed-copies)* |
| `initialize-account` | `definition-account`, `account-to-initialize` (S, init) | — |
| `transfer` | `sender` (S), `recipient` | `--amount-to-transfer` |
| `mint` | `definition-account` (S), `user-holding-account` | `--amount-to-mint` |
| `mint-with-authority` | `definition-account`, `user-holding-account`, `authority-account` (S) | `--amount-to-mint` |
| `set-authority` | `definition-account` (S) | `--new-authority` |
| `set-authority-with-authority` | `definition-account`, `authority-account` (S) | `--new-authority` |
| `burn` | `definition-account`, `user-holding-account` (S) | `--amount-to-burn` |
| `print-nft` | `master-account` (S), `printed-account` (S, init) | — |

## Gotchas

- **Recompile ⇒ new ProgramId ⇒ a different program.** Tokens created by the old build stay
  owned by it; `initialize-account`, `mint*`, and `set-authority*` reject them with `Token
  definition must be owned by token program`. Recreate your tokens after a rebuild.
- **You cannot credit an account that doesn't exist.** Neither `transfer` nor `mint` marks its
  recipient/holder as a signer, so the claim of a fresh account can't be authorized. Run
  `initialize-account` first. (`print-nft` is the exception — its printed target *is* a signer,
  so it must be fresh and must **not** be pre-initialized.)
- **`Option` args are required flags.** spel has no optional args: `--mint-authority` /
  `--new-authority` must always be passed. Pass the literal `none` for `None` — that is how you
  get a fixed-supply token or renounce an authority.
- **No optional accounts either.** That is why authority operations are split into `mint` /
  `mint-with-authority` and `set-authority` / `set-authority-with-authority`. Using the variant
  that doesn't match where the authority lives fails with `signer is not the current authority`,
  even when you hold the right key.
- **`--dry-run` only works before the `--` separator.** It is a *global* flag:
  `spel --idl … --program … --dry-run -- transfer …` resolves and prints the transaction, then
  exits with `Dry run complete — not submitted.` Put it after the instruction name instead and
  it is silently swallowed — the transaction is **submitted for real**.
- **Account ids must be bare base58/`0x`-hex** — strip the wallet's `account_id( … )` display
  wrapper.
- **Renouncing is permanent.** `--new-authority none` can never be reversed; the supply can then
  only shrink (via `burn`).
- **No decimals on-chain.** All amounts are raw base units; the definition stores no scale, so
  every client must agree on one out of band.
- **Same wallet home everywhere.** Deploying with one `LEE_WALLET_HOME_DIR` and running `spel`
  with another points them at different networks/keys.
