# AMM client

`amm_client` is the stateless host boundary for the AMM program. It reuses
`amm_program::quote` for economic calculations, builds the actual
`amm_core::Instruction` variants, derives protocol accounts through core PDA helpers, and encodes
instructions with the RISC Zero Serde codec consumed by the guest.

The crate does not fetch accounts, manage keys, sign, or submit transactions. Those remain host
adapter responsibilities.

## Rust API

- `quote` validates fetched config, pool, vault, token-definition, LP-definition, and user-holding
  snapshots before delegating calculations to `amm_program::quote`.
- `slippage` converts validated quotes into integer-only instruction guards. Minimum guards round
  down, maximum guards round up, and checked overflow returns a typed error.
- `plan` covers all ten guest instructions and returns the canonical instruction plus ordered
  account roles and writable, signer, and init flags.
- `TransactionPlan::instruction_data` serializes its `amm_core::Instruction` with
  `risc0_zkvm::serde::to_vec`.
- `wire` exposes lossless JSON adapters for non-Rust hosts.

Planner coverage:

| Guest instruction | Planner |
|---|---|
| `Initialize` | `plan_initialize` |
| `UpdateConfig` | `plan_update_config` |
| `CreatePriceObservations` | `plan_create_price_observations` |
| `CreateOraclePriceAccount` | `plan_create_oracle_price_account` |
| `NewDefinition` | `plan_create_pool` |
| `AddLiquidity` | `plan_add_liquidity` |
| `RemoveLiquidity` | `plan_remove_liquidity` |
| `SwapExactInput` | `plan_swap_exact_input` |
| `SwapExactOutput` | `plan_swap_exact_output` |
| `SyncReserves` | `plan_sync_reserves` |

Quote coverage includes protocol constants, pair ordering, pool creation, preview and exact
add/remove liquidity, preview and exact-input/output swaps, reserve synchronization, and
oracle-price initialization. `prepare_create_pool`, `prepare_add_liquidity`,
`prepare_remove_liquidity`, `prepare_swap_exact_input`, and `prepare_swap_exact_output` return a
quote plus the exact amount fields to pass to the corresponding planner. Consumers choose a
slippage tolerance in basis points but do not calculate chain guards. Prepared add-liquidity maxima
use the quote's actual deposits, so execution cannot spend above the displayed/current quote even
when the caller supplied a lopsided pair of caps.

## Compatibility assumption

The client and deployed AMM are expected to be built from the corresponding source version. The
client performs no runtime ImageID, release-version, or program allowlist check. The supplied AMM
program ID is used for transaction targeting and canonical PDA derivation. Snapshot owner, account
relationship, and PDA checks remain normal protocol validation.

## C and JSON boundary

The built library exports:

```c
char *amm_client_plan(const char *request_json);
char *amm_client_quote(const char *request_json);
void amm_client_free(char *value);
```

Every call returns an owned JSON envelope. Release it exactly once with `amm_client_free`; passing
`NULL` to the free function is allowed. See [`include/amm_client.h`](include/amm_client.h) and
[`docs/wire-api.md`](docs/wire-api.md) for the complete transport contract.

Raw `u128` and `u64` values cross JSON as decimal strings. Account IDs use their canonical base58
display form, program IDs use eight JSON `u32` words, account data uses hexadecimal, and encoded
instruction words remain JSON `u32` numbers. No JavaScript `Number` conversion is required for
chain amounts or deadlines.
