# AMM client JSON wire API

The C ABI accepts one tagged JSON object and returns one envelope:

```json
{"schema":"amm-client.v1","ok":true,"value":{"schema":"amm-client.v1"}}
```

```json
{"schema":"amm-client.v1","ok":false,"error":{"code":"invalid_request","message":"..."}}
```

Requests may include `"schema":"amm-client.v1"`. Schema-less requests remain accepted for
compatibility. Every successful wire value and every C envelope identifies the response schema.

All `u128` amounts, reserves, supplies, fees, nonces, and balances are unsigned decimal strings.
All `u64` windows and deadlines are also decimal strings. Program IDs are JSON arrays containing
exactly eight `u32` words in their canonical order. Hexadecimal and byte layouts are host-adapter
concerns and are neither accepted nor emitted by this shared wire API. Signed ticks are decimal
strings. Account IDs use canonical base58.
Account `data` is an even-length hexadecimal string.

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

Discovery and task-transaction operations use the complete caller-ordered pair read set:

```json
{
  "snapshots": {
    "pool": { "...": "account snapshot" },
    "firstTokenDefinition": { "...": "account snapshot" },
    "secondTokenDefinition": { "...": "account snapshot" },
    "firstTokenVault": { "...": "account snapshot" },
    "secondTokenVault": { "...": "account snapshot" },
    "liquidityDefinition": { "...": "account snapshot" },
    "lpLockHolding": { "...": "account snapshot" },
    "currentTick": { "...": "account snapshot" },
    "clock": { "...": "account snapshot" }
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
| `prepare_create_pool_transaction` | same task request documented under Task transactions |
| `prepare_add_liquidity_transaction` | same task request documented under Task transactions |
| `prepare_remove_liquidity_transaction` | same task request documented under Task transactions |
| `prepare_swap_exact_input_transaction` | same task request documented under Task transactions |
| `prepare_swap_exact_output_transaction` | same task request documented under Task transactions |

A successful plan value contains the following fields (`instructionWords` is abbreviated here):

```json
{
  "instruction": "add_liquidity",
  "instructionArgs": {
    "minAmountLiquidity": "99",
    "maxAmountToAddTokenA": "400",
    "maxAmountToAddTokenB": "100",
    "deadline": "1900000000000"
  },
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
  "affectedAccountIds": ["base58-account-id"],
  "instructionWords": [5]
}
```

The real `instructionWords` array contains the complete encoding produced directly from the
canonical `amm_core::Instruction` with RISC Zero Serde. `instructionArgs` is exhaustively derived
from that same typed instruction, so C++/QML consumers do not decode RISC Zero Serde. Its `u128`
and `u64` fields are decimal strings, optional fields are JSON `null`, and account IDs are base58
strings. Account rows follow guest/IDL order.

## Quote operations

Send requests to `amm_client_quote` or `wire::quote_json`. Pool economic operations use the
existing-pool quote state described above. Discovery and opening-intent operations use the fields
shown in this table and the sections below.

| `operation` | Additional fields |
|---|---|
| `protocol_constants` | none; returns decimal-string `minimumLiquidity`, `feeBpsDenominator`, `slippageBpsDenominator`, and `supportedFeeTiers` |
| `human_price_ratio_to_q64_64` | caller-ordered token IDs, `firstAmount`, `secondAmount`, and decimal-string `firstTokenDecimals`/`secondTokenDecimals` |
| `derive_config_id` | `ammProgramId` |
| `inspect_config` | `ammProgramId`, raw `config` snapshot |
| `canonical_pair` | `firstTokenDefinitionId`, `secondTokenDefinitionId` |
| `derive_pair_read_manifest` | `ammProgramId`, raw `config`, `firstTokenDefinitionId`, `secondTokenDefinitionId` |
| `inspect_pair` | fields from `derive_pair_read_manifest` plus complete `snapshots` |
| `prepare_minimum_opening_pair` | `desiredPriceQ64_64`, `feeBps` |
| `prepare_opening_from_token_a` | `tokenAAmount`, `desiredPriceQ64_64`, `feeBps` |
| `prepare_opening_from_token_b` | `tokenBAmount`, `desiredPriceQ64_64`, `feeBps` |
| `validate_explicit_opening_pair` | `tokenAAmount`, `tokenBAmount`, `desiredPriceQ64_64`, `feeBps` |
| `prepare_caller_opening_pair` | caller token IDs, desired price, fee, and tagged `intent` described below |
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
- swaps: `direction`, `amountIn`, `effectiveAmountIn`, `feeAmount`, `amountOut`, `pool`, and
  decimal-string `poolSpotChangeBps`;
- reserve sync: `donatedAmountA`, `donatedAmountB`, `pool`;
- oracle price: `baseAsset`, `quoteAsset`, `initialPriceQ64_64`, `windowDuration`; and
- pair order: `order` (`stored` or `reversed`).

A `pool` result contains decimal-string `liquidityPoolSupply`, `reserveA`, `reserveB`, and
`spotPriceQ64_64` fields.

`poolSpotChangeBps` is the exact directional movement of the pool's spot price from the pre-swap
snapshot to the returned quote. It is not execution-price impact.

## Host adapters

Hosts normalize sequencer or wallet responses into the canonical snapshot fields before calling the
client: base58 `id`, eight-word `programOwner`, decimal-string `balance` and `nonce`, and
hexadecimal `data`. Keep raw RPC parsing and wallet-specific representations outside AMM.

`human_price_ratio_to_q64_64` declares that `firstAmount` human units of the first token equal
`secondAmount` human units of the second token. Amounts are unsigned decimal text and may contain
up to 38 fractional digits. Token decimals are accepted from `0` through `38`. The adapter derives
stored token A/B order from the token IDs, applies unequal token decimals, floors once, and returns
decimal-string `priceQ64_64`. Callers keep display order; reversed pairs must not invert locally.

## Discovery, inspection, and opening intents

Discovery functions derive IDs only; adapters fetch the returned accounts and submit raw
snapshots for inspection. `inspect_pair` returns `status` as `missing` or `active`. Missing output
contains the read manifest, caller-ordered definitions, vault lifecycle states, and clock. Active
output contains the manifest, `callerOrder`, stored token/vault/LP IDs, reserves, vault balances,
LP supply, fee, stored Q64.64 spot price, current tick, and clock. Numeric protocol fields remain
strings.

`prepare_caller_opening_pair` accepts caller token order without reproducing canonical ordering:

```json
{
  "operation": "prepare_caller_opening_pair",
  "firstTokenDefinitionId": "base58-account-id",
  "secondTokenDefinitionId": "base58-account-id",
  "desiredPriceQ64_64": "18446744073709551616",
  "feeBps": "30",
  "intent": { "kind": "first_amount", "amount": "2000" }
}
```

Other intent shapes are `{ "kind":"minimum" }`,
`{ "kind":"second_amount", "amount":"..." }`, and
`{ "kind":"explicit", "firstAmount":"...", "secondAmount":"..." }`. The result includes
`callerOrder`, caller `firstAmount`/`secondAmount`, and the canonical stored opening quote and
amounts.

## Task transactions

The five snapshot-bound task operations are accepted only by
`amm_client_plan`/`wire::plan_json`. Every request includes `ammProgramId`, raw `config`, the
complete caller-ordered `snapshots`, and decimal-string `deadline`. The quote endpoint rejects
these operation tags with `invalid_request`.

| `operation` | Additional fields |
|---|---|
| `prepare_create_pool_transaction` | caller token IDs, `firstTokenHolding`, `secondTokenHolding`, `liquidityHolding`, `firstAmount`, `secondAmount`, `feeBps` |
| `prepare_add_liquidity_transaction` | caller token IDs and holdings, `maxFirstAmount`, `maxSecondAmount`, `slippageBps`, optional `expectedFeeBps` |
| `prepare_remove_liquidity_transaction` | caller token IDs and holdings, `removeLiquidityAmount`, `slippageBps`, optional `expectedFeeBps` |
| `prepare_swap_exact_input_transaction` | input/output token IDs and holdings, `amountIn`, `slippageBps`, optional `expectedFeeBps` |
| `prepare_swap_exact_output_transaction` | input/output token IDs and holdings, `exactAmountOut`, `slippageBps`, optional `expectedFeeBps` |

Successful task output contains:

```json
{
  "operation": "swap_exact_output",
  "quote": {},
  "callerAmounts": { "first": "101", "second": "100" },
  "plan": {
    "instruction": "swap_exact_output",
    "instructionArgs": {
      "exactAmountOut": "100",
      "maxAmountIn": "102",
      "deadline": "1900000000000"
    },
    "instructionWords": []
  },
  "quoteCommitment": "64-lowercase-hex-characters",
  "affectedAccountIds": ["base58-account-id"],
  "walletPrerequisites": {
    "signerAccountIds": ["base58-account-id"],
    "freshAccountIds": [],
    "funding": [{
      "holdingAccountId": "base58-account-id",
      "tokenDefinitionId": "base58-account-id",
      "available": "1000",
      "required": "102"
    }]
  },
  "deadline": "1900000000000",
  "poolSpotChangeBps": "42"
}
```

`poolSpotChangeBps` is `null` for non-swap tasks. Add-liquidity funding requirements use the
caller caps. Exact-output swap funding uses the plan's slippage-adjusted `maxAmountIn`. Hosts
should refresh snapshots, prepare again, compare `quoteCommitment`, and submit only the refreshed
plan. A deadline-only refresh keeps the same `quoteCommitment`; the refreshed plan still carries
the deadline to submit.

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

Prepared add-liquidity maximums preserve the original caller caps. Replacing them with rounded
`actualAmountA` and `actualAmountB` can change the program quote when reserve ratios are not
divisible, because execution performs proportional integer rounding again. Funding prerequisites
therefore cover the caller caps while display amounts remain the canonical quote's actual deposit.

## Ownership and failures

The client validates account decoding, configured owners, canonical PDAs, pool/vault/token/LP
relationships, swap input/output pairing, and required input balances. Quote arithmetic failures
retain the stable `amm_program::quote::QuoteError` code.

Every failure uses `{ "code": "...", "message": "..." }`. `code` is the stable
machine-readable contract; `message` is diagnostic text. JSON adapter failures return
`invalid_request` or `unsupported_schema`. The C envelope additionally returns `null_request`,
`invalid_utf8`, `invalid_json`, `response_serialization_failed`, or `response_contains_nul` for
boundary failures. Human-price conversion uses the stable `IntentError` codes documented by the
Rust API.

No request performs network I/O or checks an ImageID, release version, compatibility manifest, or
program allowlist. Deployment configuration is expected to select the corresponding AMM build.
