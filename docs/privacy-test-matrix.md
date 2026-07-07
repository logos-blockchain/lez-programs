# Privacy test matrix (Q2 privacy validation)

Living tracker for the Q2 privacy-feature validation commitment: add privacy-preserving
variants of the existing `token` / `ata` / `amm` / `stablecoin` integration test flows, and
record which combinations work, fail, or cannot be expressed. Every row starting in
**Not started** should end the effort as **Pass** (test merged) or **Fail** /
**Not-expressible** (folded into `docs/privacy-gap-report.md` with root cause).

This is the tracking scaffold, not the final deliverable — `docs/privacy-gap-report.md` gets
written from the resolved state of this table.

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
| Sharing a private account (group-owned) | **Covered** | `token_group_owned_holding_shared_control` — see finding below. |
| Sending funds to an existing private account | **Covered** | `token_transfer_into_existing_private_holding` — see finding above. |
| Multiple private accounts in one tx / private accounts through chained calls | **Partially covered** | "Multiple private accounts in one tx" half: covered, but by the *pre-existing* `token_private_transfer` (two private legs, zero public), not by anything added this phase — none of the new tests this phase have more than one private leg. "Carried through chained calls" half: N/A at this layer, Token issues no `ChainedCall`s (only ATA/AMM/Stablecoin do); deferred. |

Net: of the 4 checkboxes, Token-phase work directly validated 2 (`EXIST`, `GROUP`), leaned on a
pre-existing test for half of a 3rd (`CHAIN`'s multi-account half), and the remaining checkbox
(`PDA`) plus the other half of `CHAIN` are structurally out of reach until ATA/AMM/Stablecoin
phases — not oversights specific to this phase.

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
| Transfer + Burn | `GROUP` | `token_group_owned_holding_shared_control` — shield into a GMS-derived shared holding, spend from it via an independently-derived key | Pass |
| Mint | `EXIST` | `token_mint_into_existing_private_holding` — mint once to establish the holding, mint again into it via `PrivateAuthorizedUpdate` | Pass |

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

| Instruction | Dimension | Test | Priority | Depends on | Status |
|---|---|---|---|---|---|
| MintWithAuthority | BASE | `token_mint_with_authority_to_private_holding` | P3 | — | Not started |
| NewFungibleDefinition, NewDefinitionWithMetadata, SetAuthority(WithAuthority), PrintNft | — | **Not planned** — these operate on canonical, publicly-resolvable definitions/authorities; a "private token definition" has no coherent meaning since holders/traders must resolve it | — | — | Out of scope |

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
in `token_group_owned_holding_shared_control`), and "Bob" — who never touches Alice's
`GroupKeyHolder` object — independently re-derives the matching nsk/npk and signs. Both pass.
Worth feeding back as a positive finding: private/shared accounts can serve as full signing
authorities for instructions that require it, not just as passive recipients.

---

## AMM (`amm.rs`) — depends on Token, TWAP oracle

33 public tests, 0 private. Confirmed in `amm_core`: all 5 PDAs (config, pool, vault×2,
liquidity-token, lp-lock) use `for_public_pda` exclusively.

Not every account is an equally meaningful privacy target: Pool/Config are the AMM's public
price surface (reserves must be readable to quote a swap; TWAP needs a continuously
observable tick) — privatizing them fights the AMM's purpose. Vault/LP-lock are the credible
middle case. User-held token/LP balances are the highest-value target.

### Existing

0 private tests out of 33 public.

### Planned

| Instruction | Dimension | Test | Priority | Depends on | Status |
|---|---|---|---|---|---|
| SwapExactInput | `CHAIN` | `amm_swap_a_to_b_private_user_holding` | P1 | Token, TWAP oracle (public leg) | Not started |
| SwapExactOutput | `CHAIN` | `amm_swap_exact_output_private_user_holding` | P1 | Token, TWAP oracle (public leg) | Not started |
| AddLiquidity | `CHAIN` | `amm_add_liquidity_private_user_holdings` | P1 | Token, TWAP oracle (public leg) | Not started |
| AddLiquidity | BASE | `amm_add_liquidity_private_lp_holding` — private LP output holding | P1 | Token | Not started |
| RemoveLiquidity | `CHAIN` | `amm_remove_liquidity_private_lp_holding` | P1 | Token, TWAP oracle (public leg) | Not started |
| Swap / AddLiquidity | `EXIST` | `amm_swap_into_existing_private_holding` | P2 | Token | Not started |
| NewDefinition | BASE | `amm_new_definition_private_initial_lp_holder` | P2 | Token | Not started |
| Swap / AddLiquidity (vault) | `PDA` | `amm_swap_with_private_vault_pda` — predicted **not-expressible** per the ATA `PDA` finding (same `for_public_pda`-only root cause, confirmed in `amm_core`); write as a quick confirmation citing that finding, not a fresh investigation | P2 | Token | Not started |
| AddLiquidity / RemoveLiquidity | `GROUP` | `amm_group_owned_lp_holding` | P3 | Token, `key_protocol` | Not started |
| Pool/Config (any) | `PDA` | `amm_attempt_private_pool_pda` — same predicted not-expressible outcome as above; low priority given the vault row already confirms the root cause for this program | P3 | Token | Not started |
| Initialize, UpdateConfig, CreatePriceObservations, CreateOraclePriceAccount, SyncReserves | — | **Not planned** — admin/infra instructions over public protocol state; a private admin authority is legitimate but low value | — | — | Out of scope (for now) |

Note: every Swap/AddLiquidity/RemoveLiquidity chains to *both* Token (transfers) and TWAP
oracle (tick refresh) in one instruction — so every `CHAIN` row above is automatically also
a "some legs private, some public" test. Call that out explicitly when the test is written,
not as an incidental detail.

---

## Stablecoin (`stablecoin.rs`) — depends on Token

Only 2 tests total today (`stablecoin_open_position_then_withdraw_collateral`,
`stablecoin_repay_debt_burns_stablecoins_and_decreases_debt`), 0 private. Both PDAs
(position, position vault) are `for_public_pda` only.

Arguably the most naturally privacy-motivated program of the four — a CDP's collateral/debt
is exactly what a user would want hidden — despite having the thinnest existing baseline.

### Existing

0 private tests out of 2 public.

### Planned

| Instruction | Dimension | Test | Priority | Depends on | Status |
|---|---|---|---|---|---|
| OpenPosition | `CHAIN` | `stablecoin_open_position_private_collateral_holding` | P1 | Token | Not started |
| WithdrawCollateral | `CHAIN` | `stablecoin_withdraw_collateral_private_holding` | P1 | Token | Not started |
| RepayDebt | `CHAIN` | `stablecoin_repay_debt_private_holding` | P1 | Token | Not started |
| OpenPosition / Position + Vault | `PDA` | `stablecoin_open_position_private_pda` — predicted **not-expressible** per the ATA `PDA` finding (same `for_public_pda`-only root cause, confirmed in `stablecoin_core`); still worth writing as the clearest real-world case (a CDP position is the most natural thing to want private of anything in this whole exercise), but as a confirmation citing the root cause, not a fresh investigation | P1 (high value as *documentation* of the clearest case, even though the outcome is now predicted) | Token | Not started |
| OpenPosition / WithdrawCollateral | `EXIST` | `stablecoin_deposit_into_existing_private_holding` | P2 | Token | Not started |
| OpenPosition (joint CDP) | `GROUP` | `stablecoin_group_owned_position` | P3 | Token, `key_protocol` | Not started |
| (ProtocolParameters, any) | — | **Not planned** — not yet consumed by any instruction (no freeze/admin logic wired up); nothing to test | — | — | Out of scope |

---

## Phase 0 prerequisites (blocking every remaining `GROUP` row)

- ~~Add `key_protocol` as a git dependency~~ — **done** (2026-07-07), added to
  `integration_tests/Cargo.toml` pinned to the same repo/tag as `nssa`/`nssa_core`. Unblocks the
  remaining `GROUP` rows in ATA/AMM/Stablecoin; each still needs its own program-specific test
  (PDA-based group ownership, not just the regular-account path proven for Token).
- Build the shared privacy test kit in `integration_tests/src/lib.rs` (shield / spend /
  private-PDA fund-spend / group-derive helpers) — still not done. Tests so far (Token and ATA
  phases) are still hand-rolled per-file; revisit whether to extract shared helpers before AMM.

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
| Token | 13 (3 pre-existing + 10 new: 9 pass + 1 confirmed not-expressible by design) | 1 | 5 |
| ATA | 5 (4 pass + 1 confirmed not-expressible — phase complete) | 0 | 0 |
| AMM | 0 (2 rows now predicted not-expressible pending confirmation) | 10 | 5 |
| Stablecoin | 0 | 6 | 1 |
