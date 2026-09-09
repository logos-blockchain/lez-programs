# Stablecoin core module

`stablecoin_module` is a headless Logos `core` module for the LEZ Stablecoin
Program. It exposes deployment discovery, protocol-state reads, and protocol
initialization through the same universal API used by `logoscore` and UI
modules.

The Qt-free C++ adapter handles live wallet reads and transaction submission.
`stablecoin_ffi` owns exact account decoding, PDA derivation, request
validation, and `stablecoin_core::Instruction` serialization.

## API

Every method returns a stable envelope. Success starts with:

```json
{ "status": "ok", "error": "" }
```

Failure returns:

```json
{ "status": "error", "error": "<stable_code>" }
```

### `programInfo()`

Returns the configured Stablecoin Program ID and the derived singleton account
IDs for protocol parameters, stability-fee accumulator, redemption-price
state, stablecoin definition, stablecoin master holding, and `CLOCK_01`. Each
ID is returned in base58 and lowercase hexadecimal form.

### `protocolParameters()`

Reads the singleton Protocol Parameters account through
`lez_core`, verifies its PDA and owner, and exactly decodes its
data. All `u128`, `i128`, and `u64` values are returned as decimal strings.

### `stabilityFeeAccumulator()`

Reads the singleton Stability Fee Accumulator account through `lez_core`,
verifies its PDA and owner, and exactly decodes its stored snapshot. The result
includes the account ID in base58 and lowercase hexadecimal form plus
`accumulatedRateAtLastAccrual` and `lastAccruedAt` as decimal strings. It does
not project the accumulator to the current time.

### `redemptionPriceState()`

Reads the singleton Redemption Price State account through `lez_core`, verifies
its PDA and owner, and exactly decodes its stored controller state. The result
includes the account ID in base58 and lowercase hexadecimal form plus
`redemptionPriceAtLastUpdate`, `redemptionRatePerMillisecond`,
`controllerIntegralTerm`, and `lastUpdatedAt` as decimal strings. It does not
project the current redemption price or simulate a controller update.

### `currentGlobalState()`

Reads Protocol Parameters, Stability Fee Accumulator, Redemption Price State,
and the canonical `CLOCK_01` account through `lez_core`. It verifies each
stablecoin singleton's PDA, owner, and data before projecting both current
values at the clock account's Unix-millisecond timestamp.

The result contains `accumulatedRateAtLastAccrual`, `lastAccruedAt`,
`redemptionPriceAtLastUpdate`, `lastUpdatedAt`, `currentAccumulatedRate`,
`currentRedemptionPrice`, and `projectedAt`. Every value is an exact decimal
string. Projection uses saturating timestamp subtraction and the on-chain
seven-day compounding-window clamp. The method accepts no caller-provided time.

### `redemptionRateUpdateQuote()`

Reads Protocol Parameters, Redemption Price State, the configured market-price
oracle, and canonical `CLOCK_01` account, then quotes the next controller tick
without submitting a transaction. It projects the current redemption price
with the stored rate before calling the same pure controller used on-chain.

Ready quotes return `canSubmit: true`, `code: "ready"`, the current redemption
and market prices, elapsed milliseconds, next redemption rate, next controller
integral term, and integral/rate clamp bounds. All numeric values are exact
decimal strings.

A stale oracle, zero oracle price, or not-yet-due update returns a successful
read-only quote with `canSubmit: false`, `code: "blocked"`, machine-readable
`errors`, and explicit `null` next-controller values. Multiple blockers are
reported in on-chain gate order. The frozen flag does not block this operation.

### Permissionless maintenance transactions

`accrueStabilityFee(callerId)`, `updateRedemptionRate(callerId)`, and
`refreshGlobals(callerId)` submit permissionless protocol-maintenance
transactions. `callerId` accepts base58 or 64-character hexadecimal form, must
be a public account controlled by the connected wallet, and is the sole signer.
Success adds `transactionId` to the standard response envelope.

The module reads live protocol state, derives every singleton account and the
canonical `CLOCK_01` account internally, then submits these exact account
orders:

| Method | Accounts |
| --- | --- |
| `accrueStabilityFee` | caller, Protocol Parameters, Stability Fee Accumulator, `CLOCK_01` |
| `updateRedemptionRate` | caller, Protocol Parameters, Redemption Price State, configured market-price oracle, `CLOCK_01` |
| `refreshGlobals` | caller, Protocol Parameters, Stability Fee Accumulator, Redemption Price State, configured market-price oracle, `CLOCK_01` |

`updateRedemptionRate` runs the live quote preflight and does not submit when
the first on-chain gate is `oracle_stale`, `oracle_price_zero`, or
`rate_update_too_soon`. `refreshGlobals` intentionally submits under those soft
gates: its fee-accrual half still runs while the on-chain instruction may skip
the controller update. A frozen protocol does not block any of the three
maintenance methods.

### `initializeProgram(request)`

Required request fields:

| Field | Type |
| --- | --- |
| `adminId` | base58 or 64-character hexadecimal account ID |
| `freezeAuthorityId` | base58 or 64-character hexadecimal account ID |
| `collateralDefinitionId` | base58 or 64-character hexadecimal account ID |
| `marketPriceOracleId` | base58 or 64-character hexadecimal account ID |
| `initialStabilityFeePerMillisecond` | exact `u128` decimal |
| `initialControllerProportionalGain` | exact `i128` decimal |
| `initialControllerIntegralGain` | exact `i128` decimal |
| `initialMinimumCollateralizationRatio` | exact `u128` decimal |
| `minimumMillisecondsBetweenRateUpdates` | exact `u64` decimal |
| `maximumOraclePriceAgeMilliseconds` | exact `u64` decimal |
| `initialRedemptionPrice` | exact `u128` decimal |
| `stablecoinName` | string accepted by the Stablecoin Program |

The module verifies all five derived target PDAs are uninitialized, validates
the collateral definition, oracle asset pair, and clock accounts, then submits
the exact nine-account instruction. Only `adminId` signs. Success adds
`transactionId` to the response envelope.

Pass numeric values as decimal strings. JSON integers are accepted when their
exact value survives parsing. JSON floating-point values are always rejected.

## Runtime configuration

Set either environment variable on the process hosting the module:

```bash
STABLECOIN_PROGRAM_ID=<base58-or-hex-program-id>
STABLECOIN_PROGRAM_BIN=/absolute/path/to/stablecoin.bin
```

When both are set, they must identify the same program. The binary must be the
exact deployable RISC Zero `.bin`; rebuilding it can change the program ID.

Set `STABLECOIN_DEBUG=1` to emit adapter diagnostics to module stderr.

## Build and test

Run from repository root:

```bash
RISC0_DEV_MODE=1 cargo +1.94.0 test -p stablecoin_ffi
RISC0_SKIP_BUILD=1 cargo +1.94.0 clippy -p stablecoin_ffi --all-targets -- -D warnings
nix build path:.#stablecoin_ffi -L
nix build path:.#stablecoin-module -L
nix build path:.#stablecoin-module-tests -L
```

Use `path:.` while files are untracked. Once tracked, `.#stablecoin-module` is
equivalent.
