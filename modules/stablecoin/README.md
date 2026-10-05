# Stablecoin core module

`stablecoin_module` is a headless Logos `core` module for the LEZ Stablecoin
Program. It exposes deployment discovery, protocol-state reads, protocol
initialization, position health, position opening, collateral deposits, and debt repayment through the same
universal API used by `logoscore` and UI modules.

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

### `positionHealth(request)`

Accepts `ownerId` (base58 or hexadecimal account ID) and `positionNonce` (an
exact `u64` decimal string). It derives Position/Vault IDs and reads the Position,
Protocol Parameters, Stability Fee Accumulator, Redemption Price State, and
canonical `CLOCK_01`. It validates canonical identities, stablecoin ownership,
exact decoding, and stored Position owner/nonce/vault fields. Quotes work while
frozen, without wallet ownership or signatures, oracle/controller gates, vault
balance reads, or transaction submission. Stored Position collateral is used;
direct vault donations are reconciled through `depositCollateral`.

Success adds these fields to `{status: "ok", error: ""}`:

- `ownerId`, `ownerIdHex`, `positionId`, `positionIdHex`, `vaultId`, `vaultIdHex`.
- Exact decimal strings: `positionNonce`, `collateralAmount`,
  `normalizedDebtAmount`, `openedAt`, `currentAccumulatedRate`,
  `currentRedemptionPrice`, `minimumCollateralizationRatio`, `projectedAt`,
  `nominalDebt`, `collateralValue`, and `requiredCollateralValue`.
- Booleans: `isCollateralized` and `requirementSaturated`.

The program's exact fractional-debt comparison is shared with the quote:

```text
collateralValue = collateralAmount * FIXED_POINT_ONE^3
requiredCollateralValue = normalizedDebtAmount * currentAccumulatedRate
    * currentRedemptionPrice * minimumCollateralizationRatio
isCollateralized = collateralValue >= requiredCollateralValue
```

`nominalDebt` floors `normalizedDebtAmount * currentAccumulatedRate /
FIXED_POINT_ONE` for display only; it is never used in the comparison. Zero debt
has a zero requirement. Wide projections preserve saturating elapsed-time
subtraction and the seven-day clamp, and can exceed `u128`. A saturated
requirement is returned as the decimal `U512::MAX` with
`requirementSaturated: true`; it is conservatively undercollateralized because
all possible collateral values are below that cap.

Unrepresentable projection intermediates return `health_projection_overflow`;
an unrepresentable display calculation returns `health_arithmetic_overflow`.
These errors prevent a bounded projection from being reported as exact.
Absent Positions return `position_not_found` with the derived Position/Vault
IDs. Missing globals return `not_initialized`; failed reads return
`account_read_failed`. Numeric, ownership, PDA, stored-identity, and exact-data
validation reuse the existing stable error codes. Every call reads current
state again, including after an earlier successful quote.

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

### `openPosition(request)`

Opens a new collateral-only position with no debt. Required request fields:

| Field | Type |
| --- | --- |
| `ownerId` | base58 or 64-character hexadecimal account ID |
| `positionNonce` | exact `u64` decimal string |
| `initialCollateralAmount` | exact `u128` decimal string |
| `userCollateralHoldingId` | base58 or 64-character hexadecimal account ID |

The module derives the Position and Vault PDAs from the configured Stablecoin
Program ID, owner, and nonce. It reads and validates Protocol Parameters,
collateral definition, source holding, and canonical `CLOCK_01`; rejects a
frozen protocol or mismatched collateral/token program; and verifies both
derived accounts are uninitialized. `ownerId` and `userCollateralHoldingId`
must both be public accounts controlled by the connected wallet, and both
sign. Submission uses the shared transaction result envelope.

Account order is owner, Position, Vault, source collateral holding, collateral
definition, Protocol Parameters, `CLOCK_01`. No numeric JSON values are
accepted for the nonce or collateral amount. Those fields must be decimal
strings; fractions, signs, malformed digits, and values outside `u64`/`u128`
return `invalid_numeric_value`.

### `depositCollateral(request)`

Deposits collateral into an existing Position. Required request fields:

| Field | Type |
| --- | --- |
| `ownerId` | base58 or 64-character hexadecimal account ID |
| `positionNonce` | exact `u64` decimal string |
| `userCollateralHoldingId` | base58 or 64-character hexadecimal account ID |
| `amount` | exact `u128` decimal string or lossless JSON integer |

The module derives the Position and Vault PDAs from the configured Stablecoin
Program ID, owner, and nonce. It reads Protocol Parameters, Position, Vault,
and the source holding, then validates their canonical addresses, stored
Position identity, collateral definition, and Token Program ownership. The
source holding must have enough balance. `ownerId` and
`userCollateralHoldingId` must both be public accounts controlled by the
connected wallet, and both sign, including for zero-amount deposits.

Account order is owner, Position, Vault, source collateral holding, and
Protocol Parameters. The Position's collateral is reconciled from the live
Vault balance plus `amount`; it is not incremented from its stored collateral
field. Thus `amount: "0"` absorbs a direct Vault donation, even while the
protocol is frozen. The operation does not require an oracle or clock and does
not apply the collateralization gate. The instruction moves `amount` through
the chained Token Program transfer and leaves normalized debt unchanged.

Use decimal strings for portable exact integers. JSON floats, negative values,
malformed decimals, and values outside `u64`/`u128` return
`invalid_numeric_value`. Other preflight failures use stable errors including
`position_pda_mismatch`, `position_owner_mismatch`, `position_nonce_mismatch`,
`position_vault_mismatch`, `vault_pda_mismatch`,
`invalid_user_collateral_holding`, `collateral_definition_mismatch`,
`token_program_mismatch`, `insufficient_collateral_balance`, and
`collateral_amount_overflow`. Failed preflight never submits a transaction.

### `repayDebt(request)`

Burns the requested stablecoins and reduces an existing Position's normalized
debt. Required request fields:

| Field | Type |
| --- | --- |
| `ownerId` | base58 or 64-character hexadecimal account ID |
| `positionNonce` | exact `u64` decimal string |
| `userStablecoinHoldingId` | base58 or 64-character hexadecimal account ID |
| `amount` | exact `u128` decimal string or lossless JSON integer |

The module derives the Position and global account addresses from the configured
program ID, reads current Protocol Parameters, Position, stablecoin definition,
source holding, Stability Fee Accumulator, and canonical `CLOCK_01`, and validates
their identities, owners, exact data, and token bindings. Both `ownerId` and
`userStablecoinHoldingId` must be public accounts controlled by the active wallet;
both sign, including for zero amounts. The source holding must cover `amount`.

Account order is owner, Position, stablecoin definition, source stablecoin
holding, Stability Fee Accumulator, Protocol Parameters, `CLOCK_01`. The program
burns exactly `amount` and reduces normalized debt by
`floor(amount * FIXED_POINT_ONE / current_accumulator)`. The accumulator uses
the current canonical clock, saturating elapsed time and the seven-day window
clamp. The floored reduction must not exceed remaining normalized debt.
Repayment remains available while frozen and reads no redemption price,
collateral vault, or market-price oracle.

A positive burn can reduce normalized debt by zero; it is not rejected as dust
or rounded up. The API does not cap the amount to a separately floored nominal
debt value and does not offer automatic repay-all behavior. Decimal strings are
the portable exact format. Floats, negative amounts, malformed decimals, and
values outside `u64`/`u128` are rejected.

Preflight errors include `invalid_numeric_value`, `position_pda_mismatch`,
`position_owner_mismatch`, `position_nonce_mismatch`,
`stablecoin_definition_mismatch`, `invalid_stablecoin_definition`,
`invalid_user_stablecoin_holding`, `token_program_mismatch`,
`insufficient_stablecoin_balance`, `repay_amount_exceeds_debt`, and
`repayment_arithmetic_error` for unrepresentable program arithmetic.
Missing initialization returns `not_initialized`; malformed or missing reads
and missing public wallet signers prevent submission. Success returns
`{status: "ok", error: "", transactionId: "..."}`. Wallet rejection or transport
failure returns `wallet_submission_failed`, without a transaction ID or retry.

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
