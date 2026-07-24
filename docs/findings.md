
# Privacy coverage in LEZ programs

LEZ programs, ideally, are privacy agnostic. E.g., a program should work the same for public and private accounts. Currently, LEZ program integration tests only cover public accounts. This task, we expand the tests for LEZ programs to determine how compatible LEZ programs are with selective privacy.


# Private account variants in LEE

LEE's private state supports (regular) accounts, PDAs and group owned accounts.

## Overview of (regular) private accounts

### Private account initialization
Regular private accounts can be initialized with or without knowledge of the account's nullifier secret key `nsk`. This results in two initialization "types": `PrivateUnauthorized` and `PrivateAuthorizedInit`.

- `PrivateUnauthorized`

    A special case for private accounts initialization that uses only public keys `npk` and `vpk`. Example: Alice can use Bob's keys (`npk`, `vpk`) and an `identifier` to send Bob a private transaction. Since Alice does not know the corresponding `nsk`, she is unable to spend the resulting private account. E.g., Alice cannot authorize the transaction.

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
- `is_authorized = false` is crucial for public accounts, since a public account id can be freely referenced and used by anyone in a transaction — `is_authorized` is what stops that. Private accounts don't have this exposure: only the account owner (via `nsk`) can ever update a private account, regardless of `is_authorized`. So for `PrivateUnauthorized` specifically — a third party initializing a *new* account on the owner's behalf, which nobody but the owner can subsequently update — `is_authorized = false` doesn't protect anything; it only blocks legitimate program functions that require a signer. This issue has been resolved by [PR 621](https://github.com/logos-blockchain/logos-execution-zone/pull/621); for more information see the Action items section.

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

This ensures that any member of the group can execute programs on shared accounts using either `PrivateAuthorizedInit` or `PrivateAuthorizedUpdate`. From a program's perspective, shared accounts should behave the same as regular private accounts.

# Privacy coverage for LEZ programs objectives

In this task, we extend testing for LEZ programs to cover privacy features:

|         | description |
|---------|----|
| PDA     | test checks for private PDA functionality. |
| REGULAR | private accounts usage using `nsk` |
| EXIST   | private account initialized without `nsk`; `PrivateUnauthorized` | 
| GROUP   | Shared group account |
| CHAIN   | private account used in a chain call |

A test function whose name ends in `_is_not_expressible` is a negative test: it demonstrates that
a desirable privacy pattern currently fails.

# LEZ programs

## AMM program

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| SwapExactInput | `amm_swap_a_to_b_private_user_holding` | REGULAR, CHAIN | Private `user_holding_a` deposit leg, through the Token + TWAP-oracle chained calls | ✅ |
| SwapExactOutput | `amm_swap_exact_output_private_user_holding` | REGULAR, CHAIN | Same coverage for `SwapExactOutput` | ✅ |
| AddLiquidity | `amm_add_liquidity_private_lp_holding` | REGULAR, CHAIN | Private LP-output holding (`user_holding_lp`) receives newly-minted LP on top of an existing private balance | ✅ |
| AddLiquidity | `amm_add_liquidity_private_user_holdings` | REGULAR, CHAIN | Private deposit legs (`user_holding_a` + `user_holding_b`), public LP recipient | ✅ |
| RemoveLiquidity | `amm_remove_liquidity_private_lp_holding` | REGULAR, CHAIN | Private LP holding (the account that signs/burns to remove liquidity) | ✅ |
| RemoveLiquidity | `amm_remove_liquidity_private_new_user_holdings_is_not_expressible` | EXIST, CHAIN | Brand-new `PrivateUnauthorized` token A/B destinations — rejected by a separate, unrelated program-level precondition (destination must already exist) | ❌ Not-expressible — AMM's own precondition requires the destination to already be owned by the Token Program. **[Open — Programs]** |
| SwapExactInput | `amm_swap_a_to_b_private_unauthorized_destination_is_not_expressible` | EXIST, CHAIN | Swap paying out to a brand-new `PrivateUnauthorized` destination (`npk` only, no `nsk`) | ❌ Not-expressible — the guest's signer check requires `is_authorized == true` on both swap legs, and `PrivateUnauthorized` always initializes with `is_authorized == false`. **[Resolved — PR #621]** |
| SwapExactInput | `amm_swap_a_to_b_private_authorized_init_destination_is_not_expressible` | REGULAR, CHAIN | Swap paying out to a brand-new `PrivateAuthorizedInit` destination (owner self-initializes with its own `nsk`) | ❌ Not-expressible — same "destination must already exist" precondition as `RemoveLiquidity`. **[Open — Programs]** |
| NewDefinition | `amm_new_definition_private_initial_lp_holder` | REGULAR | Pool creation with a private `PrivateAuthorizedInit` initial LP holder | ✅ |
| NewDefinition | `amm_new_definition_private_unauthorized_lp_holder_is_not_expressible` | EXIST, REGULAR | Pool creation with a `PrivateUnauthorized` initial LP holder (`npk` only, no `nsk`) | ❌ Not-expressible — same `is_authorized == true` signer check as the `Swap` row above, on `user_holding_lp`. **[Resolved — PR #621]** |

### Remarks
- `Swap` and `Remove` rejects any uninitialized destination account; this is a AMM design choice, and not Token program requirement.
- AMM's chained-call privacy tests were initially blocked by a "bug" in `logos-execution-zone`. An account was silently dropped from the programs output (pre and post states) before the transaction was validated. This behavior was acceptable in public transactions, but not for privacy transactions. This issue has been resolved by [PR 625](https://github.com/logos-blockchain/logos-execution-zone/pull/625); for more information see the Action items section.
    - The `clock` account was the offending account in the tests. For `integration_tests` the default account id was used which resulted in the account being dropped by LEZ. This behavior does not occur in practice as the `clock` account id is used for real. The AMM tests have been updated to avoid this issue for public and privacy tests.

## ATA program

ATA program offers limited usage with private accounts. Private accounts can be used as the `owner` (or as a recipient to transactions). But, ATA program can only generate public PDAs. The `owner` account can be public/private/shared and have any `program_owner`.

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| Create | `ata_create_from_private_owner` | REGULAR, EXIST | Any third party can bootstrap another owner's ATA using only that owner's public key material (`PrivateUnauthorized` — `npk`/`vpk` only, no `nsk`) — `Create` never asserts `owner.is_authorized` | ✅ |
| Create | `ata_create_private_ata_holding_is_not_expressible` | PDA | Attempts to make the ATA holding itself a private account via `PrivatePdaInit`/`PrivatePdaUpdate` — confirms the public-form PDA match ATA authorizes with and the private-form binding those variants require are mutually exclusive for the same account id | ❌ Not-expressible — `ata_core` derives the ATA holding's `AccountId` via the public-only formula, and the public/private formulas are mutually exclusive for the same account id. **[Open — Zones]**, a unified `AccountId` formula needs to be devised before programs can support private PDAs |
| Create | `ata_create_from_group_owned_owner` | GROUP | Group-derived owner identity used to create an ATA — **weaker than the other `GROUP` rows**: `Create` never requires `owner` to prove control. | ✅ (defensive/symmetry coverage only) |
| Transfer | `ata_transfer_to_existing_private_recipient` | REGULAR | Sends more into an already-shielded private recipient through ATA's *nested* chained call into Token — the first test in the whole exercise proving a private identity survives a chained call at all | ✅ |
| Transfer | `ata_transfer_with_private_owner_signing` | REGULAR | Key discovery: unlike `Create` (merely `mut`), `Transfer` requires `owner` to be a *signer* (`#[account(signer)]`) — a private owner self-initializes and signs in the same transaction via `PrivateAuthorizedInit` | ✅ |
| Transfer | `ata_transfer_with_group_owned_owner_signing` | GROUP | Group-owned owner (real GMS seal/unseal handshake) signs `ATA::Transfer` as the required authorizing party | ✅ |
| Burn | `ata_burn_with_private_owner_signing` | REGULAR | Same signer-authorization discovery as `ata_transfer_with_private_owner_signing`, for `Burn` | ✅ |
| Burn | `ata_group_owned_owner_signing` | GROUP | Group-owned owner signs `ATA::Burn` as the required authorizing party | ✅ |

### Remarks
- Transfer explicitly blocks `PrivateUnauthorized`and `PrivateAuthorizedInit`. ATA's transfer checks that the recipient's account is non-default. E.g., ATA can not transfer funds to a third-party's private account.
- ATA does not permit the creation of private token accounts. E.g., ATA only emits public PDA accounts. This is based on the PDA `AccountId` formulas used.

## Stablecoin program

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| WithdrawCollateral | `stablecoin_withdraw_collateral_private_destination` | REGULAR | Withdraws collateral through the single `Token::Transfer` chained call into an already-existing private destination holding | ✅ |
| WithdrawCollateral | `stablecoin_withdraw_collateral_group_owned_destination` | EXIST, GROUP | Same, but the destination holding is group-owned (real GMS seal/unseal handshake) | ✅ |
| WithdrawCollateral | `stablecoin_group_owned_position_owner` | GROUP | The position's `owner` identity itself (not the destination) is group-derived — proves shared authority over a CDP by withdrawing collateral through it | ✅ |
| RepayDebt | `stablecoin_repay_debt_private_stablecoin_holding` | REGULAR | Burns from a private stablecoin holding through the single `Token::Burn` chained call | ✅ |
| RepayDebt | `stablecoin_repay_debt_group_owned_stablecoin_holding` | GROUP | Same, group-owned holding | ✅ |

### Remarks
- `OpenPosition` is blocked for use in privacy transactions due to the chained calls usage. `OpenPosition` calls `Token::InitializeAccount` and `Token::Transfer` for the same vault account which is disallowed behavior in privacy preserving circuit. Demonstrated with test `stablecoin_open_position_via_privacy_transaction_is_not_expressible`. **[Open — Zones]**, see Conclusions.
- `WithdrawCollateral` does not support withdrawals to `PrivateUnauthorized` and `PrivateAuthorizedInit`; explicitly checks that the destination account is not default. Demonstrated with the test `stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`. **[Open — Programs]**
- Vault is explicitly public PDA by formula requirement. **[Open — Zones]**, same `AccountId` formula issue as ATA's — see the Action items section.

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
| InitializeAccount | `token_initialize_private_account_without_nsk_is_not_expressible` | EXIST | `InitializeAccount`'s target is `#[account(init, signer)]` — the guest's signer check requires `is_authorized == true`, and a third party initializing via `PrivateUnauthorized` (no `nsk`) always gets `is_authorized == false`, rejected before the program's own logic runs | ❌ Not-expressible. **[Resolved — PR #621]** |
| InitializeAccount | `token_group_owned_holding_shared_control_initialize` | GROUP | A group member — not the party who created the group — self-initializes the shared holding directly via `PrivateAuthorizedInit` | ✅ |
| MintWithAuthority | `token_mint_with_authority_to_private_holding` | EXIST | External-authority mint (distinct signer from the definition) directly to a fresh private recipient | ✅ |
| NewFungibleDefinition | `token_new_fungible_definition_private_initial_holder` | REGULAR | Public token definition, private initial holder that self-initializes via `PrivateAuthorizedInit` (own `nsk` supplied) — same self-service shape as `InitializeAccount`'s target | ✅ |
| NewFungibleDefinition | `token_new_fungible_definition_private_holder_without_nsk_is_not_expressible` | EXIST | The initial holder cannot be created via `PrivateUnauthorized` — same `is_authorized == true` signer check as `InitializeAccount` above, rejected before the program's own logic runs | ❌ Not-expressible. **[Resolved — PR #621]** |


### Remarks
- `Initialization` is not possible for `PrivateUnauthorized` accounts due to `is_authorized = false`. **[Resolved — PR #621]**
- New token definition is not permitted for `PrivateUnauthorized` as Token holding due to `is_authorized = false`. E.g., both Token Definition and Token Holding for a new Token must be from an authorized account. **[Resolved — PR #621]**

# Conclusions

Privacy coverage for LEZ program tests is greatly improved from the added tests. Though, there are a few noticable gaps:
- `PrivateUnauthorized` accounts can be blocked by programs with a check requiring `is_authorized = true`, since a fresh `PrivateUnauthorized` account is always initialized with `is_authorized = false`. This is a `logos-execution-zone` protocol-level issue, not something programs can work around — see the `PrivateUnauthorized` remark above for the full reasoning. This has been resolved by [PR 621](https://github.com/logos-blockchain/logos-execution-zone/pull/621); see the Action items section.
- Privacy transactions have issues with chain calls in which multiple calls affect the same private account. The privacy preserving circuit's `authorized_accounts` bookkeeping is monotonic (once an account is authorized, every later occurrence within the same transaction must also declare it authorized), which rejects some call patterns that are valid on the public-transaction path (E.g. `Stablecoin::OpenPosition`). This is a `logos-execution-zone` protocol level issue. A proposed revision to account updates would mitigate this issue: accounts updated iteratively based on their state diff rather than "full replacement".

Additionally, testing undercovered a "bug" in LEZ:
AMM's chained-call privacy tests were initially blocked because the `clock` account — seeded in the test fixture with a default (unclaimed) account id, `DEFAULT_PROGRAM_ID`, rather than owned by a dedicated clock program as in production — was silently dropped from a program's output, undetected by `logos-execution-zone`'s public-transaction validation: `ValidatedStateDiff::from_public_transaction` never checked that the accounts touched in a program's output matched the caller-declared `message.account_ids` — no count check, no membership check, nothing like the privacy circuit's own `account_identities.len() == states_iter.len()` assertion. That gap is why no pre-existing public AMM test ever caught the clock account being dropped: the public path had no validation capable of catching a silently-dropped account at all; only the privacy circuit's stricter bookkeeping turned it into a hard failure. Worked around at the test-fixture level in the meantime (giving the fixture's clock account a non-default owner; see the AMM section). This also had a soundness implication beyond blocking AMM tests: because `clock` never reached `public_pre_states` on the privacy-preserving path, the host validator never checked the clock data a proof was generated against real chain state — a malicious prover could in principle have supplied an arbitrary timestamp as a private witness with nothing to catch it. This has been resolved by [PR 625](https://github.com/logos-blockchain/logos-execution-zone/pull/625), which closes both the test-blocking symptom and the soundness gap; see the Action items section.

## Action items

Every open or resolved issue raised in this report, grouped by which repo the fix lives in —
**Zones** owns `logos-execution-zone`, **Programs** owns `lez-programs`:

| Owner | Item | Status |
|---|---|---|
| Zones | `is_authorized = false` blocks `PrivateUnauthorized` on signer-gated instructions | ✅ Resolved — [PR #621](https://github.com/logos-blockchain/logos-execution-zone/pull/621) |
| Zones | Public transactions don't detect a silently-dropped declared account | ✅ Resolved — [PR #625](https://github.com/logos-blockchain/logos-execution-zone/pull/625) |
| Zones | `OpenPosition`-shaped chain calls (re-authorizing the same account across chained calls) fail under the privacy circuit | 🔲 Open — proposed circuit revision, no PR yet |
| Zones | ATA/AMM/Stablecoin PDAs derive via `for_public_pda` only, blocking private PDA use — the public and private `AccountId` formulas are mutually exclusive for the same account, so a unified formula needs to be devised before programs can adopt it | 🔲 Open |
| Programs | AMM/Stablecoin reject any destination account that isn't already initialized, blocking fresh private destinations (3 tests) | 🔲 Open |
| Programs | ATA's `Transfer` rejects a non-default (fresh) recipient, blocking shield-style transfers into a brand-new private destination | 🔲 Open |

Resolved items from Zones land in `dev` branch of `logos-execution-zone`.