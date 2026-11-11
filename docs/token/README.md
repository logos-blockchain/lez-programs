# Token program: spel runbook

How to use the **token** program end to end with `spel`: create a token, give accounts
holdings, transfer, mint, rotate or renounce the mint authority, and burn. Each section stands
alone; the only order is "create a definition before you use it".

The token program is fungible only. NFTs are a separate program, `programs/nft`, with its own
IDL and runbook ([`docs/nft/README.md`](../nft/README.md)).

**Verified against:** the token program on `refactor/lez-v0.4.x`, LEZ's actor model
(`moudy/lez-books` @ `616256b9c`), and the `spel` CLI from spel's `refactor/lez-v0.4.x`
(`dbb6aec`), on a local sequencer. `scripts/e2e-token-nft.sh` runs the same flows.

> **A program is its address.** Every deploy gets a new address, and it is a different
> program: token state lives on an account per program, so definitions and holdings created
> under one deployment don't exist for another. After a redeploy, update `spel.toml` and
> recreate your tokens.

---

## Contents

- [0. Prerequisites](#0-prerequisites)
  - [Argument formats](#argument-formats)
- [1. Build & deploy](#1-build--deploy)
- [2. Accounts and labels](#2-accounts-and-labels)
- [3. The model](#3-the-model)
  - [Actors and state](#actors-and-state)
  - [The access list](#the-access-list)
  - [Who signs](#who-signs)
  - [Outcomes](#outcomes)
- [4. Create a token](#4-create-a-token)
- [5. Give an account a holding](#5-give-an-account-a-holding)
- [6. Transfer](#6-transfer)
- [7. Mint](#7-mint)
- [8. Rotate or renounce the mint authority](#8-rotate-or-renounce-the-mint-authority)
- [9. Burn](#9-burn)
- [10. Inspect](#10-inspect)
- [11. Other programs reading token state](#11-other-programs-reading-token-state)
- [Message reference](#message-reference)
- [Events](#events)
- [Gotchas](#gotchas)

---

## 0. Prerequisites

- **`spel`** built from spel's `refactor/lez-v0.4.x` branch, and LEZ's **`wallet`**, on `PATH`.
  Older `spel` releases read v1 IDLs only and fail with ``missing field `instructions` ``.
- **Wallet home** exported in every shell (`spel` opens LEZ's wallet from it; its config names
  the sequencer):
  ```bash
  export LEE_WALLET_HOME_DIR="$HOME/.lee/wallet"
  ```
- A **funded account** in the wallet to pay fees (`--fee-payer`).
- The **IDL** at `artifacts/token-idl.json` (regenerate with `make idl`).
- A **`spel.toml`** naming the deployed program (section 1). The examples below run from its
  directory and use `--program token`.

### Argument formats

| Kind | Accepted forms |
|---|---|
| **account** (`--root`, `--actor`, `--signer`, `--fee-payer`, account fields) | a wallet label (`alice`), base58, or `0x`-prefixed hex; a `Public/` prefix is accepted |
| **amount** (`u128`) | a decimal integer in base units. The program stores **no decimals**; `1000000000000000000` is one whole token only if your clients agree on 18. |
| **`Option<account>`** (`--mint-authority`, `--new-authority`) | an account for `Some`, or `none`. The flag is required: omitting it is an error, not `None`. |

`spel --program token -- <message> --help` lists a message's fields with their documentation.

---

## 1. Build & deploy

Skip this if you use an existing deployment; you only need its address.

```bash
make build-programs                                   # all guests → target/guest/<program>.bin
spel deploy target/guest/token.bin --fee-payer <FEE_PAYER>
```

`spel deploy` creates the header and segment accounts in the wallet, uploads the binary and
prints the program's address (the header account). Point a `spel.toml` at it:

```toml
[programs.token]
address = "<TOKEN_PROGRAM>"
idl     = "artifacts/token-idl.json"
binary  = "target/guest/token.bin"   # default for `spel deploy`
```

The program has no configuration and no admin: deploy it and start creating tokens. Programs
that call the token program (AMM, ATA) take this address, not the ImageID that
`spel program-id` prints.

## 2. Accounts and labels

Every account below is a wallet account. Create one public account per role with a label; the
label works anywhere `spel` takes an account:

```bash
wallet account new public --label usdc     # the definition
wallet account new public --label alice
wallet account new public --label bob
wallet account list
```

Accounts start empty. Nothing about an account says "definition" or "holding" until a token
message writes that state onto it.

## 3. The model

### Actors and state

An **actor** is an account under a program, written `ACCOUNT@PROGRAM` (`alice@token`). A
message goes to one actor, the **root**; the token program sees that actor's state and nothing
else. Steps that touch other accounts are messages between token actors: a transfer debits the
sender and messages the recipient, a burn messages the definition.

A token actor's state is one of:

| State | Fields |
|---|---|
| `Definition` | `name` (1–64 bytes), `total_supply`, `authority`: `Some(account)` may mint and rotate, `None` fixes the supply |
| `Holding` | `definition_id`, `balance` |

**A holding belongs to one definition** and can't change it. An account can hold one token
state per program: alice's token holding and her NFT holding are separate actors
(`alice@token`, `alice@nft`).

### The access list

LEZ loads only the actors a transaction declares, and a message to an undeclared actor fails
the whole transaction. `spel send` declares:

1. the root;
2. each `--actor`;
3. an actor on the root's program for each account field of the message (`--to`,
   `--definition`, `--holding`, `--mint-authority`, …);
4. an actor for each signer.

Actors a message reaches through **state** are not fields, so declare them yourself. In the
token program that is one case: **`burn` reaches the holding's definition**; pass
`--actor <definition>`.

### Who signs

`--signer` is explicit; `spel` never infers it. Which actor must authorize each message:

| Message | Must authorize |
|---|---|
| `new-definition` | the definition **and** the initial holding (both must be empty) |
| `initialize-holding` | the new holding |
| `transfer`, `burn` | the sending holding |
| `mint`, `set-authority` | the current authority (the definition itself when it is its own authority) |

The recipient of a transfer or mint never signs, and the definition doesn't sign for
`initialize-holding` or `burn`: anyone can open a holding of any token, and any holder can burn
their own balance.

A signature authorizes the **account**: every actor of that account in the transaction is
authorized, under every program. `send` prints what each signature covers before it sends;
read it when an account has state under several programs.

### Outcomes

LEZ includes a refused transaction in a block, leaves state unchanged, and reports no outcome
or error. `spel send` reads every declared actor before and after and prints what changed:

| exit | meaning |
|---|---|
| `0` | included, state changed |
| `2` | included, nothing changed: the program refused the message (or it was a no-op) |
| `1` | not sent: bad arguments, unknown account, a signer whose key the wallet lacks, … |

A refusal can't say why. The usual causes: an undeclared actor, a missing signature, or the
program's own checks (balance, authority, a holding of another token). `--dry-run` prints
the access list and signers without sending.

## 4. Create a token

The message goes to the empty **definition**. It writes the definition and creates the first
holding with the whole `total_supply`. Both accounts sign.

```bash
spel --program token send --fee-payer <FEE_PAYER> --root usdc --signer usdc --signer alice -- \
  new-definition --name USDC --total-supply 1000 --mint-authority usdc --holding alice
```

`--mint-authority` picks the supply model:

| Value | Meaning |
|---|---|
| the definition itself (`usdc`) | **self authority**: mint by sending to the definition, signed by it (7a) |
| another account | **external authority**: mint by sending to that account, signed by it (7b) |
| `none` | **fixed supply**: minting is refused forever |

Refused: a non-empty definition or holding, a missing signature, a name outside 1–64 bytes,
the zero account as authority, and `--holding` equal to the definition.

Verify:

```bash
spel --program token inspect usdc
spel --program token inspect alice
```

## 5. Give an account a holding

A transfer or mint can only credit an **existing holding of the same token**. Create one with
`initialize-holding`, sent to the new holding, which signs:

```bash
spel --program token send --fee-payer <FEE_PAYER> --root bob --signer bob -- \
  initialize-holding --definition usdc
```

The holding asks the definition, which answers, so a holding only exists for a definition that
does. The result is `Holding { definition_id: usdc, balance: 0 }`.

- The definition doesn't sign: anyone can open a holding of any token for an account they
  control.
- Repeating it for the same token is a no-op (exit `2`).
- An account that already holds another token, or is a definition, refuses.

## 6. Transfer

Sent to the **sender's** holding, which signs. It debits itself and credits the holding at
`--to`:

```bash
spel --program token send --fee-payer <FEE_PAYER> --root alice --signer alice -- \
  transfer --to bob --amount 300
```

- `--to` must already hold this token (section 5). Otherwise the credit fails and the whole
  transaction rolls back, the debit included (exit `2`).
- More than the balance is refused.
- The recipient doesn't sign.

## 7. Mint

Minting raises the definition's `total_supply` and credits the holding at `--to`, which must
already hold this token. Where you send it depends on who the authority is.

### 7a. The definition is its own authority

Send to the definition; it signs:

```bash
spel --program token send --fee-payer <FEE_PAYER> --root usdc --signer usdc -- \
  mint --definition usdc --to bob --amount 50
```

This is also how programs mint: a definition on a PDA (the AMM's LP token) authorizes with
its seed instead of a key.

### 7b. Another account is the authority

Send to the **authority's** token actor; it signs, and forwards the mint to the definition,
which checks the sender is its authority. Here `gov` was created with
`--mint-authority gov-authority`:

```bash
spel --program token send --fee-payer <FEE_PAYER> --root gov-authority --signer gov-authority -- \
  mint --definition gov --to bob --amount 5
```

The authority's token actor can be empty; forwarding leaves it unchanged.

Refused: a sender that isn't the current authority (including sending to the definition when
it isn't its own authority), a renounced authority, and a `--to` without a holding of this
token.

## 8. Rotate or renounce the mint authority

`set-authority` is routed like `mint`: to the definition when it is its own authority (7a),
otherwise to the authority's actor (7b).

```bash
# self authority: hand it to bob
spel --program token send --fee-payer <FEE_PAYER> --root usdc --signer usdc -- \
  set-authority --definition usdc --new-authority bob

# bob is the authority now: renounce
spel --program token send --fee-payer <FEE_PAYER> --root bob --signer bob -- \
  set-authority --definition usdc --new-authority none
```

- After a rotation only the new authority can mint or rotate; the old one is refused.
- `none` **renounces for good**: the supply can then only shrink, through `burn`.
- The zero account is refused as a new authority.

Check the `authority` field:

```bash
spel --program token inspect usdc
```

## 9. Burn

Sent to the holding, which signs. It debits itself and tells its definition to lower the
supply. The definition is reached through state, so **declare it with `--actor`**:

```bash
spel --program token send --fee-payer <FEE_PAYER> --root bob --signer bob --actor usdc -- \
  burn --amount 20
```

Without `--actor usdc` the transaction is refused and nothing burns (exit `2`). Burning more
than the balance is refused. Burning needs no authority, so a fixed-supply token can still
shrink.

## 10. Inspect

Reads an actor's state from the chain and decodes it through the IDL. No signature, no
transaction:

```bash
spel --program token inspect usdc        # {"Definition": {...}}
spel --program token inspect alice       # {"Holding": {...}}
spel inspect alice                       # every program holding state on alice, as hex
```

`u128` values print as strings (`"balance": "700"`), so JSON readers keep their precision.
An account with no token state prints `null`. A holding's `definition_id` names its token.

`inspect alice@token` works too, and `spel` refuses an IDL whose name doesn't match the
program: the token IDL would decode an NFT actor into plausible nonsense.

## 11. Other programs reading token state

A program can't read another program's state, so it asks: `ReadState`, sent to any token
actor, replies with that actor's raw state (`lee_core::program::StateReply`) to the sender,
which decodes `TokenState`. The reader must check that the reply comes from the token program
and the account it asked about. There is no reply without a sender, so as a transaction's root
message it is refused; use `inspect` instead.

## Message reference

Root = the actor the message is sent to. S = must authorize (`--signer`).

| Message | Root | S | Extra `--actor` | Fields |
|---|---|---|---|---|
| `new-definition` | empty definition | root, `holding` | | `--name`, `--total-supply`, `--mint-authority`, `--holding` |
| `initialize-holding` | empty holding | root | | `--definition` |
| `transfer` | sending holding | root | | `--to`, `--amount` |
| `burn` | holding | root | its definition | `--amount` |
| `mint` | definition (self authority) or the authority | root | | `--definition`, `--to`, `--amount` |
| `set-authority` | definition (self authority) or the authority | root | | `--definition`, `--new-authority` |
| `read-state` | any token actor | | | sent by programs, not transactions |

## Events

Each event is emitted by the actor whose state changed and names its accounts. Selectors are
`sha256("token::<Name>")[..8]`; the IDL lists them with their fields. `spel send` doesn't print
events yet.

| Event | When |
|---|---|
| `NewDefinition` | a token is created |
| `Transferred` | a holding is credited by another holding |
| `Minted` | the definition raises its supply |
| `Burned` | the definition lowers its supply |
| `AuthorityChanged` | the authority is rotated or renounced |

## Gotchas

- **Inclusion isn't success.** Check `spel send`'s exit code: `2` means the program refused,
  even though the transaction is in a block.
- **Initialize before you credit.** Transfers and mints to an account without a holding of the
  token are refused and roll back.
- **`burn` needs `--actor <definition>`.** Otherwise it is refused and nothing burns.
- **Mint where the authority is.** With an external authority, send `mint` and `set-authority`
  to the authority's account, not to the definition.
- **`Option` flags are required.** Pass `none` for no authority.
- **Renouncing is permanent.** `--new-authority none` can't be undone.
- **No decimals on chain.** Amounts are base units; clients agree on a scale out of band.
- **Redeploy ⇒ new program.** Tokens from an earlier deployment don't exist for the new one.
- **Use the token IDL for token actors.** `spel` refuses a mismatch when the program has a
  `spel.toml` name; with a raw program id it can't check.
- **Same wallet home everywhere.** `wallet` and `spel` must use the same
  `LEE_WALLET_HOME_DIR`.
