
# LEE privacy

Similar to public accounts, private accounts can be regular (generated using based on user generated keys) and PDA. Additionally, private accounts can be shared by a group.

## Overview of (regular) private accounts

### Private account initialization
Regular private accounts initialization with or without knowledge of the account's nullifier secret key `nsk`.

- `PrivateUnauthorized`
    A special case for initializing private accounts using only `npk` and `vpk`.

    Use cases;
        Private donations. A user publishes public keys (`npk`, `vpk`) associated to a set of private account keys. A third party initializes a fresh private account using these keys (and some `identifier`). This initialization transaction does not require the corresponding `nsk`. Any future transactions with this private account must be performed by the account owner (using the `nsk`).
- `PrivateAuthorizedInit`
    Private account initialized using the account's `nsk` (and some `identifier`).

### Private account update (`PrivateAuthorizedUpdate`)
Regular private accounts are updated the same way. Knowledge of the account's `nsk` and other data that is used for the 

### Summary

|type | authorized | who can use |
|----|----|----|
| `PrivateUnauthorized`| &#10060; | anyone |
| `PrivateAuthorizedInit` | &#9989; | owner |
| `PrivateAuthorizedUpdate` | &#9989; | owner |

Only the account owner can (1) update their initialized account, and (2) use functions that require authorization with their account.

## Private PDA

### Private PDA vs public PDA
- `AccountId` formulas are different:
    - Public: `hash(prefix || program_id || seed)`
    - Private: `hash(prefix || program_id || seed || npk || identifier)`

The difference in these PDA `AccountId` formulas prevents programs from being privacy agnostic for PDAs.

## Group-shared (multi-party) private accounts (TODO)

A single private account can be jointly controlled by two or more parties without either one
handing over their actual secret key. The mechanism is a **Group Master Secret (GMS)**,
distributed via a real seal/unseal handshake (ML-KEM-768), not key reuse:

1. One party ("Alice") creates a `GroupKeyHolder` and derives the shared account's `npk`/`vpk`
   from it (`derive_keys_for_shared_account(seed)`).
2. Alice **seals** the GMS against a second party's ("Bob's") own sealing public key
   (`seal_for`) and hands over only the sealed bytes.
3. Bob **unseals** it with his own sealing secret key (`GroupKeyHolder::unseal`), then
   independently re-derives the *identical* `nsk`/`npk` from the same seed — without ever
   touching Alice's `GroupKeyHolder` object directly.

Bob's re-derived `nsk` then works in `PrivateAuthorizedInit`/`PrivateAuthorizedUpdate` exactly
like a personally-held key — confirmed indistinguishable from a personal account for every
instruction tried (spend, sign, self-initialize), across Token, ATA, and Stablecoin.

# Privacy testing objectives for LEZ programs (TODO)

- [ ] **Private PDAs used as program inputs across the above flows.**
      **Not achieved — structurally blocked, not a test gap.** Every program with PDAs (ATA,
      AMM, Stablecoin) derives them via `for_public_pda(program_id, seed)` only. The private
      formula, `for_private_pda(program_id, seed, npk, identifier)`, additionally requires an
      `npk` — but none of `ata_core`/`amm_core`/`stablecoin_core`'s seed-computation functions
      accept an `npk` today, so it's never reachable through these programs as coded. Confirmed
      empirically not-expressible for ATA (`ata_create_private_ata_holding_is_not_expressible`);
      the same root cause applies to AMM and Stablecoin (identical `for_public_pda`-only
      pattern, verified directly in their `*_core` crates). Token has no PDAs at all — N/A at
      that layer, not a gap.
      *Re: "could we compose a test program that uses private PDAs with these pre-existing?"* —
      no. None of the four existing programs can be made to produce a `for_private_pda` address
      through a test alone, since the formula choice is hardcoded in their source. Demonstrating
      the mechanism at all would require either changing one of the `*_core` crates to derive via
      `for_private_pda`, or standing up a small purpose-built program whose only job is to
      exercise it — both are source changes, not test-writing. **This is the single most
      actionable item to feed back to the protocol team.**

- Group owned shared private account as input to programs.

- [x] **Sending funds to an existing private account.**
      **Achieved, with one real condition: cooperation is required.** Confirmed across Token
      (`Transfer`, `Mint`), ATA (`Transfer`, including through a nested chained call into
      Token), and Stablecoin (`WithdrawCollateral`). Every path that touches an *existing*
      private account (`PrivateAuthorizedUpdate`) requires that account's own `nsk` plus a
      membership proof, supplied in the same transaction — there is no blind-credit analog to
      `PrivateUnauthorized` for existing accounts (only *fresh* accounts can be credited by a
      stranger). This isn't partial — it's a clean, fully-confirmed yes with one unavoidable,
      real-world condition: the recipient must be reachable to supply their `nsk` (online or
      pre-coordinated). That's a protocol/wallet-UX property to design around, not a bug or an
      untested edge.

- [~] **Multiple private accounts in one transaction, and private accounts carried through
      chained calls.** This is two separate sub-objectives with different status — worth
      splitting:
      - **Multiple private accounts in one tx — Achieved.** `token_private_transfer` (sender +
        recipient, both private, zero public accounts anywhere) and
        `token_private_transfer_into_existing_private_holding` (same, recipient already
        existing).
      - **Carried through a chained call — Achieved, but only single-hop so far.**
        `ata_transfer_to_existing_private_recipient` proves a private identity survives one
        chained call (ATA → Token) — the first test in the whole exercise to prove this works
        at all. Every private Stablecoin `WithdrawCollateral`/`RepayDebt` test also carries a
        private account through exactly one chained call (Stablecoin → Token). **Not yet
        tested:** deeper, multi-hop chaining — an instruction issuing more than one chained
        call with a private account threaded through it (e.g. AMM's `SwapExactInput` chains
        into *both* Token and the TWAP oracle in one instruction). That case is currently
        unreachable: AMM is blocked entirely by a separate, privacy-unrelated circuit bug (see
        the AMM section) before any chaining depth can even be exercised. So: not unclear —
        genuinely proven for the single-hop case, with the deeper case blocked pending AMM.


# LEZ programs (TODO)

## AMM program
TODO

## ATA program

ATA program offers limited usage with private accounts. Private accounts can be used as the `owner` (or as a recipient to transactions). But, ATA program can only generate public PDAs. The `owner` account can be public/private/shared and have any `program_owner`.

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| Create | `ata_create_private_ata_holding_is_not_expressible` | PDA | Attempts to make the ATA holding itself a private account via `PrivatePdaInit`/`PrivatePdaUpdate` — confirms the public-form PDA match ATA authorizes with and the private-form binding those variants require are mutually exclusive for the same account id | ❌ (confirmed not-expressible) |
| Create | `ata_create_from_group_owned_owner` | GROUP | Group-derived owner identity used to create an ATA — **weaker than the other `GROUP` rows**: `Create` never requires `owner` to prove control, so this can't demonstrate genuine shared control the way the `Transfer`/`Burn` rows below do; it only confirms `Create` doesn't secretly care where `npk`/`vpk` came from | ✅ (defensive/symmetry coverage only) |
| Transfer | `ata_transfer_to_existing_private_recipient` | EXIST, CHAIN | Sends more into an already-shielded private recipient through ATA's *nested* chained call into Token — the first test in the whole exercise proving a private identity survives a chained call at all | ✅ |
| Transfer | `ata_transfer_with_group_owned_owner_signing` | GROUP | Group-owned owner (real GMS seal/unseal handshake) signs `ATA::Transfer` as the required authorizing party | ✅ |
| Burn | `ata_group_owned_owner_signing` | GROUP | Group-owned owner signs `ATA::Burn` as the required authorizing party | ✅ |

**`PDA`** is confirmed not-expressible for every ATA instruction, not just `Create` — `Transfer`
and `Burn` call the same `ata_core::verify_ata_and_get_seed` function, so the identical
public-form/private-form conflict applies to them too, even though only `Create` has a dedicated
test asserting it.

Two tests exist outside this table's four categories and are worth noting separately:
`ata_burn_with_private_owner_signing` and `ata_transfer_with_private_owner_signing` (a
*personal*, non-group private owner signing `Burn`/`Transfer`). They were the key discovery that
`owner` must be a *signer* for these two instructions (unlike `Create`) — a real finding, just
not one of the four Q2 checkboxes, so it's omitted here the same way Token's `BASE` rows were.

## Stablecoin program

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| WithdrawCollateral | `stablecoin_withdraw_collateral_private_destination` | CHAIN, EXIST | Withdraws collateral through the single `Token::Transfer` chained call into an already-existing private destination holding | ✅ |
| WithdrawCollateral | `stablecoin_withdraw_collateral_group_owned_destination` | CHAIN, EXIST, GROUP | Same, but the destination holding is group-owned (real GMS seal/unseal handshake) | ✅ |
| WithdrawCollateral | `stablecoin_group_owned_position_owner` | GROUP | The position's `owner` identity itself (not the destination) is group-derived — proves shared authority over a CDP by withdrawing collateral through it | ✅ |
| RepayDebt | `stablecoin_repay_debt_private_stablecoin_holding` | CHAIN | Burns from a private stablecoin holding through the single `Token::Burn` chained call | ✅ |
| RepayDebt | `stablecoin_repay_debt_group_owned_stablecoin_holding` | CHAIN, GROUP | Same, group-owned holding | ✅ |

**`PDA`** has no rows, and can't even be isolated as its own question for this program: position
and vault are only ever PDA-claimed *inside* `OpenPosition`, and — see below — that instruction
can't reach the privacy circuit at all. The `PDA` question is subsumed by that finding rather
than independently testable; the ATA `PDA` finding (same `for_public_pda`-only root cause,
confirmed in `stablecoin_core`) stands as the citable reference.

One test sits outside this table's four categories but is the headline finding for the whole
program, worth stating plainly rather than omitting silently:
**`stablecoin_open_position_via_privacy_transaction_is_not_expressible`** — `OpenPosition`
cannot be executed through a privacy-preserving transaction *at all*, for any reason connected
to privacy. Confirmed with an all-public control case (every account `InputAccountIdentity::Public`,
zero private accounts) that fails identically, proving it's a protocol incompatibility in the
`PrivacyPreservingTransaction` code path itself, not a privacy bug — `owner`'s identity type is
irrelevant. Every test above routes around it by seeding position/vault directly rather than
calling `OpenPosition` for real.

**Root cause, precisely traced:** `open_position.rs` returns two *sibling* chained calls in one
shot (`vec![initialize_call, transfer_call]` — both discovered at once from a single execution of
`open_position`, neither nested inside the other) that both touch `vault`: `InitializeAccount`
declares it `is_authorized: true` (claimed via its PDA seed), `Transfer` then declares the *same*
account_id `is_authorized: false` (a hand-predicted post-`InitializeAccount` state, not a value
threaded through by the framework — the program author is predicting what call 1 will produce,
not observing it). This reuse of one account across two sibling calls with differing declared
authorization is the *only* thing that matters here — contrast with AMM's `remove_liquidity`,
which also returns multiple sibling chained calls at once (4: token A/B withdraw, LP burn, TWAP
tick update) but never reuses one account across two of them, so it never exercises this code
path at all.

Both transaction-type validators re-derive `is_authorized` per occurrence and assert it matches
the declared value — but they scope that derivation differently. `validated_state_diff.rs` (the
plain `PublicTransaction` validator) computes a fresh `authorized_accounts` set once per parent
call and clones it independently for each sibling *before* any sibling runs — so `Transfer`'s
view of `vault` never sees `InitializeAccount`'s PDA-based authorization, re-derives `false`,
matches. This is why the pre-existing public `stablecoin_open_position_then_withdraw_collateral`
test works. `execution_state.rs` (the `PrivacyPreservingTransaction`/circuit validator) instead
keeps one mutable `authorized_accounts: HashSet<AccountId>` on `self`, threaded with no
per-branch scoping through the entire flat call queue — `InitializeAccount` processing inserts
`vault` into it, and when `Transfer` is processed next, `resolve_authorization_and_record_bindings`
short-circuits via `if authorized_accounts.contains(&pre_account_id) { return true; }`, re-deriving
`true` — which conflicts with the declared `false` and fails
`assert_eq!(pre_is_authorized, is_authorized, "Inconsistent authorization for account {id}")`.

**This means `OpenPosition` is fixable two ways**: either scope `execution_state.rs`'s
`authorized_accounts` per sibling branch to match `validated_state_diff.rs`'s behavior (a circuit
fix, benefits every program with this pattern), or change `open_position.rs` to not re-declare
`vault` unauthorized on its second occurrence (a one-line fix local to this program, routing
around the bug rather than fixing it).

A second, unrelated negative result:
**`stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`** —
`WithdrawCollateral` cannot pay out to a brand-new private destination (`PrivateUnauthorized`,
only `npk` known, no `nsk`). `withdraw_collateral.rs` hard-asserts
`destination.account != Account::default()` before the chained `Token::Transfer` is even
constructed, so the destination must already exist — this is a plain program precondition, not a
privacy-circuit artifact, and would equally reject a withdraw to a brand-new *public*
destination. It's why every `WithdrawCollateral` test above uses `PrivateAuthorizedUpdate`
(`nsk` known) rather than `PrivateUnauthorized` for the destination.

## Token program

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| Transfer | `token_transfer_into_existing_private_holding` | EXIST | Second transfer into an already-shielded recipient — confirms crediting an existing private account requires the recipient's own cooperation (`nsk`), not just their public key | ✅ |
| Transfer | `token_private_transfer_into_existing_private_holding` | EXIST, CHAIN | Both legs private (sender + recipient) in one transaction, and the recipient is already existing rather than fresh | ✅ |
| Transfer | `token_group_owned_holding_shared_control_transfer` | GROUP | Group-owned sender (real GMS seal/unseal handshake) spends outward via `Transfer` to a fresh private recipient | ✅ |
| Transfer | `token_private_transfer` | CHAIN | Pre-existing test; two private accounts (sender + fresh recipient) compose in a single transaction with no public account at all — fulfills the "multiple private accounts in one tx" half of `CHAIN` | ✅ |
| Mint | `token_mint_into_existing_private_holding` | EXIST | Mint once to establish a private holding, mint again into it via `PrivateAuthorizedUpdate` — crediting an existing private account | ✅ |
| Burn | `token_group_owned_holding_shared_control_burn` | GROUP | Shield tokens into a GMS-derived shared holding, then burn from it using an independently re-derived key | ✅ |
| InitializeAccount | `token_group_owned_holding_shared_control_initialize` | GROUP | A group member — not the party who created the group — self-initializes the shared holding directly via `PrivateAuthorizedInit` | ✅ |

**`PDA`** has no Token-layer rows: Token holdings are addressed by an arbitrary `AccountId`, not
a program-derived one — there's no PDA to make private at this layer. Only testable once a
holding is wrapped by another program's PDA (ATA/AMM/Stablecoin).

**`CHAIN`**'s "carried through chained calls" half also has no Token-layer rows: Token issues no
`ChainedCall`s of its own (only ATA/AMM/Stablecoin do) — that half is exercised for the first
time in the ATA section instead.

# Conclusions

## Group shared private accounts
- Group shared accounts are authorized 

# Observations
- Programs can be made privacy agnostic for PDAs by adjusting private PDA `AccountId` formula to match the public variant. Unclear how to precisely handle this to ensure `AMM program` generates unique pools for token pairs (in public PDA case).
- A private PDA can be initialized and used for a program without using traditional PDA lifecycle. E.g., TODO(provide example from `token.rs`) 