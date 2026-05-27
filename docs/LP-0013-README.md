# LP-0013: Token Program Mint Authority

This document describes the mint authority model added to the LEZ Token program as part of LP-0013.

## Overview

The LEZ Token program now supports a mint authority model for fungible tokens:

- **Mint authority set at initialization** — create a token with a designated minter
- **Minting by the authority** — the authority can mint additional tokens at any time
- **Authority rotation** — transfer minting rights to a new key
- **Authority revocation** — permanently fix the supply by setting authority to `None`

The `lez-authority` crate provides a reusable, program-agnostic authority library (RFP-001).

## Architecture

### Authority Model

`mint_authority: Option<[u8; 32]>` is added to `TokenDefinition::Fungible`:
- `Some(key)` — the key holder can mint and rotate/revoke
- `None` — supply is permanently fixed, minting rejected

### New Instructions

| Instruction | Description |
|---|---|
| `NewFungibleDefinitionWithAuthority` | Create token with mint authority |
| `Mint` (updated) | Now authority-gated — rejects if authority is None |
| `SetAuthority` | Rotate or revoke mint authority |

### Atomicity

`SetAuthority` only mutates state after all checks pass. A failed authorization check returns an error before any write occurs, leaving the prior authority intact.

### Error Codes

| Condition | Message |
|---|---|
| Mint with revoked authority | Mint authority has been revoked; this token has a fixed supply |
| SetAuthority without authorization | Definition account authorization is missing |
| SetAuthority on already-revoked | Mint authority already revoked; supply is permanently fixed |

## Crate Structure

- `lez-authority/` — Agnostic AuthoritySlot library (RFP-001)
- `programs/token/core/` — TokenDefinition with mint_authority field
- `programs/token/src/mint.rs` — Authority-gated minting
- `programs/token/src/set_authority.rs` — Rotation and revocation handler
- `programs/token/src/new_definition.rs` — NewFungibleDefinitionWithAuthority handler
- `program_methods/guest/src/bin/token.rs` — Guest binary dispatch
- `wallet/src/program_facades/token.rs` — SDK facade methods

## Deployment Steps

### Prerequisites

```bash
git clone https://github.com/bristinWild/logos-execution-zone
cd logos-execution-zone
cargo install logos-scaffold
lgs new my-project && cd my-project
lgs setup
```

### Start local sequencer

```bash
lgs localnet start
lgs wallet topup
```

### Create accounts

```bash
lgs wallet -- account new public   # definition account
lgs wallet -- account new public   # supply account
```

### Create token

```bash
lgs wallet -- token new \
  --definition-account-id <definition_id> \
  --supply-account-id <supply_id> \
  --name "MyCoin" \
  --total-supply 1000000
```

### Mint additional tokens

```bash
lgs wallet -- token mint \
  --definition <definition_id> \
  --holder <holder_id> \
  --amount 500000
```

### Verify on-chain

```bash
lgs wallet -- account get --account-id <definition_id>
```

## Running Tests

```bash
# Unit tests
cargo test -p lez-authority --lib
cargo test -p token_program --lib

# All LP-0013 tests
RISC0_DEV_MODE=1 cargo test -p lez-authority -p token_program --lib
```

## Example Scripts

```bash
# Fixed supply token
bash scripts/examples/fixed_supply_token.sh

# Variable supply token with authority rotation
bash scripts/examples/variable_supply_token.sh
```

## End-to-End Demo

```bash
RISC0_DEV_MODE=0 bash scripts/demo-full-flow.sh
```

## Compute Unit Costs

| Operation | CU Cost |
|---|---|
| NewFungibleDefinitionWithAuthority | TBD |
| Mint (with authority check) | TBD |
| SetAuthority (rotate) | TBD |
| SetAuthority (revoke) | TBD |

## References

- [lez-authority crate](../lez-authority/src/lib.rs)
- [SetAuthority handler](../programs/token/src/set_authority.rs)
- [Mint handler](../programs/token/src/mint.rs)
- [Solana SPL Token - Set Authority](https://solana.com/docs/tokens/basics/set-authority)
