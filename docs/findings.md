
# Privacy coverage in LEZ programs

LEZ programs, ideally, are privacy agnostic. E.g., a program should work the same for public and private accounts. Currently, LEZ program integration tests only cover public accounts. This task, we expand the tests for LEZ programs to determine how adaptable (TODO-probably wrong word) LEZ programs are to selective privacy.


# Private account variants in LEE

LEE's private state supports (regular) accounts, PDAs and group owned accounts.

## Overview of (regular) private accounts

### Private account initialization
Regular private accounts can be initialized with or without knowledge of the account's nullifier secret key `nsk`. This results in two initialization "types": `PrivateUnauthorized` and `PrivateAuthorizedInit`.

- `PrivateUnauthorized`

    A special case for private accounts initialization that uses only public keys `npk` and `vpk`. Example: Alice can use Bob's keys (`npk`, `vpk`) and an `identifier` to send Bob a private transaction. Since Alice does not know the corresponding `nsk`, she is spend the resulting private account. E.g., Alice cannot authorize the transaction.

- `PrivateAuthorizedInit`
    Private account initialized using the account's `nsk` (and some `identifier`). This operation cannot be done by the a third-party (an entity that does not possess spending authority of the account).

### Private account update (`PrivateAuthorizedUpdate`)
Private account updates require knowledge of the account's `nsk`. E.g., Alice cannot update the private account that she initialized for Bob.

### Summary

|type | authorized | who can use |
|----|----|----|
| `PrivateUnauthorized`| &#10060; | anyone |
| `PrivateAuthorizedInit` | &#9989; | owner |
| `PrivateAuthorizedUpdate` | &#9989; | owner |

Only the account owner can (1) update their initialized account, and (2) use functions that require authorization with their account.

### Remark
- `PrivateUnauthorized` initialization is used for account initialization. `is_authorized = false` is a protection that does not seem crucial. Artifically, blocks some functions. (TODO: return to and shift to conclusions)

## Private PDA

Private PDAs spending is restrict by a specific program. E.g., an AMM pool has PDAs for liquidity definition and vaults (for Token A and Token B). A program sets `is_authorized = true` for an account (purported PDA) by checking the correctness of its `AccountId`.

- `AccountId` formulas:
    - Public: `hash(prefix || program_id || seed)`
    - Private: `hash(prefix || program_id || seed || npk || identifier)`

The difference in these PDA `AccountId` formulas prevents programs from being privacy agnostic for PDAs.

## Group-shared (multi-party) private accounts

A single private account can be jointly controlled by two or more parties without either one
handing over their actual secret key. The mechanism is a **Group Master Secret (GMS)**,
distributed via a real seal/unseal handshake (ML-KEM-768), not key reuse:

1. Alice creates a `GroupKeyHolder` and derives the shared account's keys (`nsk`, `vsk`)
   from it.
2. Alice **seals** the GMS against Bob's sealing public key and hands over only the sealed bytes.
3. Bob **unseals** it with his own sealing secret key, then
   independently re-derives the account's keys from the same seed.

This ensures that any member of the group can execute programs on shared accounts using either `PrivateAuthorizedInit` or `PrivateAuthorizedUpdate`. From a program's perspective, shared accounts should behave the same as regular public accounts.

# Privacy coverage for LEZ programs objectives (TODO)

In this task, we plan to add tests for e

|         | description |  |
|---------|----|----|
| PDA     |
| REGULAR |
| EXIST   |
| GROUP   |
| CHAIN   |

- Regular private accounts
- `PrivateUnauthorized` accounts; e.g., "transfer to existing accounts".
- Group shared private accounts
- Private PDAs.

# LEZ programs (TODO)

## AMM program

**Headline finding: no privacy-preserving test can be written for AMM's pool-mutating
instructions at all right now — not because of privacy, but a distinct circuit-level bug.**

Before any private-account test, an all-public control test through `execute_and_prove` (same
discipline that caught Stablecoin's `OpenPosition` bug) turned up a second, unrelated
circuit-level issue specific to AMM: `SwapExactInput` fails inside `execute_and_prove` with
`"Invalid account_identities length"` — we supply 8 account identities, the circuit's
`states_iter` only computes 7 — with every account `Public` and zero private accounts involved.
The same pattern reproduces on `SyncReserves` (6 vs 5). The account that silently vanishes from
the circuit trace is `CLOCK_01_PROGRAM_ACCOUNT_ID` — present in the top-level input and in the
AMM program's own returned `post_states` (confirmed in `sync.rs`/`swap.rs` source), but never
seen by the circuit at any call depth. Root cause not yet found.

Five tests confirm this **also blocks real private-account attempts**, not just the all-public
control case — `amm_swap_a_to_b_private_user_holding_is_not_expressible` and
`amm_swap_exact_output_private_user_holding_is_not_expressible` (private `user_holding_a`, 8 vs
7), `amm_add_liquidity_private_lp_holding_is_not_expressible` (private `user_holding_lp`, 10 vs
9), `amm_add_liquidity_private_user_holdings_is_not_expressible` (private `user_holding_a` +
`user_holding_b` deposit legs, 10 vs 9), `amm_remove_liquidity_private_lp_holding_is_not_expressible`
(private `user_holding_lp`, 10 vs 9) — all five fail with the identical
`"Invalid account_identities length"` panic, always exactly one account short. **Consequence**:
Swap (both variants), AddLiquidity, and RemoveLiquidity cannot be tested for any Q2 privacy
dimension until this circuit bug is fixed — every planned AMM privacy test is blocked on it. See
`docs/privacy-test-matrix.md`'s AMM section for the full bisection log.

**⚠ To track down later — confirmed `clock` is the account that vanishes, root cause still
open**: instrumented tracing (`eprintln!`s in the pinned `lee_core` checkout's
`execution_state.rs`, exact `Display`-string matching against `CLOCK_01_PROGRAM_ACCOUNT_ID`)
confirmed the circuit's internal per-account processing (`states_iter`) never contains an entry
for `clock`, at any call depth — not the top-level AMM call, not even inside the TWAP
`UpdateCurrentTick` chained call, which itself explicitly re-passes `clock.clone()`. Ruled out a
coincidental `AccountId` collision. **Still unknown**: whether the entry is dropped inside the
AMM guest's own execution, inside the SPEL-macro-generated `#[lez_program]` wrapper code, or
inside the circuit's own bookkeeping before `validate_and_sync_states`'s per-account loop even
runs. **Next concrete step**: check whether `pre_states.len()`/`post_states.len()` already
differ from N/N *before* that loop runs — that single check localizes the bug to one side or the
other and was never executed before this investigation was paused.

**A second, distinct finding for `RemoveLiquidity`, unrelated to the circuit bug above:**
`remove_liquidity` requires `user_holding_a`/`user_holding_b` to already exist and already be
owned by the configured Token Program (`remove.rs`'s
`assert_eq!(user_holding_a.account.program_owner, token_program_id, ...)`) — unlike
`token::transfer`'s recipient handling, which tolerates `Account::default()` and self-initializes
it. So `RemoveLiquidity` can never pay out to a brand-new private destination
(`PrivateUnauthorized` — only `npk` known, no `nsk`): the attempt
(`amm_remove_liquidity_private_new_user_holdings_is_not_expressible`) fails inside the AMM
program's own precondition check, *before* any chained call or the privacy-preserving circuit is
ever reached — and would equally reject a brand-new *public* destination. Same shape of finding
as Stablecoin's `stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`:
a plain program-level precondition that predates privacy entirely, not a circuit artifact.

## ATA program

ATA program offers limited usage with private accounts. Private accounts can be used as the `owner` (or as a recipient to transactions). But, ATA program can only generate public PDAs. The `owner` account can be public/private/shared and have any `program_owner`.

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| Create | `ata_create_from_private_owner` | BASE (private owner only; ATA account + definition public) | Any third party can bootstrap another owner's ATA using only that owner's public key material (`PrivateUnauthorized` — `npk`/`vpk` only, no `nsk`) — `Create` never asserts `owner.is_authorized` | ✅ |
| Create | `ata_create_private_ata_holding_is_not_expressible` | PDA | Attempts to make the ATA holding itself a private account via `PrivatePdaInit`/`PrivatePdaUpdate` — confirms the public-form PDA match ATA authorizes with and the private-form binding those variants require are mutually exclusive for the same account id | ❌ (confirmed not-expressible) |
| Create | `ata_create_from_group_owned_owner` | GROUP | Group-derived owner identity used to create an ATA — **weaker than the other `GROUP` rows**: `Create` never requires `owner` to prove control, so this can't demonstrate genuine shared control the way the `Transfer`/`Burn` rows below do; it only confirms `Create` doesn't secretly care where `npk`/`vpk` came from | ✅ (defensive/symmetry coverage only) |
| Transfer | `ata_transfer_to_existing_private_recipient` | EXIST, CHAIN | Sends more into an already-shielded private recipient through ATA's *nested* chained call into Token — the first test in the whole exercise proving a private identity survives a chained call at all | ✅ |
| Transfer | `ata_transfer_with_group_owned_owner_signing` | GROUP | Group-owned owner (real GMS seal/unseal handshake) signs `ATA::Transfer` as the required authorizing party | ✅ |
| Burn | `ata_group_owned_owner_signing` | GROUP | Group-owned owner signs `ATA::Burn` as the required authorizing party | ✅ |

**`PDA`** is confirmed not-expressible for every ATA instruction, not just `Create` — `Transfer`
and `Burn` call the same `ata_core::verify_ata_and_get_seed` function, so the identical
public-form/private-form conflict applies to them too, even though only `Create` has a dedicated
test asserting it.

Two tests exist outside this table's categories (not `PDA`/`GROUP`/`EXIST`/`CHAIN`, and not
`BASE` either — tagged `new: signer-authorization` in `docs/privacy-test-matrix.md`) and are
worth noting separately: `ata_burn_with_private_owner_signing` and
`ata_transfer_with_private_owner_signing` (a *personal*, non-group private owner signing
`Burn`/`Transfer`). They were the key discovery that `owner` must be a *signer* for these two
instructions (unlike `Create`) — a real finding, just a distinct dimension from any tag used
elsewhere in this table.

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
| Transfer | `token_shielded_transfer` | EXIST | A public sender shields tokens into a fresh private recipient (`PrivateUnauthorized` — only `npk`/`vpk` known, no `nsk`) | ✅ |
| Transfer | `token_private_transfer` | REGULAR -> EXIST | Two private accounts (sender via `PrivateAuthorizedUpdate` + fresh recipient via `PrivateUnauthorized`) compose in a single transaction with no public account at all — fulfills the "multiple private accounts in one tx" | ✅ |
| Transfer | `token_deshielded_transfer` | REGULAR | A private sender (`PrivateAuthorizedUpdate`) transfers out to a public recipient | ✅ |
| Transfer | `token_shielded_transfer_authorized_private_init` | REGULAR | Fresh recipient self-initializes via `PrivateAuthorizedInit` (own `nsk` supplied) instead of being passively credited via `PrivateUnauthorized` | ✅ |
| Transfer | `token_transfer_into_existing_private_holding` | REGULAR | Similar to `token_shielded_transfer_authorized_private_init`, but this shielded transaction does not initialize the private account. Second transfer into an already-shielded recipient — confirms crediting an existing private account requires the recipient's own cooperation (`nsk`), not just their public key | ✅ |
| Transfer | `token_private_transfer_into_existing_private_holding` | REGULAR -> REGULAR | Both legs private (sender + recipient) in one transaction, and the recipient is already existing rather than fresh | ✅ |
| Transfer | `token_group_owned_holding_shared_control_transfer` | GROUP -> EXIST | Group-owned sender (real GMS seal/unseal handshake) spends outward via `Transfer` to a fresh private recipient (`PrivateUnauthorized`) | ✅ |
| Mint | `token_mint_private_unauthorized` | EXIST | Mint directly to a fresh private recipient (self-authority signer + `PrivateUnauthorized` recipient) | ✅ |
| Mint | `token_mint_authorized_private_init` | REGULAR (authorized variant) | Mint to a fresh recipient that self-initializes via `PrivateAuthorizedInit` (own `nsk` supplied) instead of being passively credited | ✅ |
| Mint | `token_mint_into_existing_private_holding` | REGULAR | Mint once to establish a private holding, mint again into it via `PrivateAuthorizedUpdate` — crediting an existing private account | ✅ |
| Burn | `token_private_burn` | REGULAR | Burn from an existing private holding via a single `PrivateAuthorizedUpdate` | ✅ |
| Burn | `token_group_owned_holding_shared_control_burn` | GROUP | Shield tokens into a GMS-derived shared holding, then burn from it using an independently re-derived key | ✅ |
| InitializeAccount | `token_initialize_private_account_succeeds_for_canonical_definition` | REGULAR | Self-init of a private holding via `PrivateAuthorizedInit` | ✅ |
| InitializeAccount | `token_group_owned_holding_shared_control_initialize` | GROUP | A group member — not the party who created the group — self-initializes the shared holding directly via `PrivateAuthorizedInit` | ✅ |
| MintWithAuthority | `token_mint_with_authority_to_private_holding` | EXIST | External-authority mint (distinct signer from the definition) directly to a fresh private recipient | ✅ |

**`PDA`** has no Token-layer rows: Token holdings are addressed by an arbitrary `AccountId`, not
a program-derived one — there's no PDA to make private at this layer. Only testable once a
holding is wrapped by another program's PDA (ATA/AMM/Stablecoin).

**`CHAIN`**'s "carried through chained calls" half also has no Token-layer rows: Token issues no
`ChainedCall`s of its own (only ATA/AMM/Stablecoin do) — that half is exercised for the first
time in the ATA section instead.

|    | coverage? | explanation | 
|----|---------|----------------|
| REGULAR |  full | REGULAR private accounts are used as sender/recipient for initialize, transfer, mint and burn |
| GROUP   | full | Tested with initialize, transfer, mint and burn |
| EXIST   | partial | EXIST (`PrivateUnauthorized`) cannot be used with initialize due to `is_authorize = false` |
| PDA     | N/A  | Token program does not use PDAs |






# Conclusions

## Group shared private accounts
- Group shared accounts are authorized 

# Observations
- Programs can be made privacy agnostic for PDAs by adjusting private PDA `AccountId` formula to match the public variant. Unclear how to precisely handle this to ensure `AMM program` generates unique pools for token pairs (in public PDA case).
- A private PDA can be initialized and used for a program without using traditional PDA lifecycle. E.g., TODO(provide example from `token.rs`) 

# TODO

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

