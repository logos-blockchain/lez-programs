
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
- `PrivateUnauthorized` initialization is used for account initialization. `is_authorized = false` is a protection that does not seem crucial. Artifically, blocks some functions.

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

# Privacy coverage for LEZ programs objectives

In this task, we extend testing for LEZ programs to cover privacy features:

|         | description |
|---------|----|
| PDA     | test checks for private PDA functionality. |
| REGULAR | private accounts usage using `nsk` |
| EXIST   | private account initialized without `nsk`; `PrivateUnauthorized` | 
| GROUP   | Shared group account |
| CHAIN   | private account used in a chain call |

# LEZ programs

## AMM program (unusual issues)

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| SwapExactInput | `amm_swap_a_to_b_private_user_holding_is_not_expressible` | REGULAR, CHAIN | Private `user_holding_a` deposit leg — confirms the circuit-level account-count bug also fires with a real private account (8 vs 7 accounts), not just the all-public control case | ❌ (confirmed not-expressible — circuit bug) |
| SwapExactOutput | `amm_swap_exact_output_private_user_holding_is_not_expressible` | REGULAR, CHAIN | Same confirmation for `SwapExactOutput` — identical account/chained-call shape to `SwapExactInput` (8 vs 7 accounts) | ❌ (confirmed not-expressible — circuit bug) |
| AddLiquidity | `amm_add_liquidity_private_lp_holding_is_not_expressible` | REGULAR, CHAIN | Private LP-output holding (`user_holding_lp`) — same circuit bug (10 vs 9 accounts) | ❌ (confirmed not-expressible — circuit bug) |
| AddLiquidity | `amm_add_liquidity_private_user_holdings_is_not_expressible` | REGULAR, CHAIN | Private deposit legs (`user_holding_a` + `user_holding_b`) — same circuit bug (10 vs 9 accounts) | ❌ (confirmed not-expressible — circuit bug) |
| RemoveLiquidity | `amm_remove_liquidity_private_lp_holding_is_not_expressible` | REGULAR, CHAIN | Private LP holding (the account that signs/burns to remove liquidity) — same circuit bug (10 vs 9 accounts) | ❌ (confirmed not-expressible — circuit bug) |
| RemoveLiquidity | `amm_remove_liquidity_private_new_user_holdings_is_not_expressible` | EXIST, CHAIN | Brand-new `PrivateUnauthorized` token A/B destinations — rejected by a separate, unrelated program-level precondition (destination must already exist) before the circuit bug is even reached | ❌ (confirmed not-expressible — different reason) |

### Remarks
- `RemoveLiquidity` and `Swap`s may have issues with `PrivateUnauthorized` and `PrivateAuthorizedInit` that match issues detected in Stablecoin; e.g., explicitly requires `is_authorized = true` and non default accounts.
- `clock` account issue: clock is silent dropped during privacy executions.

## ATA program

ATA program offers limited usage with private accounts. Private accounts can be used as the `owner` (or as a recipient to transactions). But, ATA program can only generate public PDAs. The `owner` account can be public/private/shared and have any `program_owner`.

| Function tested | Test name | Category | Description of objective | Result |
|---|---|---|---|---|
| Create | `ata_create_from_private_owner` | REGULAR, EXIST | Any third party can bootstrap another owner's ATA using only that owner's public key material (`PrivateUnauthorized` — `npk`/`vpk` only, no `nsk`) — `Create` never asserts `owner.is_authorized` | ✅ |
| Create | `ata_create_private_ata_holding_is_not_expressible` | PDA | Attempts to make the ATA holding itself a private account via `PrivatePdaInit`/`PrivatePdaUpdate` — confirms the public-form PDA match ATA authorizes with and the private-form binding those variants require are mutually exclusive for the same account id | ❌ (confirmed not-expressible) |
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
- `OpenPosition` is blocked for use in privacy transactions due to the chained calls usage. `OpenPosition` calls `Token::InitializeAccount` and `Token::Transfer` for the same vault account which is disallowed behavior in privacy preserving circuit. Demonstrated with test `stablecoin_open_position_via_privacy_transaction_is_not_expressible`.
- `WithdrawCollateral` does not support withdrawals to `PrivateUnauthorized` and `PrivateAuthorizedInit`; explicitly checks that the destination account is not default. Demonstrated with the test `stablecoin_withdraw_collateral_to_new_private_destination_is_not_expressible`.
- Vault is explicitly public PDA by formula requirement.

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
| InitializeAccount | `token_initialize_private_account_without_nsk_is_not_expressible` | EXIST | `InitializeAccount`'s target is `#[account(init, signer)]` — a third party cannot initialize a private holding via `PrivateUnauthorized` (no `nsk`); rejected by the SPEL macro ("must be a signer") before the program's own logic runs | ❌ (confirmed not-expressible by design) |
| InitializeAccount | `token_group_owned_holding_shared_control_initialize` | GROUP | A group member — not the party who created the group — self-initializes the shared holding directly via `PrivateAuthorizedInit` | ✅ |
| MintWithAuthority | `token_mint_with_authority_to_private_holding` | EXIST | External-authority mint (distinct signer from the definition) directly to a fresh private recipient | ✅ |
| NewFungibleDefinition | `token_new_fungible_definition_private_initial_holder` | REGULAR | Public token definition, private initial holder that self-initializes via `PrivateAuthorizedInit` (own `nsk` supplied) — same self-service shape as `InitializeAccount`'s target | ✅ |
| NewFungibleDefinition | `token_new_fungible_definition_private_holder_without_nsk_is_not_expressible` | EXIST | The initial holder cannot be created via `PrivateUnauthorized` — rejected by the SPEL macro before the program's own logic runs | ❌ (confirmed not-expressible by design) |


### Remarks
- `Initialization` is not possible for `PrivateUnauthorized` accounts due to `is_authorized = false`.
- New token definition is not permitted for `PrivateUnauthorized` as Token holding due to `is_authorized = false`.E.g., both Token Definition and Token Holding for a new Token must be from an authorized account.

# Conclusions

Privacy coverage for LEZ program tests is greatly improved from the added tests. Though, there are a few noticable gaps:
- `PrivateUnauthorized` accounts can be blocked by programs with a check `is_authorized = true`. However, this issue can be avoided by defining `is_authorized = true` for account initialization with `PrivateUnauthorized` (e.g., no knowledge of `npk`). Account initialization cannot be used to maliciously alter a pre-existing account, and thus `is_authorized = true` would not offer any malicious path forward for the third-party initializing the account.
- Privacy transactions have issues with chain calls in which multiple calls affect the same private account. This issue can be mitigated by adopting account diff paradigm instead of the current "account state replacement" that we currently use.
- AMM tests are blocked by issues with the clock account; bug in `spel-framework`.