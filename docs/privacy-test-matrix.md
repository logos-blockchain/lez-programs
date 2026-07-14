# Privacy test matrix (Q2 privacy validation)

Living tracker for the Q2 privacy-feature validation commitment: add privacy-preserving
variants of the existing `token` / `ata` / `amm` / `stablecoin` integration test flows, and
record which combinations work, fail, or cannot be expressed. Every row starting in
**Not started** should end the effort as **Pass** (test merged) or **Fail** /
**Not-expressible** (folded into `docs/privacy-gap-report.md` with root cause).

This is the tracking scaffold, not the final deliverable — `docs/privacy-gap-report.md` gets
written from the resolved state of this table.

## Key findings so far (highest priority — read this before anything else)

1. **`OpenPosition` cannot be called via a `PrivacyPreservingTransaction` at all**, for any
   reason related to privacy — confirmed with an all-public control case (zero private
   accounts, still fails identically). `open_position.rs` issues two chained calls that both
   reuse `vault`: `Token::InitializeAccount` authorizes it via `pda_seeds`, then
   `Token::Transfer` re-declares it `is_authorized: false` on its second occurrence (a
   legitimate choice on the public-transaction path, per that file's own comment). The privacy
   circuit's `authorized_accounts` bookkeeping is monotonic — once authorized, an account must
   stay declared `is_authorized: true` on every later occurrence — so this is rejected with
   `"Inconsistent authorization for account {id}"` (`lee_core`'s `execution_state.rs:301`).
   Likely fixable by not re-declaring `vault` unauthorized on its second occurrence. See
   `stablecoin_open_position_via_privacy_transaction_is_not_expressible` and the Stablecoin
   section below for the full writeup. **Single most actionable item for the protocol team.**
2. **Private PDAs are structurally impossible under every program's current derivation** — ATA,
   AMM, and Stablecoin all derive PDAs via `for_public_pda` only, which can never satisfy
   `PrivatePdaInit`/`PrivatePdaUpdate`'s binding requirement (traced precisely in
   `execution_state.rs`; see the ATA section). Fixable only by a source change to
   `for_private_pda` in each `*_core` crate.
3. **Sending to an existing private account requires the recipient's cooperation** — no
   "blind credit" path exists; confirmed across Token/ATA/Stablecoin instructions. Real
   wallet-UX implication, not a bug.
4. **Group-owned (shared) accounts work identically to personal ones** wherever tried —
   Transfer, Burn, InitializeAccount, and as the signing `owner` behind a PDA-locked resource
   (ATA, Stablecoin) — using the real seal/unseal GMS distribution, not just key reuse.
5. **AMM cannot be privacy-tested at all yet** — a *second*, distinct circuit-level issue
   blocks every pool-mutating AMM instruction (`Swap*`, `AddLiquidity`, `RemoveLiquidity`,
   `SyncReserves`) from the privacy-preserving transaction type, confirmed with all-public
   control tests (zero private accounts, still fails): `"Invalid account_identities length"`
   inside `execute_and_prove` itself. Ruled out "two different callee programs" as the cause
   (a TWAP-only instruction fails identically to a Token+TWAP one); leading unconfirmed
   suspect is AMM's pattern of passing an already-mutated `pool` copy into its chained TWAP
   call. Root-causing further requires the Docker-based guest rebuild pipeline (`make
   build-programs`), not plain `cargo test` — parked pending that investment. See the AMM
   section below for the full bisection trail.

## Legend

**Dimension** — which cross-cutting Q2 feature (or baseline coverage gap) a row exercises:

| Code | Meaning |
|---|---|
| `BASE` | Extends the already-proven single-private-account pattern (shield / private→private / deshield) to an instruction that has no private coverage yet. Not itself one of the four Q2 checkboxes. |
| `PDA` | Private PDAs used as program inputs |
| `GROUP` | Sharing a private account (group-owned) used as a program account |
| `EXIST` | Sending funds to an existing private account (not a fresh one) |
| `CHAIN` | Multiple private accounts in one transaction, and/or a private account carried through a `ChainedCall` |

**Priority** — `P1` build first, `P2` second wave, `P3` stretch/optional.

**Status** — `Not started` / `Pass` / `Fail` / `Not-expressible` / `Blocked`.

**Depends on** — which other deployed program(s) or new crate dependencies the row needs.

---

## Token (`token.rs`) — no program dependencies

Foundation layer. Has no PDAs of its own and issues no `ChainedCall`s, so `PDA` and `CHAIN`
don't apply here — it's the substrate the other three programs build on.

### Cross-cutting checkbox audit (end of Token phase, 2026-07-07)

Checked against the 4 Q2 checkboxes explicitly, not assumed:

| Checkbox | Status | Basis |
|---|---|---|
| Private PDAs used as program inputs | **N/A at this layer** | `token_core` has no `for_public_pda`/`for_private_pda` calls anywhere — Token holdings are addressed by arbitrary `AccountId`, not program-derived. Only testable once wrapped by another program's PDA (ATA/AMM/Stablecoin) — correctly deferred, not a gap in Token coverage. |
| Sharing a private account (group-owned) | **Covered** | `token_group_owned_holding_shared_control_burn`/`_transfer`/`_initialize` — see finding below. |
| Sending funds to an existing private account | **Covered** | `token_transfer_into_existing_private_holding` — see finding above. |
| Multiple private accounts in one tx / private accounts through chained calls | **Partially covered** | "Multiple private accounts in one tx" half: covered, but by the *pre-existing* `token_private_transfer` (two private legs, zero public), not by anything added this phase — none of the new tests this phase have more than one private leg. "Carried through chained calls" half: N/A at this layer, Token issues no `ChainedCall`s (only ATA/AMM/Stablecoin do); deferred. |

Net: of the 4 checkboxes, Token-phase work directly validated 2 (`EXIST`, `GROUP`), leaned on a
pre-existing test for half of a 3rd (`CHAIN`'s multi-account half), and the remaining checkbox
(`PDA`) plus the other half of `CHAIN` are structurally out of reach until ATA/AMM/Stablecoin
phases — not oversights specific to this phase.

**Update (2026-07-08):** the one remaining planned row, `token_mint_with_authority_to_private_holding`
(`BASE`, P3), passed — see the finding under Planned below. It doesn't move any of the 4
checkboxes above (it's `BASE`, not `PDA`/`GROUP`/`EXIST`/`CHAIN`), but it closes the last open
instruction/private-recipient combination at this layer. **Token phase is now complete.**

### Existing

| Instruction | Dimension | Test | Status |
|---|---|---|---|
| Transfer | BASE (shield) | `token_shielded_transfer` | Pass |
| Transfer | BASE (private→private) | `token_private_transfer` | Pass |
| Transfer | BASE (deshield) | `token_deshielded_transfer` | Pass |
| Transfer | BASE (authorized variant) | `token_shielded_transfer_authorized_private_init` — fresh recipient self-initializes via `PrivateAuthorizedInit` instead of being passively credited via `PrivateUnauthorized` | Pass |
| Mint | BASE | `token_mint_shielded` — mint directly to a fresh private recipient (self-authority signer + `PrivateUnauthorized` recipient) | Pass |
| Mint | BASE (authorized variant) | `token_mint_authorized_init` — mint to a fresh recipient that self-initializes via `PrivateAuthorizedInit` (own `nsk` supplied) instead of being passively credited | Pass |
| Burn | BASE | `token_private_burn` — burn from an existing private holding via a single `PrivateAuthorizedUpdate` | Pass |
| Transfer | `EXIST` | `token_transfer_into_existing_private_holding` — second transfer into an already-shielded recipient | Pass — **with a finding**, see below |
| Transfer | `EXIST` + `CHAIN` (fully private) | `token_private_transfer_into_existing_private_holding` — both legs private, recipient already existing (not fresh); two distinct accounts both via `PrivateAuthorizedUpdate` in one tx | Pass |
| InitializeAccount | BASE | `token_initialize_private_account` — self-init of a private holding via `PrivateAuthorizedInit` | Pass |
| InitializeAccount | new: self-service-only boundary | `token_initialize_private_account_without_nsk_is_not_expressible` | **Not-expressible — confirmed by design, not a gap** |
| Burn | `GROUP` | `token_group_owned_holding_shared_control_burn` — shield into a GMS-derived shared holding, burn from it via an independently-derived key | Pass |
| Transfer | `GROUP` | `token_group_owned_holding_shared_control_transfer` — group-owned sender spends outward via Transfer to a fresh private recipient, instead of destroying the funds via Burn | Pass |
| InitializeAccount | `GROUP` | `token_group_owned_holding_shared_control_initialize` — a group member (not the group's creator) self-initializes the shared holding directly via `PrivateAuthorizedInit` | Pass |
| Mint | `EXIST` | `token_mint_into_existing_private_holding` — mint once to establish the holding, mint again into it via `PrivateAuthorizedUpdate` | Pass |
| MintWithAuthority | BASE | `token_mint_with_authority_to_private_holding` — external-authority mint (distinct signer from the definition) directly to a fresh private recipient | Pass |

**Finding (`GROUP`, confirmed 2026-07-07):** sharing a private account genuinely works, and the test
was built to prove *sharing*, not just code reuse: "Alice" creates a `GroupKeyHolder` (fresh GMS)
and derives the shared account's npk/vpk via `derive_keys_for_shared_account`; she shields tokens
into it. The GMS is then distributed to "Bob" through the real `seal_for`/`unseal` ML-KEM-768
handshake — Bob never touches Alice's `GroupKeyHolder` object, only the sealed bytes. Bob
independently re-derives the identical nsk/npk from the unsealed GMS and successfully burns from
the shared holding using his own derivation. Required adding `key_protocol` as a new git dependency
(same repo/tag as `nssa`/`nssa_core`) to `integration_tests/Cargo.toml` — it wasn't previously a
dependency of `lez-programs`. Passed on the first attempt; no gap found for this dimension at the
Token layer.

**Finding (group-owned spend + self-init, confirmed 2026-07-07):** the `_burn` test only proved
group funds could be *destroyed*; `token_group_owned_holding_shared_control_transfer` closes
that gap by having Bob spend outward via `Transfer` to a fresh private recipient instead —
same seal/unseal rigor, both legs private (group sender via `PrivateAuthorizedUpdate`, fresh
recipient via `PrivateUnauthorized`), no public account anywhere in the transaction.
`token_group_owned_holding_shared_control_initialize` closes the other gap: a group *member*
(not the party who created the group) self-initializing the shared holding directly via
`InitializeAccount`/`PrivateAuthorizedInit`, rather than the holding only ever coming into
existence as a side effect of a shield. Both passed on the first attempt — group-owned
accounts behave identically to personal ones across every instruction tried so far.

**Finding (`EXIST`, confirmed 2026-07-07):** crediting an *existing* private account works, but only if the
recipient cooperates in the same transaction. Confirmed directly against `InputAccountIdentity`'s
doc comments and `output.rs` in `lee_core`: every variant that touches an existing private account
(`PrivateAuthorizedUpdate`, `PrivatePdaUpdate`) requires that account's own `nsk` + a membership
proof. There is no "blind credit" variant analogous to how any public account can be unilaterally
credited — a sender cannot push funds into an existing private account without the recipient
actively co-signing (supplying their nsk) in that same transaction. This is a real protocol/UX
property, not a bug: worth flagging to the privacy work as the answer to "can you send to an
existing private account" being **yes, but only cooperatively**, which has real wallet-UX
implications (recipient must be online / pre-coordinate, unlike a public transfer or a fresh
shield).

**Finding (`token_private_transfer_into_existing_private_holding`, confirmed 2026-07-07):**
fills the last open combination for Transfer — every prior private test had at most one
existing-and-private leg (`token_transfer_into_existing_private_holding`'s recipient) or a
fresh second leg (`token_private_transfer`'s recipient), never both legs private *and* the
recipient already existing. Two distinct private accounts, each independently proven via its
own `PrivateAuthorizedUpdate` (one spending, one crediting an existing balance), compose in a
single transaction with no public account anywhere — no signer, no public message ids at all.
Passed on the first attempt; built entirely on direct seeding (`with_private_accounts`) for
both sides, no real setup transactions needed.

**Finding (`token_mint_into_existing_private_holding`, confirmed 2026-07-07):** the `EXIST`
cooperation requirement generalizes across instructions, not just Transfer. `mint_inner`
already supports crediting an existing holding on the public side (branches on
`user_holding_account.account == Account::default()`); the private side needs the same
`PrivateAuthorizedUpdate` cooperation as Transfer — no instruction-specific escape hatch.
Passed on the first attempt once modeled on `token_transfer_into_existing_private_holding`.

### Planned

All originally-planned Token rows are now resolved (`token_mint_with_authority_to_private_holding`
passed — moved into the `Existing` table above) — Token phase is complete.

| Instruction | Dimension | Test | Priority | Depends on | Status |
|---|---|---|---|---|---|
| NewFungibleDefinition, NewDefinitionWithMetadata, SetAuthority(WithAuthority), PrintNft | — | **Not planned** — these operate on canonical, publicly-resolvable definitions/authorities; a "private token definition" has no coherent meaning since holders/traders must resolve it | — | — | Out of scope |

**Finding (`token_mint_with_authority_to_private_holding`, confirmed 2026-07-08):** closes the
last open Token combination — external-authority minting (`MintWithAuthority`, distinct signer
from the definition account) composed with a private recipient. Every prior `MintWithAuthority`
coverage minted to a public holder; every prior private-recipient mint test used self/PDA
authority (plain `Mint`). `mint_inner` never asserts `is_authorized` on `user_holding_account`
regardless of authority mode, so a passive `PrivateUnauthorized` recipient works here exactly as
it does under plain `Mint`. Passed on the first attempt after correcting the `Message`
construction: with two public accounts in the same privacy transaction (`definition`, not a
signer, plus `authority`, the signer), `public_account_ids` must list *both* — in their
`execute_and_prove` input order — for the circuit's public post-states to zip correctly, while
`nonces` lists *only* the signer(s), positionally matched to the witness keys (`signer_account_ids`
is derived from the witness set's public keys, not from `public_account_ids`). This is the first
test in the file with more than one public account alongside a private one, so it's worth
carrying forward: `public_account_ids` (post-state zipping) and `nonces` (signature/nonce
verification) are two independently-sized lists, not one shared list.

**Correction (`token_initialize_private_account`, resolved 2026-07-07):** originally flagged as a
plausible `Not-expressible` case because `initialize.rs` hard-asserts `is_authorized == true` while
a fresh account created via `PrivateUnauthorized` must be `false`. That flag was based on picking
the wrong identity variant, not a real protocol limit. `InitializeAccount`'s guest requires the
target to be a *signer* (`#[account(init, signer)]`) — i.e. self-initialization, the same shape as
`PrivateAuthorizedInit` (owner supplies their own `nsk` directly, `is_authorized: true` is
legitimate), not `PrivateUnauthorized` (third party credits an account they don't control, `nsk`
withheld, `is_authorized` must be `false`). Matching the identity variant to the instruction's
actual authorization shape resolved it cleanly — passed on the first attempt once corrected.

**Finding (self-service-only boundary, confirmed 2026-07-07 — prompted by a direct question,
not originally in the matrix):** can a third party initialize a private Token holding for an
`(npk, vpk, identifier)` whose `nsk` they don't possess? No — and this is a deliberate design
boundary, not a gap. Unlike `Transfer`/`Mint`, whose recipient-side host logic never asserts
`is_authorized` (which is exactly why third-party shielding into a fresh recipient works there
via `PrivateUnauthorized`), `InitializeAccount`'s guest declares `account_to_initialize` as
`#[account(init, signer)]`. Attempting it via `PrivateUnauthorized` (`is_authorized: false`,
no `nsk` needed) is rejected — empirically confirmed — at the SPEL macro's own account
validation layer ("`must be a signer`"), before `token_program::initialize::initialize_account`'s
own `is_authorized` assert is even reached. The only variant that can construct a fresh private
account here is `PrivateAuthorizedInit`, which requires supplying `nsk` directly. Net: this
instruction is self-service-only by construction — you can initialize your own private holding,
but not one on someone else's behalf without their key material. Worth carrying into the gap
report as a scoping note on `EXIST`/`BASE`, not a defect.

**Finding (`token_mint_authorized_init`, confirmed 2026-07-07):** the self-service-only
boundary above is specific to `InitializeAccount`, not a general rule about "authorized" private
identities. `Mint`'s guest marks `user_holding_account` as `#[account(mut)]` only (no
`signer`), and `mint_inner` never asserts `is_authorized` on it — confirmed by reading
`token/src/mint.rs` before writing the test, then verified empirically. So minting to a
recipient that self-initializes via `PrivateAuthorizedInit` (their own `nsk` supplied) works
just as well as `token_mint_shielded`'s passive `PrivateUnauthorized` recipient — passed on the
first attempt. Worth stating plainly in the gap report: whether a "self-authorized fresh
recipient" is accepted is instruction-specific (gated by that instruction's own signer
requirement), not a blanket protocol rule.

**Finding (`token_shielded_transfer_authorized_private_init`, confirmed 2026-07-07):** the same
`PrivateAuthorizedInit`-instead-of-`PrivateUnauthorized` variant generalizes to `Transfer` too,
closing the last instruction where every fresh-recipient test used only `PrivateUnauthorized`
(`token_shielded_transfer`, `token_private_transfer`'s new recipient, the group test's shield
step). `transfer.rs` asserts `is_authorized` only on the sender, never the recipient — same
shape as `Mint` — so this was expected and passed on the first attempt. Between this and the
`Mint`/`InitializeAccount` results, the picture is now complete: whether a fresh recipient can
choose to self-initialize (`PrivateAuthorizedInit`) instead of being passively credited
(`PrivateUnauthorized`) depends entirely on whether that instruction's guest marks the target
as a signer — true for `InitializeAccount` only (where `PrivateUnauthorized` is actually
rejected), optional for `Transfer`/`Mint` (both variants accepted).

---

## ATA (`ata.rs`) — depends on Token

### Existing

| Instruction | Dimension | Test | Status |
|---|---|---|---|
| Create | BASE (private owner only; ATA account + definition public) | `ata_create_from_private_owner` | Pass |
| Create | `PDA` | `ata_create_private_ata_holding_is_not_expressible` | **Not-expressible — confirmed** |

Verified in `ata/src/create.rs`: the owner account is **not** forwarded into the
`ChainedCall` to Token — only `token_definition` and the ATA holding are. So the existing
`ata_create_from_private_owner` test proves a private account can seed a PDA derivation and
appear as a top-level tx participant, but does **not** prove a private account traveling
through a chained call. That gap is still open despite appearances.

**Finding (third-party bootstrap, confirmed 2026-07-07 — positive finding, not a gap):**
`Create` never asserts `owner.is_authorized`, and the only private identity variant compatible
with an unauthorized owner (`PrivateUnauthorized`) structurally has no `nsk` field at all — it's
built from `npk`/`vpk` alone. So `ata_create_from_private_owner` demonstrates something worth
stating plainly rather than leaving implicit: **any third party can bootstrap another owner's
ATA using only that owner's public key material, without the owner ever exposing (or even
needing to possess yet) their `nsk`.** This mirrors Token's finding that anyone can shield funds
into a fresh private recipient who has never been online — here a wallet provider, faucet, or
counterparty program can pre-create a user's per-token account the same way, purely from public
inputs. The boundary is exactly where signing starts: the moment an instruction needs to *move*
value or prove ongoing control (`Transfer`, `Burn`), `nsk` becomes mandatory — see the
signer-authorization finding below.

**Finding (`PDA`, confirmed 2026-07-07 — root cause, not just an observation):** the ATA
holding can never be made a private account as ATA is currently coded, and this is a
structural fact provable from `lee_core`'s circuit source, not empirical friction. Traced
precisely: `Create`'s `ChainedCall.pda_seeds` authorizes Token to mutate
`for_public_pda(ata_program_id, seed)` — a match under the *public* PDA formula. In
`resolve_authorization_and_record_bindings` (`execution_state.rs`), a caller-seed match only
gets recorded into `private_pda_bound_positions` when it matches under `for_private_pda`
(`is_private_form == true`) — a public-form match authorizes the account but never binds it
as a private PDA. Every `PrivatePdaInit`/`PrivatePdaUpdate` identity requires its position to
appear in that binding map (hard `assert!` at `execution_state.rs:211`), and ATA's own
`verify_ata_and_get_seed` independently requires the account id to equal
`for_public_pda(ata_program_id, seed)` — never `for_private_pda`'s output, by construction of
two different hash domains. These two requirements are mutually exclusive for the same
account_id, full stop — confirmed empirically by attempting exactly this and getting the
precise, deterministic rejection (`ata_create_private_ata_holding_is_not_expressible`, which
asserts on the exact panic text).
**This generalizes**: AMM's vault/pool and Stablecoin's position/vault use the identical
`for_public_pda`-only derivation, so they will hit the *same* wall for the *same* reason — no
need to rediscover this per program, just confirm each one uses `for_public_pda` (already
verified for both in `amm_core`/`stablecoin_core`) and cite this finding. **The only fix** is a
source change to `ata_core`/`amm_core`/`stablecoin_core` to derive PDAs via `for_private_pda`
instead — out of scope for this test-writing task, but this is the single clearest, most
actionable item to feed back to the privacy/protocol work.

All originally-planned ATA rows are now resolved — see updated `Existing` table below. ATA phase
is complete.

| Instruction | Dimension | Test | Status |
|---|---|---|---|
| Transfer | `CHAIN` + `EXIST` (collapsed — see finding) | `ata_transfer_to_existing_private_recipient` | Pass |
| Burn | new: signer-authorization | `ata_burn_with_private_owner_signing` | Pass |
| Burn | `GROUP` + signer-authorization | `ata_group_owned_owner_signing` | Pass |
| Transfer | new: signer-authorization | `ata_transfer_with_private_owner_signing` | Pass |
| Transfer | `GROUP` + signer-authorization | `ata_transfer_with_group_owned_owner_signing` | Pass |
| Create | `GROUP` (defensive/symmetry only — see finding) | `ata_create_from_group_owned_owner` | Pass |

**Finding (`CHAIN` + `EXIST`, confirmed 2026-07-07):** `ata_program::transfer::transfer_from_associated_token_account`
hard-asserts `recipient.account != Account::default()` ("Recipient token holding must be
initialized"). That means a *fresh* private recipient (shield-style, `PrivateUnauthorized`) can
never be created through `ATA::Transfer` — only an already-existing account can be credited.
This collapses what the matrix originally planned as two separate rows (`BASE` and `EXIST`)
into one: `ata_transfer_to_existing_private_recipient` funds a private holding via a direct
(non-ATA) `Token::Transfer` shield first, then sends more into it through ATA's chained call,
with the recipient cooperating via `PrivateAuthorizedUpdate` (consistent with the Token-phase
`EXIST` finding). This is also the first test in the whole exercise where a private account
identity travels through a *nested* `ChainedCall` rather than a top-level instruction call —
and it worked on the first attempt, with no special handling needed.

**Finding (signer-authorization, confirmed 2026-07-07 — new angle, not in the original matrix):**
`Transfer`/`Burn` require `owner` to be a *signer* (`#[account(signer)]`), unlike `Create`
(merely `mut`). Every existing private-owner test only used owner passively (`Create`, no
signer requirement). `ata_burn_with_private_owner_signing` tests whether a private account can
satisfy a signer requirement by self-initializing *and* signing in the same transaction via
`PrivateAuthorizedInit` — it does, cleanly, on the first attempt. `ata_group_owned_owner_signing`
composes this with `GROUP`: the GMS is distributed through the real seal/unseal handshake (as
in `token_group_owned_holding_shared_control_burn`), and "Bob" — who never touches Alice's
`GroupKeyHolder` object — independently re-derives the matching nsk/npk and signs. Both pass.
Worth feeding back as a positive finding: private/shared accounts can serve as full signing
authorities for instructions that require it, not just as passive recipients.

**Follow-up (confirmed 2026-07-08 — closing a coverage review gap, not a new dimension):** a
review pass noticed `Burn` had both personal and group-owned signer coverage but `Transfer`
(identical `#[account(signer)]` requirement on `owner`) only had the pre-existing public-owner
test — a private owner had never actually been tried signing `ATA::Transfer`.
`ata_transfer_with_private_owner_signing` / `ata_transfer_with_group_owned_owner_signing` close
that gap directly, mirroring the `Burn` pair exactly (self-init + sign via `PrivateAuthorizedInit`,
personal and group-owned). Both passed on the first attempt, as expected given `Burn`'s identical
shape. Also added `ata_create_from_group_owned_owner` for symmetry — but **this one is a weaker
test by construction, not a gap closure**: `Create` places no signer requirement on `owner` at
all, and its only compatible private identity (`PrivateUnauthorized`) never touches `nsk`, so a
group-derived `owner` is indistinguishable from a personal one at this instruction. The test
confirms that empirically (nothing in `Create` secretly assumes anything about where `npk`/`vpk`
came from) but does **not** demonstrate genuine shared control the way the `Transfer`/`Burn`
group tests do — there is nothing for `Create` to prove sharing over, since it never asks anyone
to prove control of `owner` in the first place. Net: `Create`'s "group ownership" question isn't
an open gap, it's a category mismatch — worth stating that plainly in the gap report rather than
implying it was untested.

**Finding (ATA cannot originate a fresh private holding, confirmed 2026-07-08 — synthesizes two
separate facts above into one conclusion worth stating plainly): no ATA instruction can bring a
new private token holding into existence, for two independent reasons covering the two accounts
involved.** (1) The ATA's own holding can never be private at all — the confirmed `PDA` finding:
`Create` authorizes it via `for_public_pda` only, which can never satisfy
`PrivatePdaInit`/`PrivatePdaUpdate`'s binding requirement. (2) Even a separate, non-ATA private
recipient can't be freshly created through `ATA::Transfer` — `transfer_from_associated_token_account`
hard-asserts `recipient.account != Account::default()`, rejecting a shield-style fresh
`PrivateUnauthorized` recipient outright; only an *already-existing* recipient can be credited
(per the `CHAIN` + `EXIST` finding above). So ATA can send value *toward* a private destination,
but only one that already exists via some other path — every private holding that appears in
these tests was originated by a direct, non-ATA `Token` call
(`ata_transfer_to_existing_private_recipient`'s setup shields the recipient via `Token::Transfer`
before the ATA transfer under test ever runs). Worth stating as its own line in the gap report:
"ATA cannot emit private token holdings" is a real, structural limitation, not a coverage gap
in the tests written here.

---

## AMM (`amm.rs`) — depends on Token, TWAP oracle

33 public tests, 0 private. Confirmed in `amm_core`: all 5 PDAs (config, pool, vault×2,
liquidity-token, lp-lock) use `for_public_pda` exclusively.

Not every account is an equally meaningful privacy target: Pool/Config are the AMM's public
price surface (reserves must be readable to quote a swap; TWAP needs a continuously
observable tick) — privatizing them fights the AMM's purpose. Vault/LP-lock are the credible
middle case. User-held token/LP balances are the highest-value target.

### ⚠ Blocked pending investigation (2026-07-08) — read before starting AMM test-writing

Before writing any private AMM test, an all-public control test through `execute_and_prove`
(the same discipline that found Stablecoin's `OpenPosition` bug) turned up a **second,
distinct circuit-level issue specific to AMM**, unrelated to any privacy dimension. No AMM
privacy tests have been written yet — this needs resolving (or explicitly working around)
first.

**Symptom**: `SwapExactInput` (8 top-level accounts, 3 chained calls: 2×`Token::Transfer` +
1×`TWAP::UpdateCurrentTick`) fails *inside* `execute_and_prove`, before any private account is
even involved, with `"Invalid account_identities length"` (`lee_core`'s `output.rs:27`) —
`account_identities.len()` (8, what we supply) vs `states_iter.len()` (7, what the circuit
computes). Confirmed with every account `Public`.

**Bisection done so far**:
- **Ruled out "two different callee programs"**: `SyncReserves` (6 accounts, *one* chained
  call, into TWAP oracle only — zero Token calls) fails with the identical pattern (6 vs 5).
  So it's not about chaining into two different programs.
- **Ruled out "any multi-account reuse in one chained call"**: Stablecoin's
  `WithdrawCollateral` reuses *two* accounts (`vault`, `destination`) inside its single chained
  call and works fine — so plain reuse-of-multiple-accounts isn't sufficient on its own to
  trigger this.
- **Simplest AMM instruction works**: `UpdateConfig` (2 accounts, zero chained calls) gets
  *past* `execute_and_prove` cleanly — it fails later, at `transition_from_privacy_preserving_transaction`,
  with `InvalidInput("Empty commitments and empty nullifiers found in message")`. This looks
  like an unrelated, general protocol rule (a `PrivacyPreservingTransaction` needs at least one
  actual private account, or use `PublicTransaction` instead) — not a bug, but worth noting:
  **the "all-public control" methodology needs at least one trivial private leg to get past
  this check for future control tests**, not just all-`Public` identities.
- **Leading structural lead, superseded below**: every AMM instruction that hits the length
  mismatch passes a *post-update* copy of `pool` (`pool_price_source`, holding `pool_post` —
  the already-mutated state, not the original pre-state) into its chained TWAP call. This
  "pass what's about to become the post-state as the next call's own pre-state" pattern is
  proven correct on the public-transaction path (33 passing tests) but nothing in
  Token/ATA/Stablecoin ever exercised it under the privacy circuit. This was the leading lead
  at the time, but is likely **not** the real cause — see the more precise finding below, which
  identifies the specific missing account directly.
- **Precisely identified the missing account (2026-07-13)**: instrumented `execution_state.rs`'s
  per-account loop in `validate_and_sync_states` with `eprintln!` tracing (see below for how this
  was made to actually take effect) and confirmed via exact string-level `AccountId` matching
  that `CLOCK_01_PROGRAM_ACCOUNT_ID` is the account that vanishes — it's supplied as a top-level
  input and is clearly present in the AMM program's own returned `post_states` (confirmed
  directly in `sync.rs`'s `sync_reserves` and `swap.rs`'s `finalize_swap`, both of which
  explicitly include `AccountPostState::new(clock.account...)`), yet it never appears in the
  circuit-level trace at any call depth, not even inside the TWAP chained call which also
  explicitly passes `clock.clone()`. Root cause of *why* it's dropped was not yet found at this
  point — **since resolved, see "Root cause found" below**: it's a `spel-framework` guest-wrapper
  filter, not the circuit's own processing. Instrumentation was fully reverted afterward
  (verified byte-identical to the original checkout and original artifact) rather than left in
  place.
- **Confirmed this also blocks real private-account attempts, not just the all-public control
  case (2026-07-13)**: three tests — `amm_swap_a_to_b_private_user_holding_is_not_expressible`
  (private `user_holding_a`, 8 vs 7 accounts), `amm_add_liquidity_private_lp_holding_is_not_expressible`
  (private `user_holding_lp`, 10 vs 9), `amm_remove_liquidity_private_lp_holding_is_not_expressible`
  (private `user_holding_lp`, 10 vs 9) — all fail with the identical
  `"Invalid account_identities length"` panic, always exactly one account short. This rules out
  "the bug only manifests because there are zero private accounts" as an explanation; it's a
  structural property of these instructions' account/chained-call shape, independent of privacy
  entirely.

**How the instrumentation was made to actually take effect (2026-07-08 attempt failed, 2026-07-13
attempt succeeded)**: `eprintln!` tracing added directly to the pinned `lee_core` checkout's
`execution_state.rs` first appeared to have no effect — prints never surfaced, and the original
panic kept firing from the same file/line even after `cargo clean -p lee -p lee_core` and a fresh
compile. Root cause: real guest execution runs a separately cross-compiled RISC-V ELF
(`risc0_build::embed_methods!`), and the pinned `PRIVACY_PRESERVING_CIRCUIT_ELF` artifact is a
**pre-built, checked-in binary** (`artifacts/lee/privacy_preserving_circuit/privacy_preserving_circuit.bin`
in the checkout) embedded via `build_utils::include_artifacts` — editing the `.rs` source alone
never touches that binary. Fix: rebuild the guest ELF directly with
`cargo risczero build -p privacy_preserving_circuit_program --manifest-path <checkout>/Cargo.toml`
(matching the checkout's own `Justfile` `build-artifacts` recipe) and copy the result over the
checked-in `.bin` — **plus** `cargo clean -p lee -p lee_core` again afterward, since
`cargo:rerun-if-changed` was scoped to the artifacts *directory*, and overwriting a file's
content in place doesn't change the directory's own mtime, so cargo's incremental build silently
kept using the old compiled rlib (with the old bytes baked in via `include_bytes!`) even after
the file swap. Once both steps were done, the `eprintln!` output finally appeared and led
directly to the `CLOCK_01_PROGRAM_ACCOUNT_ID` finding above. All instrumentation (source edits,
rebuilt artifact) was fully reverted afterward and verified byte-identical to the original.

**Next step when this is picked back up**: check whether `output_pre_states.len()`/
`output_post_states.len()` already differ from 8/8 (or 6/6, etc.) *before* the per-account
validation loop in `validate_and_sync_states` runs — that would localize the drop to either the
AMM guest/SPEL-macro layer or the circuit's own processing, and is the next concrete step now
that instrumentation is confirmed to work end-to-end.

### ✅ Root cause found (2026-07-14) — it's in `spel-framework`, not `lee_core`, and not AMM's own code

Investigated (via a Fable 5 subagent, source-reading only — no instrumentation needed this time)
by comparing the two transaction validators side by side and checking the guest-wrapper code
that sits between AMM's own functions and either validator. Fully verified by direct inspection
afterward (both citations below reproduced and confirmed independently).

**The account is deleted before it ever reaches either validator.** The `#[lez_program]` macro's
generated `main()` — `spel-framework-macros/src/lib.rs:303-329`, in the pinned
`spel-393b37c2cff64018` checkout at rev `91023c9115bf88173b0d25d2e905f2a55ef0313b` — post-processes
every guest function's returned `(pre_states, post_states)` pairs before writing the
`ProgramOutput`:

```rust
// Filter out non-program-owned, non-default-state accounts from the output.
//
// LEZ validate_execution rule 7: if post.program_owner == DEFAULT_PROGRAM_ID
// and pre.account != Account::default(), validation fails. This would happen
// for signer accounts (e.g., proposer/executor) whose nonce has been incremented
// by a prior transaction — they are not owned by the program and must not be
// returned in the program's post-states.
.filter(|(pre, post)| {
    let is_default_owner = pre.account.program_owner == DEFAULT_PROGRAM_ID;
    let pre_is_default = pre.account == Account::default();
    let has_claim = post.required_claim().is_some();
    !is_default_owner || pre_is_default || has_claim
})
```

This was written to solve a real, narrow problem: drop *signer* accounts (proposer/executor)
whose nonce got bumped by a prior transaction, since they're not owned by the program and
`validate_execution`'s rule 7 would otherwise reject the output. But the predicate is broader
than that one case, and `clock` happens to satisfy it too:

- `is_default_owner = true` — the clock account is seeded via `force_insert_account` with
  `Account { data: <real clock bytes>, ..Account::default() }` (`advance_clock` in `amm.rs`),
  so its `program_owner` stays `DEFAULT_PROGRAM_ID` — it's never claimed by any program.
- `pre_is_default = false` — its `data` field holds real, non-default clock bytes.
- `has_claim = false` — AMM never issues a `Claim` for clock; it only reads it.

`!true || false || false` = `false` → the `(pre, post)` pair for `clock` is silently dropped from
`ProgramOutput.pre_states`/`post_states`, every single time, for every AMM instruction that
touches it — and for TWAP's `UpdateCurrentTick` too, since it's built with the exact same macro
at the exact same pin. This is exactly why the earlier `eprintln!` trace never saw `clock` at
*any* call depth, including inside the nested TWAP call: it was gone before the circuit ever got
the chance to see it, not dropped by the circuit itself.

**Why the public-transaction path never noticed**: `ValidatedStateDiff::from_public_transaction`
(`lee/state_machine/src/validated_state_diff.rs`) only ever iterates whatever the program's
*output* actually contains (`program_output.pre_states`) and zips it against
`program_output.post_states` to build the state diff. There is no check anywhere that the
output covers every account the *caller* originally supplied — a silently-dropped, unmodified
account just never appears in the diff, and nothing asserts it should have. `validate_execution`
(the rule 7 the filter comment refers to) only checks `pre_states.len() == post_states.len()`
*within* the already-filtered output (7 == 7 — passes trivially, since both sides of the pair
were dropped together).

**Why the privacy-preserving path panics**: the circuit builds its own account-tracking state as
the union of every `ProgramOutput.pre_states` it sees across the whole call tree — 7 accounts,
no clock. But the *caller* (the test, or in production a real wallet/client) must supply one
`InputAccountIdentity` per account it believes is involved — 8, including clock, since nothing
told the caller clock would be dropped. `compute_circuit_output`'s
`assert_eq!(account_identities.len(), states_iter.len())` (`output.rs:27`) then fails: `8 != 7`.
The public path tolerates exactly this same silent drop; only the private path's stricter
1:1 correspondence check turns it into a hard failure.

**This is a `spel-framework` bug, not a `lez_core`/circuit bug, and not an AMM program bug.**
Neither this repo's own code nor the pinned LEZ dependency is at fault — the defect is in the
`0x-r4bbit/spel` proc-macro crate's generated wrapper, one layer removed from both. Fix options
belong upstream: scope the filter to only the specific signer-nonce-bump case it was written for
(e.g. keep any pair the handler's own logic explicitly returned, rather than blanket-filtering
by ownership), or have the circuit tolerate identities without a corresponding output pre-state.
The trigger condition is narrow but real: any account with `program_owner == DEFAULT_PROGRAM_ID`
that a program reads but never claims will hit this — not just clock, and not just AMM. It just
happens to be clock here because every pool-mutating AMM instruction reads it.

**Soundness implication, not just a test-writing inconvenience**: because `clock` never reaches
`public_pre_states` on the privacy-preserving path, the host validator
(`check_privacy_preserving_circuit_proof_is_valid`) never checks the clock data a proof was
generated against against real chain state. A malicious prover could in principle supply an
arbitrary timestamp as a private witness and no check anywhere would catch it. Worth escalating
to the LEZ/SPEL maintainers independent of whether/when the AMM test-writing blocker itself gets
prioritized.

### Existing

6 private tests out of 33 pre-existing public + 6 = 39. No test can yet demonstrate an
actually-working AMM privacy path — five exist purely to confirm the circuit bug also blocks
real private accounts (not just the all-public control case), and one
(`amm_remove_liquidity_private_new_user_holdings_is_not_expressible`) found a second, distinct,
earlier blocker specific to `RemoveLiquidity`.

**Second finding, unrelated to the circuit bug (2026-07-13)**: `remove_liquidity` requires
`user_holding_a`/`user_holding_b` to already exist and already be owned by the configured Token
Program (`remove.rs`'s `assert_eq!(user_holding_a.account.program_owner, token_program_id, ...)`)
— unlike `token::transfer`'s recipient handling, which tolerates `Account::default()` and
self-initializes it. So `RemoveLiquidity` can never pay out to a brand-new private destination
(`PrivateUnauthorized` — only `npk` known, no `nsk`, the pattern
`token_mint_shielded_to_private_unauthorized` uses): the attempt
(`amm_remove_liquidity_private_new_user_holdings_is_not_expressible`) fails inside the AMM
program's own precondition check (`"User Token A holding must be owned by the configured Token
Program"`), *before* any chained call or the privacy-preserving circuit is ever reached — and
would equally reject a brand-new *public* destination. Same shape of finding as Stablecoin's
`stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`: a plain
program-level precondition that predates privacy entirely, not a circuit artifact.

### Planned

| Instruction | Dimension | Test | Priority | Depends on | Status |
|---|---|---|---|---|---|
| SwapExactInput | `CHAIN` | `amm_swap_a_to_b_private_user_holding_is_not_expressible` | P1 | Token, TWAP oracle (public leg) | **Confirmed not-expressible** — private `user_holding_a`, fails identically to the all-public control (8 vs 7 accounts) |
| SwapExactOutput | `CHAIN` | `amm_swap_exact_output_private_user_holding_is_not_expressible` | P1 | Token, TWAP oracle (public leg) | **Confirmed not-expressible** — identical 8-account/chained-call shape to `SwapExactInput`, fails identically (8 vs 7 accounts) |
| AddLiquidity | `CHAIN` | `amm_add_liquidity_private_user_holdings_is_not_expressible` — private deposit legs (`user_holding_a`/`user_holding_b`) | P1 | Token, TWAP oracle (public leg) | **Confirmed not-expressible** — fails identically (10 vs 9 accounts) |
| AddLiquidity | BASE | `amm_add_liquidity_private_lp_holding_is_not_expressible` — private LP output holding | P1 | Token | **Confirmed not-expressible** — private `user_holding_lp`, fails identically (10 vs 9 accounts) |
| RemoveLiquidity | `CHAIN` | `amm_remove_liquidity_private_lp_holding_is_not_expressible` | P1 | Token, TWAP oracle (public leg) | **Confirmed not-expressible** — private `user_holding_lp`, fails identically (10 vs 9 accounts) |
| RemoveLiquidity | `EXIST` (negative) | `amm_remove_liquidity_private_new_user_holdings_is_not_expressible` — brand-new `PrivateUnauthorized` token A/B destinations | P1 | Token | **Confirmed not-expressible for a different reason** — AMM's own precondition requires the destination to already be owned by the Token Program; fails before the circuit bug is even reached |
| Swap / AddLiquidity | `EXIST` | `amm_swap_into_existing_private_holding` | P2 | Token | **Blocked** — see above |
| NewDefinition | BASE | `amm_new_definition_private_initial_lp_holder` | P2 | Token | **Blocked** — see above (also issues chained calls reusing `pool`-derived accounts; check on resolution) |
| Swap / AddLiquidity (vault) | `PDA` | `amm_swap_with_private_vault_pda` — predicted **not-expressible** per the ATA `PDA` finding (same `for_public_pda`-only root cause, confirmed in `amm_core`); write as a quick confirmation citing that finding, not a fresh investigation | P2 | Token | Not started (also behind the blocker above) |
| AddLiquidity / RemoveLiquidity | `GROUP` | `amm_group_owned_lp_holding` | P3 | Token, `key_protocol` | **Blocked** — see above |
| Pool/Config (any) | `PDA` | `amm_attempt_private_pool_pda` — same predicted not-expressible outcome as above; low priority given the vault row already confirms the root cause for this program | P3 | Token | Not started |
| Initialize, UpdateConfig, CreatePriceObservations, CreateOraclePriceAccount, SyncReserves | — | **Not planned** — admin/infra instructions over public protocol state; a private admin authority is legitimate but low value | — | — | Out of scope (for now) |

Note: every Swap/AddLiquidity/RemoveLiquidity chains to *both* Token (transfers) and TWAP
oracle (tick refresh) in one instruction — so every `CHAIN` row above is automatically also
a "some legs private, some public" test. Call that out explicitly when the test is written,
not as an incidental detail. **All of these are currently blocked by the circuit-level issue
above, since it fires with zero private accounts involved — no privacy dimension can be tested
on any pool-mutating AMM instruction until it's resolved.**

---

## Stablecoin (`stablecoin.rs`) — depends on Token

2 pre-existing public tests (`stablecoin_open_position_then_withdraw_collateral`,
`stablecoin_repay_debt_burns_stablecoins_and_decreases_debt`). Both PDAs (position, position
vault) are `for_public_pda` only, per the ATA `PDA` finding.

### Existing

| Instruction | Dimension | Test | Status |
|---|---|---|---|
| OpenPosition | new: chained-call re-authorization | `stablecoin_open_position_via_privacy_transaction_is_not_expressible` | **Not-expressible — confirmed, root cause traced** |
| WithdrawCollateral | `CHAIN` + `EXIST` | `stablecoin_withdraw_collateral_private_destination` | Pass |
| WithdrawCollateral | `CHAIN` + `EXIST` + `GROUP` | `stablecoin_withdraw_collateral_group_owned_destination` | Pass |
| RepayDebt | `CHAIN` | `stablecoin_repay_debt_private_stablecoin_holding` | Pass |
| RepayDebt | `CHAIN` + `GROUP` | `stablecoin_repay_debt_group_owned_stablecoin_holding` | Pass |
| WithdrawCollateral (owner identity) | `GROUP` | `stablecoin_group_owned_position_owner` | Pass |
| WithdrawCollateral | new: destination must pre-exist | `stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible` | **Not-expressible — confirmed** |

**Finding (`OpenPosition`, confirmed 2026-07-08 — the headline finding for this program, and
arguably the whole exercise): `OpenPosition` cannot be executed through the privacy-preserving
transaction type at all, for any reason related to privacy.** Confirmed with an all-public
control test (every account `Public`, zero private accounts) that fails with the *identical*
error as the private attempt. Root cause traced precisely in `lee_core`'s
`execution_state.rs`: `authorized_accounts` is a monotonic/sticky set — once an account is
authorized via one chained call's `pda_seeds` match, every later occurrence of that same
account must *also* declare `is_authorized: true`, or
`assert_eq!(pre_is_authorized, is_authorized, "Inconsistent authorization for account {id}")`
fails. `open_position.rs` issues two chained calls that both reuse `vault`: the first
(`Token::InitializeAccount`) authorizes it via `pda_seeds`, sticking it as authorized; the
second (`Token::Transfer`) then deliberately constructs `post_init_vault` with
`is_authorized: false` — a legitimate choice on the public-transaction path (the file's own
comment: "the recipient is already initialized, so no second PDA claim is needed here") — but
the privacy circuit rejects that as inconsistent. **This means no privacy-preserving test can
ever open a position** — not because of anything about privacy, but because the instruction
itself is incompatible with the privacy transaction machinery as currently coded. Every test
below routes around it by seeding position/vault directly via `force_insert_account` (public
accounts, no real `OpenPosition` call), matching how the pre-existing public
`stablecoin_repay_debt_burns_stablecoins_and_decreases_debt` test already worked before this
phase. This is the single most actionable, most severe finding to feed back to the protocol
team — it blocks privacy for `OpenPosition` categorically, independent of the four Q2
dimensions, and is likely fixable by having `open_position.rs` mark `post_init_vault` as
authorized (or otherwise not re-declare it unauthorized) on its second occurrence.

**Consequence for the `PDA` dimension**: the originally-planned
`stablecoin_open_position_private_pda` confirmation test was dropped as redundant. Position and
vault are *only* ever claimed (via `Claim::Pda` and chained `pda_seeds` respectively) inside
`OpenPosition` — and since that instruction can't reach the privacy circuit at all, the `PDA`
question for Stablecoin can't even be isolated independently; it's subsumed by the finding
above. No separate test needed — the ATA `PDA` finding (same `for_public_pda`-only root cause)
still stands as the citable reference.

**Finding (`stablecoin_withdraw_collateral_private_destination` / `..._group_owned_destination`,
confirmed 2026-07-08):** unlike `OpenPosition`, `WithdrawCollateral` issues only *one* chained
call (`Token::Transfer`, reusing `vault` exactly once) — it doesn't hit the re-authorization
bug, and passed on the first attempt with a private, pre-existing destination (`EXIST`,
requiring the destination's `PrivateAuthorizedUpdate` cooperation per the Token/ATA-phase
finding) and again with a group-owned destination (real seal/unseal distribution, `GROUP`).

**Finding (`stablecoin_repay_debt_private_stablecoin_holding` / `..._group_owned_...`, confirmed
2026-07-08):** `RepayDebt` also has only one chained call (`Token::Burn`) and isn't affected by
the `OpenPosition` bug. `user_stablecoin_holding` is notably *not* PDA-locked (unlike ATA's own
holdings) — it's an ordinary user-controlled token holding — so it's free to be private with no
structural obstacle at all. Passed personal and group-owned variants on the first attempt.

**Finding (`stablecoin_group_owned_position_owner`, confirmed 2026-07-08 — reframes what
"group-owned position" means):** the position/vault themselves can never be private or
group-owned (the `PDA` finding), and can't even be *opened* through the privacy machinery (the
finding above) — but `owner` is just an `AccountId` used for PDA seed derivation and signer
verification, so it doesn't need to be a plain public keypair. Directly mirroring
`ata_group_owned_owner_signing`'s precedent: position/vault are seeded directly (bypassing the
blocked `OpenPosition`), keyed to a group-derived `owner` identity; "Bob" — who only ever
receives the sealed GMS — self-initializes *and* signs that owner identity in one transaction
via `PrivateAuthorizedInit`, then withdraws collateral through it. Passed on the first attempt.
This is the correct, expressible version of "joint control over a CDP": shared control of the
*authority* over a PDA-locked resource, not shared privacy of the resource itself.

**Finding (`stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`,
confirmed 2026-07-09 — second, unrelated not-expressible result for this program):**
`WithdrawCollateral` cannot pay out to a brand-new private destination. `withdraw_collateral.rs`
hard-asserts `destination.account != Account::default()` before the chained `Token::Transfer` is
even constructed — a plain host-side program precondition, unrelated to the `OpenPosition`
authorization-bookkeeping bug above. It fires regardless of privacy: a brand-new *public*
destination would be rejected identically. Confirmed by attempting `WithdrawCollateral` with a
`PrivateUnauthorized` destination (fresh `Account::default()` pre-state, only `npk` known) and
observing the exact `"Destination must be initialized"` panic surface as the circuit-execution
error. Consequence: every `WithdrawCollateral` test in this phase necessarily uses
`PrivateAuthorizedUpdate` (`nsk` known) for the destination — a pre-existing private destination
is the *only* expressible shape, not a coverage choice.

`ProtocolParameters` remains out of scope — not yet consumed by any instruction (no
freeze/admin logic wired up), nothing to test.

---

## Phase 0 prerequisites (blocking every remaining `GROUP` row)

- ~~Add `key_protocol` as a git dependency~~ — **done** (2026-07-07), added to
  `integration_tests/Cargo.toml` pinned to the same repo/tag as `nssa`/`nssa_core`. Unblocks the
  remaining `GROUP` rows in ATA/AMM/Stablecoin; each still needs its own program-specific test
  (PDA-based group ownership, not just the regular-account path proven for Token).
- Build the shared privacy test kit in `integration_tests/src/lib.rs` — **mostly done**
  (2026-07-08): `private_unauthorized_identity`/`private_authorized_init_identity`/
  `private_authorized_update_identity` (build an `InputAccountIdentity` from just the key
  material) and `GroupOwner` (the Alice-creates/Bob-admitted GMS handshake, via `::new(seed)` +
  `.admit_member()`) now live there and are used throughout `token.rs`, `stablecoin.rs` (fully
  migrated), and the newer `ata.rs` group tests. Only the original `ata_group_owned_owner_signing`
  still has its own independent inline copy — not yet migrated. Low priority; revisit
  before/during AMM if it's still outstanding then.

**Implementation technique worth carrying into AMM/Stablecoin (found 2026-07-07):** private
account preconditions don't need a real proven transaction to set up. `V03State::with_private_accounts(impl IntoIterator<Item = (Commitment, Nullifier)>)`
is a genuine, non-test-gated builder method — pair `Commitment::new(&id, &account)` with
`Nullifier::for_account_initialization(&id)` (the same pairing a real `PrivateUnauthorized`/
`PrivateAuthorizedInit` would have produced) and the seeded state is indistinguishable from a
real one to any subsequent transaction. Confirmed against `lee`'s own test suite pattern before
using it, then applied to refactor `token_private_burn`, `token_transfer_into_existing_private_holding`,
and `token_mint_into_existing_private_holding`'s setup legs — all still pass. Caveat: seeding
skips whatever *public*-side effect the bypassed transaction would have had (sender debit for a
shield, supply increase for a mint) — assertions on public state must account for that, matching
how public fixtures (`Accounts::holder_init()`) already set balances without a real mint ever
having produced them. This will matter more for AMM/Stablecoin, where setup transactions are
heavier (chained calls, multiple accounts) than a single shield.

## Row count summary

| Program | Existing private / confirmed | Planned rows | Out-of-scope instructions noted |
|---|---|---|---|
| Token | 16 (3 pre-existing + 13 new: 12 pass + 1 confirmed not-expressible by design) — phase complete | 0 | 5 |
| ATA | 8 (7 pass + 1 confirmed not-expressible — phase complete) | 0 | 0 |
| AMM | 6 (all confirmed not-expressible: Swap/SwapExactOutput/AddLiquidity (both LP and deposit legs)/RemoveLiquidity blocked by the same circuit bug, plus RemoveLiquidity's separate new-destination precondition; 2 further rows predicted not-expressible pending confirmation via the `PDA` finding) | 5 | 5 |
| Stablecoin | 7 (5 pass + 2 confirmed not-expressible — phase complete) | 0 | 1 |
