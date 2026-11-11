# NFT program: spel runbook

How to use the **NFT** program end to end with `spel`: create an NFT with its metadata, give
accounts holdings, print copies, transfer copies and the master, and burn. Each section stands
alone; the only order is "create a definition before you use it".

The NFT program is separate from the fungible token program (`programs/token`,
[`docs/token/README.md`](../token/README.md)) and works the same way: the same actor model,
holdings that must be initialized before they receive, and the same `spel` workflow. This
runbook repeats what you need and points to the token runbook for the general parts.

**Verified against:** the NFT program on `refactor/lez-v0.4.x`, LEZ's actor model
(`moudy/lez-books` @ `616256b9c`), and the `spel` CLI from spel's `refactor/lez-v0.4.x`
(`dbb6aec`), on a local sequencer. `scripts/e2e-token-nft.sh` runs the main flows.

> **A program is its address.** Every deploy gets a new address, and it is a different
> program: NFT state lives on an account per program, so definitions and holdings created
> under one deployment don't exist for another. After a redeploy, update `spel.toml` and
> recreate your NFTs.

---

## Contents

- [0. Prerequisites](#0-prerequisites)
- [1. Build & deploy](#1-build--deploy)
- [2. The model](#2-the-model)
  - [Editions](#editions)
  - [State](#state)
  - [Access list, signers, outcomes](#access-list-signers-outcomes)
- [3. Create an NFT](#3-create-an-nft)
- [4. Give an account a holding](#4-give-an-account-a-holding)
- [5. Print a copy](#5-print-a-copy)
- [6. Transfer](#6-transfer)
- [7. Burn](#7-burn)
- [8. Inspect](#8-inspect)
- [9. Other programs reading NFT state](#9-other-programs-reading-nft-state)
- [Message reference](#message-reference)
- [Events](#events)
- [Gotchas](#gotchas)

---

## 0. Prerequisites

As for the token program ([token runbook §0](../token/README.md#0-prerequisites)): `spel` from
spel's `refactor/lez-v0.4.x` and LEZ's `wallet` on `PATH`, `LEE_WALLET_HOME_DIR` exported, a
funded fee payer, and the IDL at `artifacts/nft-idl.json` (`make idl`).

Accounts are wallet labels, base58 or hex; amounts are decimal integers. The one structured
argument is `--metadata`, a JSON object (quote it for the shell):

```json
{"standard": "Simple", "uri": "ipfs://…", "creators": "…"}
```

`standard` is `Simple` or `Expanded`. `spel --program nft -- <message> --help` lists a
message's fields with their documentation.

## 1. Build & deploy

```bash
make build-programs                                   # all guests → target/guest/<program>.bin
spel deploy target/guest/nft.bin --fee-payer <FEE_PAYER>
```

Point a `spel.toml` at the printed address. With the token program in the same file, pass
`--program nft` or `--program token` to pick one:

```toml
[programs.nft]
address = "<NFT_PROGRAM>"
idl     = "artifacts/nft-idl.json"
binary  = "target/guest/nft.bin"
```

The program has no configuration and no admin. Create one wallet account per role:

```bash
wallet account new public --label art      # the definition
wallet account new public --label alice    # the master
wallet account new public --label bob
wallet account new public --label dave
```

## 2. The model

### Editions

An NFT is a **definition** with a fixed `printable_supply` and its metadata, plus one
**master** holding that carries the print rights. The master prints **copies** into other
holdings, one at a time. The master's own unit is not printable: `printable_supply: N` allows
**N − 1** copies, and printing stops when the master's `print_balance` is 1.

Nothing ever adds supply. `printable_supply` counts the master's print rights plus the owned
copies, and only burns lower it.

### State

An NFT actor's state is one of:

| State | Fields |
|---|---|
| `Definition` | `name` (1–64 bytes), `printable_supply`, `metadata` (`standard`, `uri` ≤ 256 bytes, `creators` ≤ 256 bytes) |
| `Master` | `definition_id`, `print_balance` |
| `Copy` | `definition_id`, `owned` |

A holding **holds nothing** when it is a `Copy` with `owned: false` or a `Master` with
`print_balance: 0`. `initialize-holding` writes `Copy { owned: false }`; a transfer or burn
leaves the sender's holding in that state. Every credit (print or transfer) needs a holding of
the same definition that holds nothing, and fills it with what arrives: a copy or the master.

A holding belongs to one definition. An account has one NFT state per program, so a second
NFT needs another account, and an account's NFT and token holdings are separate actors
(`alice@nft`, `alice@token`).

### Access list, signers, outcomes

As in the token program ([token runbook §3](../token/README.md#3-the-model)):

- `spel send` declares the root, each `--actor`, and an actor for each account field
  (`--to`, `--definition`, `--master`) and each signer. **`burn` reaches the holding's
  definition through state: pass `--actor <definition>`.**
- `--signer` is explicit. The definition and the master sign at creation; a new holding signs
  its initialization; the sending holding signs transfer, print and burn. Recipients never
  sign.
- A refused transaction is still included: `spel send` exits `0` when state changed, `2`
  when nothing changed (refused, or a no-op), `1` when nothing was sent.

## 3. Create an NFT

The message goes to the empty **definition**. It writes the definition with the metadata and
creates the master with all of `printable_supply`. Both accounts sign.

```bash
spel --program nft send --fee-payer <FEE_PAYER> --root art --signer art --signer alice -- \
  new-definition --name Art --printable-supply 3 \
  --metadata '{"standard":"Simple","uri":"ipfs://…","creators":"me"}' --master alice
```

Result: `art` is the definition, `alice` holds `Master { print_balance: 3 }` and can print
two copies.

Refused: a non-empty definition or master, a missing signature, a name outside 1–64 bytes, a
`uri` or `creators` over 256 bytes, `--printable-supply 0`, and `--master` equal to the
definition.

## 4. Give an account a holding

Prints and transfers only credit an **initialized** holding of the same NFT. Create one with
`initialize-holding`, sent to the new holding, which signs:

```bash
spel --program nft send --fee-payer <FEE_PAYER> --root dave --signer dave -- \
  initialize-holding --definition art
```

The holding asks the definition, which answers, so a holding only exists for a definition that
does. The result is `Copy { owned: false }`, which can receive a copy or the master.

- The definition doesn't sign: anyone can open a holding of any NFT for an account they
  control.
- Repeating it for the same NFT is a no-op (exit `2`).
- An account that already holds another NFT, or is a definition, refuses.

## 5. Print a copy

Sent to the **master**, which signs. It lowers its `print_balance` by one and credits a new
copy to `--to`:

```bash
spel --program nft send --fee-payer <FEE_PAYER> --root alice --signer alice -- print-nft --to dave
```

- `--to` must be an initialized holding of this NFT that holds nothing (section 4). Otherwise
  the credit fails and the transaction rolls back (exit `2`).
- A master at `print_balance` 1 refuses to print.
- The recipient doesn't sign.

## 6. Transfer

Sent to the **sender's** holding, which signs. It moves its **whole value** to `--to`: a copy
moves itself, a master moves all of its print rights. There is no amount.

```bash
# a copy: dave → bob
spel --program nft send --fee-payer <FEE_PAYER> --root dave --signer dave -- transfer --to bob

# the master: alice → carol (carol initialized first)
spel --program nft send --fee-payer <FEE_PAYER> --root alice --signer alice -- transfer --to carol
```

- `--to` must hold nothing of this NFT; a holding with a copy or a master refuses.
- After a transfer the sender holds nothing: `Copy { owned: false }`, or `Master
  { print_balance: 0 }`. Either can receive again.
- A holding that holds nothing refuses to transfer.

## 7. Burn

Sent to the holding, which signs. It lowers its value and tells the definition to lower
`printable_supply`. The definition is reached through state, so **declare it with `--actor`**:

```bash
# an owned copy: --amount must be 1
spel --program nft send --fee-payer <FEE_PAYER> --root bob --signer bob --actor art -- burn --amount 1

# the master: any amount up to its print balance, which burns print rights
spel --program nft send --fee-payer <FEE_PAYER> --root alice --signer alice --actor art -- burn --amount 1
```

- Without `--actor art` the transaction is refused and nothing burns (exit `2`).
- A copy burns exactly 1 and must be owned. A master can burn down to 0, after which it
  can't print but can still receive.

## 8. Inspect

```bash
spel --program nft inspect art       # {"Definition": {..., "metadata": {...}}}
spel --program nft inspect alice     # {"Master": {"print_balance": "2", ...}}
spel --program nft inspect dave      # {"Copy": {"owned": true, ...}}
spel inspect alice                   # every program holding state on alice, as hex
```

`u128` values print as strings. An account with no NFT state prints `null`. `spel` refuses the
token IDL for an NFT actor and the other way round, when the program has a `spel.toml` name:
their layouts can decode into each other.

## 9. Other programs reading NFT state

As in the token program: `ReadState`, sent by another program to any NFT actor, replies with
the actor's raw state (`lee_core::program::StateReply`) to the sender, which decodes
`NftState` after checking the reply comes from the NFT program and the account it asked about.
As a transaction's root message it is refused; use `inspect` instead.

## Message reference

Root = the actor the message is sent to. S = must authorize (`--signer`).

| Message | Root | S | Extra `--actor` | Fields |
|---|---|---|---|---|
| `new-definition` | empty definition | root, `master` | | `--name`, `--printable-supply`, `--metadata`, `--master` |
| `initialize-holding` | empty holding | root | | `--definition` |
| `print-nft` | master | root | | `--to` |
| `transfer` | sending holding | root | | `--to` |
| `burn` | holding | root | its definition | `--amount` |
| `read-state` | any NFT actor | | | sent by programs, not transactions |

## Events

Each event is emitted by the actor whose state changed and names its accounts. Selectors are
`sha256("nft::<Name>")[..8]`; the IDL lists them with their fields. `spel send` doesn't print
events yet.

| Event | When |
|---|---|
| `NewDefinition` | an NFT is created |
| `Printed` | a copy is credited by the master |
| `Transferred` | a holding receives a copy or the master from another holding |
| `Burned` | the definition lowers its printable supply |

## Gotchas

- **`printable_supply` counts the master's unit.** `N` yields `N − 1` copies.
- **Initialize before you print or transfer.** Credits to an account without a holding of the
  NFT, or with one that already holds something, are refused and roll back.
- **Transfers move everything.** A master transfer hands over all print rights; there is no
  partial transfer.
- **`burn` needs `--actor <definition>`.** Otherwise it is refused and nothing burns.
- **One NFT per account per program.** A second NFT needs a second account.
- **Inclusion isn't success.** Check `spel send`'s exit code; `2` means refused.
- **Redeploy ⇒ new program.** NFTs from an earlier deployment don't exist for the new one.
