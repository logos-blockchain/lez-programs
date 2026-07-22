# AMM client JSON wire API

The C ABI accepts one tagged JSON object and returns one envelope:

```json
{"ok":true,"value":{}}
```

```json
{"ok":false,"error":{"code":"invalid_request","message":"..."}}
```

All `u128` amounts, reserves, supplies, fees, nonces, and balances are unsigned decimal strings.
All `u64` windows and deadlines are also decimal strings. Program IDs are arrays of eight `u32`
words. Account IDs are base58 strings. Account `data` is an even-length hexadecimal string.

## Shared inputs

Plan context:

```json
{
  "ammProgramId": [0, 0, 0, 0, 0, 0, 0, 0],
  "tokenProgramId": [0, 0, 0, 0, 0, 0, 0, 0],
  "twapOracleProgramId": [0, 0, 0, 0, 0, 0, 0, 0],
  "authority": "base58-account-id"
}
```

Decoded pool input used by existing-pool planners:

```json
{
  "poolId": "base58-account-id",
  "definitionTokenAId": "base58-account-id",
  "definitionTokenBId": "base58-account-id",
  "vaultAId": "base58-account-id",
  "vaultBId": "base58-account-id",
  "liquidityPoolId": "base58-account-id",
  "liquidityPoolSupply": "2000",
  "reserveA": "1000",
  "reserveB": "500",
  "fees": "30"
}
```

Fetched account snapshot used by quotes:

```json
{
  "id": "base58-account-id",
  "programOwner": [0, 0, 0, 0, 0, 0, 0, 0],
  "balance": "0",
  "nonce": "0",
  "data": "00ff"
}
```

Existing-pool quote operations include these top-level state fields:

```json
{
  "ammProgramId": [0, 0, 0, 0, 0, 0, 0, 0],
  "config": { "...": "account snapshot" },
  "snapshot": {
    "pool": { "...": "account snapshot" },
    "tokenADefinition": { "...": "account snapshot" },
    "tokenBDefinition": { "...": "account snapshot" },
    "vaultA": { "...": "account snapshot" },
    "vaultB": { "...": "account snapshot" },
    "liquidityDefinition": { "...": "account snapshot" }
  }
}
```

## Plan operations

Send requests to `amm_client_plan` or `wire::plan_json`.

| `operation` | Additional fields |
|---|---|
| `initialize` | `ammProgramId`, `tokenProgramId`, `twapOracleProgramId`, `authority` |
| `update_config` | `context`, optional `tokenProgramId`, optional `twapOracleProgramId`, optional `newAuthority` |
| `create_price_observations` | `context`, `poolId`, `windowDuration` |
| `create_oracle_price_account` | `context`, `poolId`, `windowDuration` |
| `create_pool` | `context`, `tokenADefinitionId`, `tokenBDefinitionId`, `userHoldingA`, `userHoldingB`, `userHoldingLp`, `tokenAAmount`, `tokenBAmount`, `fees`, `deadline` |
| `add_liquidity` | `context`, `pool`, `userHoldingA`, `userHoldingB`, `userHoldingLp`, `minAmountLiquidity`, `maxAmountToAddTokenA`, `maxAmountToAddTokenB`, `deadline` |
| `remove_liquidity` | `context`, `pool`, `userHoldingA`, `userHoldingB`, `userHoldingLp`, `removeLiquidityAmount`, `minAmountToRemoveTokenA`, `minAmountToRemoveTokenB`, `deadline` |
| `swap_exact_input` | `context`, `pool`, `userInputHolding`, `userOutputHolding`, `swapAmountIn`, `minAmountOut`, `deadline` |
| `swap_exact_output` | `context`, `pool`, `userInputHolding`, `userOutputHolding`, `exactAmountOut`, `maxAmountIn`, `deadline` |
| `sync_reserves` | `context`, `pool` |

A successful plan value contains the following fields (`instructionWords` is abbreviated here):

```json
{
  "instruction": "add_liquidity",
  "programId": [0, 0, 0, 0, 0, 0, 0, 0],
  "accounts": [
    {
      "id": "base58-account-id",
      "role": "config",
      "writable": false,
      "signer": false,
      "init": false
    }
  ],
  "instructionWords": [5]
}
```

The real `instructionWords` array contains the complete encoding produced directly from the
canonical `amm_core::Instruction` with RISC Zero Serde. Account rows follow guest/IDL order.

## Quote operations

Send requests to `amm_client_quote` or `wire::quote_json`. Except `protocol_constants`,
`create_pool`, and `prepare_create_pool`, every operation below also includes the existing-pool
quote state described above.

| `operation` | Additional fields |
|---|---|
| `protocol_constants` | none; returns decimal-string `minimumLiquidity`, `feeBpsDenominator`, `slippageBpsDenominator`, and `supportedFeeTiers` |
| `pair_order` | `firstTokenDefinitionId`, `secondTokenDefinitionId` |
| `create_pool` | `ammProgramId`, `config`, `tokenADefinition`, `tokenBDefinition`, `tokenAAmount`, `tokenBAmount`, `feeBps` |
| `prepare_create_pool` | same fields as `create_pool`; returns quote plus `NewDefinition` instruction arguments |
| `preview_add_liquidity` | `maxAmountA`, `maxAmountB` |
| `prepare_add_liquidity` | `maxAmountA`, `maxAmountB`, `slippageBps` |
| `add_liquidity` | `maxAmountA`, `maxAmountB`, `minimumLiquidity` |
| `preview_remove_liquidity` | `userLiquidityHolding`, `removeLiquidityAmount` |
| `prepare_remove_liquidity` | `userLiquidityHolding`, `removeLiquidityAmount`, `slippageBps` |
| `remove_liquidity` | `userLiquidityHolding`, `removeLiquidityAmount`, `minimumAmountA`, `minimumAmountB` |
| `preview_swap_exact_input` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `amountIn` |
| `prepare_swap_exact_input` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `amountIn`, `slippageBps` |
| `swap_exact_input` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `amountIn`, `minimumAmountOut` |
| `preview_swap_exact_output` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `exactAmountOut` |
| `prepare_swap_exact_output` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `exactAmountOut`, `slippageBps` |
| `swap_exact_output` | `userInputHolding`, `userOutputHolding`, `inputTokenDefinitionId`, `exactAmountOut`, `maximumAmountIn` |
| `sync_reserves` | no additional fields |
| `create_oracle_price_account` | `windowDuration` |

Quote values use these result shapes:

- pool creation: `pool`, `lockedLiquidity`, `userLiquidity`;
- add liquidity: `actualAmountA`, `actualAmountB`, `liquidityToMint`, `pool`;
- remove liquidity: `withdrawAmountA`, `withdrawAmountB`, `liquidityToBurn`, `pool`;
- swaps: `direction`, `amountIn`, `effectiveAmountIn`, `feeAmount`, `amountOut`, `pool`;
- reserve sync: `donatedAmountA`, `donatedAmountB`, `pool`;
- oracle price: `baseAsset`, `quoteAsset`, `initialPriceQ64_64`, `windowDuration`; and
- pair order: `order` (`stored` or `reversed`).

A `pool` result contains decimal-string `liquidityPoolSupply`, `reserveA`, `reserveB`, and
`spotPriceQ64_64` fields.

## Prepared instruction arguments

The five `prepare_*` operations return the economic result under `quote` and decimal-string chain
arguments under `instructionArgs`. Those fields map directly to the matching plan operation:

- `prepare_create_pool`: `tokenAAmount`, `tokenBAmount`, `fees`;
- `prepare_add_liquidity`: `minAmountLiquidity`, `maxAmountToAddTokenA`,
  `maxAmountToAddTokenB`;
- `prepare_remove_liquidity`: `removeLiquidityAmount`, `minAmountToRemoveTokenA`,
  `minAmountToRemoveTokenB`;
- `prepare_swap_exact_input`: `swapAmountIn`, `minAmountOut`; and
- `prepare_swap_exact_output`: `exactAmountOut`, `maxAmountIn`.

`slippageBps` accepts `0` through `slippageBpsDenominator` (`10,000`) as an unsigned decimal
string. Minimum guards use integer floor rounding and stay at least one raw unit for positive
quotes. Maximum guards use integer ceil rounding. A maximum above `u128` returns
`slippage_bound_overflow`; an out-of-range tolerance returns `slippage_tolerance_out_of_range`.
This calculation runs only in the Rust client, never in JavaScript or QML.

Prepared add-liquidity maximums are the quote's `actualAmountA` and `actualAmountB`, not the original
possibly lopsided caps. The exact quote is rerun with those fields before they are returned. This
keeps the eventual plan from spending above the displayed/current quoted deposits.

## Ownership and failures

The client validates account decoding, configured owners, canonical PDAs, pool/vault/token/LP
relationships, swap input/output pairing, and required input balances. Quote arithmetic failures
retain the stable `amm_program::quote::QuoteError` code.

No request performs network I/O or checks an ImageID, release version, compatibility manifest, or
program allowlist. Deployment configuration is expected to select the corresponding AMM build.
