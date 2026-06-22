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
| `Mint` (updated) | Now authority-gated — Now authority-gated |
| `SetAuthority` | Rotate or revoke mint authority |

### Atomicity

`SetAuthority` only mutates state after all checks pass. A failed authorization check returns an error before any write occurs, leaving the prior authority intact.

### Error Codes

| Condition | Message |
|---|---|
| Mint when authority revoked | Mint authority check failed: Revoked |
| Mint by non-authority signer | Mint authority check failed: Unauthorized |
| Mint/SetAuthority without signed authority | Mint authority must authorize the transaction |
| SetAuthority on already-revoked | SetAuthority failed: AlreadyRevoked |
| SetAuthority by wrong signer | SetAuthority failed: Unauthorized |
| Create/rotate with all-zero authority | Mint authority must be a valid non-zero account ID |

## Crate Structure

- `lez-authority/` — Agnostic AuthoritySlot library (RFP-001)
- `programs/token/core/` — TokenDefinition with mint_authority field
- `programs/token/src/mint.rs` — Authority-gated minting
- `programs/token/src/set_authority.rs` — Rotation and revocation handler
- `programs/token/src/new_definition.rs` — NewFungibleDefinitionWithAuthority handler
- `programs/token/methods/guest/src/bin/token.rs` — Guest binary dispatch

## Module/SDK

`token_core` provides the reusable types and instructions for building Logos modules. It is already consumed by `amm`, `ata`, `stablecoin`, and `integration_tests` in this workspace:

```toml
[dependencies]
token_core = { path = "programs/token/core" }
```

Key types:
- `TokenDefinition::Fungible { mint_authority, .. }` — token definition with authority
- `Instruction::NewFungibleDefinitionWithAuthority` — create with authority
- `Instruction::SetAuthority` — rotate or revoke

## RFP-001 Compliance

LP-0013 has a formal dependency on [RFP-001](https://github.com/logos-co/rfp/blob/master/RFPs/RFP-001-admin-authority-lib.md) — the standardised admin authority library. The `lez-authority` crate in this submission directly implements the approval pattern defined in RFP-001:

| RFP-001 Requirement | How `lez-authority` satisfies it |
|---|---|
| Self-sufficient, agnostic authority library | `lez-authority` has zero program-specific dependencies — it only uses `borsh` for serialisation |
| Authority slot abstraction | `AuthoritySlot` struct wraps `Option<[u8; 32]>` with `check`, `set`, and revocation semantics |
| Approval check | `AuthoritySlot::check(signer)` returns an error if the signer does not match or authority is revoked |
| Rotation | `AuthoritySlot::set(Some(new_key))` atomically rotates to a new authority |
| Permanent revocation | `AuthoritySlot::set(None)` permanently fixes the supply — subsequent `set` calls are rejected |
| Reusable by other programs | Any LEZ program can add `lez-authority` as a workspace dependency and use `AuthoritySlot` directly |

The `lez-authority` crate was also submitted as part of [RFP-001 PR #212](https://github.com/logos-co/spel/pull/212) (the `spel-admin-authority` library with the `#[require_admin]` macro). The two are complementary: `lez-authority` is the lightweight on-chain primitive; `spel-admin-authority` is the SPEL framework macro layer built on top of the same pattern.

## Deployment

The program ID is the hash of the compiled guest ELF and will change whenever
the guest is rebuilt. Obtain the current ID after building:

```bash
lgs deploy --program-path target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin
```

### Build the guest binary

```bash
cargo risczero build --manifest-path programs/token/methods/guest/Cargo.toml
```

### Deploy to the sequencer

```bash
wallet deploy-program target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin
```

## Running Tests

```bash
# Authority unit tests
cargo test -p lez-authority --lib
cargo test -p token_program --lib

# Authority integration tests (zkVM, dev mode)
RISC0_DEV_MODE=1 cargo test -p integration_tests --test token -- token_new_fungible_definition_with_authority token_set_authority_revoke
```

## CLI Usage (via `spel`)

### Create token with mint authority

```bash
spel --idl artifacts/token-idl.json --program <token-binary> \
  -- new-fungible-definition-with-authority \
  --definition-target-account <DEF_ID> \
  --holding-target-account <SUPPLY_ID> \
  --name "MyToken" \
  --initial-supply 1000000 \
  --mint-authority <AUTHORITY_KEY_HEX>
```

### Mint tokens

```bash
spel --idl artifacts/token-idl.json --program <token-binary> \
  -- mint \
  --definition-account <DEF_ID> \
  --authority-account <AUTHORITY_ID> \
  --user-holding-account <HOLDER_ID> \
  --amount-to-mint 500000
```

### Rotate authority

```bash
spel --idl artifacts/token-idl.json --program <token-binary> \
  -- set-authority \
  --definition-account <DEF_ID> \
  --authority-account <AUTHORITY_ID> \
  --new-authority <NEW_KEY_HEX>
```

### Revoke authority (fix supply permanently)

```bash
spel --idl artifacts/token-idl.json --program <token-binary> \
  -- set-authority \
  --definition-account <DEF_ID> \
  --authority-account <AUTHORITY_ID> \
  --new-authority none
```

## Example Scripts

```bash
# Fixed supply token (creates with authority, then revokes)
bash scripts/examples/fixed_supply_token.sh

# Variable supply token (creates with authority, mints more, optionally rotates)
bash scripts/examples/variable_supply_token.sh
```

## End-to-End Demo

The demo script must be run from inside an `lgs` scaffold project directory (where the localnet and wallet live):

```bash
# 1. Set up an lgs scaffold (if you don't have one):
cargo install logos-scaffold
lgs new my-scaffold && cd my-scaffold
lgs setup
lgs localnet start
lgs wallet topup

# 2. Deploy the token program:
lgs deploy --program-path /path/to/lez-programs/target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin

# 3. Run the demo:
RISC0_DEV_MODE=0 bash /path/to/lez-programs/scripts/demo-full-flow.sh
```

The script will:
1. Verify the localnet is running
2. Fund the wallet
3. Create 3 token accounts (definition, supply holder, recipient)
4. Submit `NewFungibleDefinitionWithAuthority` (creates "DemoCoin" with 1M supply)
5. Submit `Mint` (mints 500K to recipient → total supply 1.5M)
6. Submit `SetAuthority` with `None` (permanently revokes minting)
7. Run unit tests to verify authority logic (64 tests)

## Compute Unit (CU) Costs

Measured on LEZ localnet with `RISC0_DEV_MODE=1` (execution only, no proof):

| Operation | Execution Time | Notes |
|---|---|---|
| `NewFungibleDefinitionWithAuthority` | ~11ms | Creates token with mint authority |
| `Mint` (with authority) | ~10ms | Authority-gated mint |
| `SetAuthority` (rotate) | ~8ms | Rotates to new key |
| `SetAuthority` (revoke) | ~8ms | Permanently revokes, sets None |

Note: With `RISC0_DEV_MODE=0`, full ZK proof generation takes 3–10 minutes per transaction on Apple M-series hardware. LEZ's per-transaction compute budget may change during testnet.

## References

- [lez-authority crate](../lez-authority/src/lib.rs)
- [SetAuthority handler](../programs/token/src/set_authority.rs)
- [Mint handler](../programs/token/src/mint.rs)
- [Solana SPL Token - Set Authority](https://solana.com/docs/tokens/basics/set-authority)
