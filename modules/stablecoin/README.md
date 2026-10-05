# Stablecoin core module

`stablecoin_module` is a headless Logos `core` module for the LEZ Stablecoin
Program. It exposes deployment discovery, protocol-state reads, protocol
initialization, position health, position opening, collateral deposits, debt
repayment, collateral withdrawal, debt generation, and position closure through
the same universal API used by `logoscore` and UI modules. Admin parameter and
role setters and emergency freeze controls use the same current-state
authorization and transaction path.

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

### `withdrawCollateral(request)`

Accepts `ownerId`, `positionNonce` (exact `u64` decimal string),
`userCollateralHoldingId`, and `amount` (exact `u128` decimal string or lossless
JSON integer). Account IDs accept base58 or hexadecimal. Floats, negative
amounts, malformed decimals and values outside the declared types are rejected.

The module derives Position, Vault and global IDs internally and reads current
Position, Vault, destination holding, Protocol Parameters, Stability Fee
Accumulator, Redemption Price State and canonical `CLOCK_01`. It validates
canonical identities, stablecoin ownership, exact data, stored Position
owner/nonce/vault identity, and collateral definition/Token Program consistency.
The destination must differ from the owner and Vault, preserving the runtime's
distinct-account requirement. Only the Position owner must be a public account
controlled by the active wallet and signs; the destination holding can be
outside that wallet. The program
authorizes the outgoing Token transfer with the Vault PDA seed.

Account order is owner, Position, Vault, destination collateral holding,
Stability Fee Accumulator, Redemption Price State, Protocol Parameters,
`CLOCK_01`. All `init` flags are false. Recorded Position collateral bounds the
amount; a direct vault donation is unavailable until `depositCollateral`
reconciles it. Preflight also checks live vault availability and destination
balance overflow, including inconsistent observations from separate RPC reads.

Frozen withdrawals fail, including zero amounts. For nonzero withdrawals with
nonzero debt, preflight checks the remaining collateral using the program's
wide projections, canonical clock and seven-day clamp:

```text
remainingCollateral * FIXED_POINT_ONE^3 >= normalizedDebt
    * currentAccumulator * currentRedemptionPrice * minimumCollateralizationRatio
```

Requirement multiplication saturates conservatively in U512. A zero projected
redemption price is rejected for an indebted, nonzero withdrawal. No nominal
debt flooring, narrowing to `u128`, market-oracle read or controller gate is
used. This operation uses the program's conservative wide projection, rather
than the exact-display requirements of `positionHealth`.

Zero amounts and zero-debt positions skip projections and the health gate, but
still require valid Position, global, clock and token accounts, an unfrozen
protocol and the owner's wallet signature. Every call reads current state;
an earlier health quote does not authorize a later withdrawal.

Stable errors include `protocol_frozen`, `withdraw_amount_exceeds_collateral`,
`position_undercollateralized`, `redemption_price_zero`,
`insufficient_vault_balance`, `collateral_amount_overflow` and the existing
numeric/account/PDA/ownership/token-binding errors. Failed preflight submits
nothing. Success returns `{status: "ok", error: "", transactionId: "..."}`;
wallet rejection or transport failure returns `wallet_submission_failed`
without a successful transaction ID or automatic retry.

### `generateDebt(request)`

Accepts `ownerId`, `positionNonce` (exact `u64` decimal string),
`userStablecoinHoldingId`, and `amount` (exact `u128` decimal string or lossless
JSON integer). Account IDs accept base58/hex; floats, negative amounts,
malformed decimals and out-of-range values are rejected. Only the Position
owner must be a public account in the active wallet and signs. The destination
holding can be outside that wallet and cannot alias owner or definition.

The module reads current Position, stablecoin definition, destination holding,
Stability Fee Accumulator, Redemption Price State, configured market-price
oracle, Protocol Parameters, and canonical `CLOCK_01`. Position and global IDs
are derived internally; definition and oracle come from validated parameters.
It validates identities, stablecoin ownership, exact data, token bindings,
the definition's canonical PDA/self-mint authority, and mint balance/supply
capacity. The stablecoin instruction authorizes its chained Token mint with
the definition PDA seed; no client-issued mint or wallet authority signer is
used.

Account order is owner, Position, stablecoin definition, destination stablecoin
holding, Stability Fee Accumulator, Redemption Price State, market-price
oracle, Protocol Parameters, `CLOCK_01`. All `init` flags are false.

Borrowing rejects frozen state. The oracle is a liveness gate only: future
observations fail, and age equal to `maximumOraclePriceAgeMilliseconds` is
accepted. Its price is not used in debt/health math; a fresh zero-price oracle
is allowed. No producer-program, asset-pair or controller-update-interval gate
is added beyond the native instruction's checks.

Debt pricing uses the native narrow accumulator algorithm, including each
`u128` compounding step, saturating elapsed time and seven-day clamp:

```text
normalizedDebtDelta = ceil(amount * FIXED_POINT_ONE / currentAccumulator)
newNormalizedDebt = oldNormalizedDebt + normalizedDebtDelta
```

Checked, non-panicking arithmetic reports unrepresentable pricing and debt
addition through stable errors. The wide projected redemption price must be
nonzero, and post-mint health uses the shared fractional-debt/U512 requirement
saturation comparison. No nominal debt is floored before that comparison.
Zero amounts still require unfrozen state, a fresh oracle, valid pricing,
nonzero projected redemption price, health, token accounts and authorization.
Every invocation re-reads current parameters, oracle binding and accounts.

Stable errors include `protocol_frozen`, `oracle_stale`, `oracle_future`,
`debt_pricing_arithmetic_error`, `normalized_debt_overflow`,
`redemption_price_zero`, `position_undercollateralized`,
`invalid_stablecoin_mint_authority`, `stablecoin_supply_overflow`,
`stablecoin_balance_overflow` and the existing numeric/identity/token errors.
Failed preflight submits nothing. Success returns
`{status: "ok", error: "", transactionId: "..."}`; wallet rejection or transport
failure returns `wallet_submission_failed` without a successful ID or retry.

### `closePosition(request)`

Accepts `ownerId` (base58 or hexadecimal account ID) and `positionNonce` (exact
`u64` decimal string). Decimal strings preserve values above `2^53`, including
`u64::MAX`; JSON numbers, floats, negative values, malformed decimals and
out-of-range nonces are rejected. Only the Position owner must be a public
account controlled by the active wallet and signs.

The module derives Position, Vault and Protocol Parameters IDs, reads those
three accounts, and validates their canonical identities, Position/parameters
program ownership, exact data, stored owner/nonce/vault fields and fungible
vault balance. Closure requires zero normalized debt, zero recorded collateral
and zero actual vault balance. It is allowed while frozen and needs no clock,
oracle, accumulator, redemption-price projection or destination holding.

The planner encodes the zero-argument `Instruction::ClosePosition`, without a
nonce payload. Its account contract matches the guest and IDL:

| Account order | Writable | Signer | Init |
| --- | --- | --- | --- |
| owner | No | Yes | No |
| Position | Yes | No | No |
| Vault | No | No | No |
| Protocol Parameters | No | No | No |

Only Position data is cleared. Its program ownership, account nonce and native
balance remain unchanged; the empty Token vault remains intact. Neither account
is released. The same `(owner, positionNonce)` cannot be reopened; choose a new
nonce for a new position. A second closure fails preflight with
`invalid_position_data`, without another submission or fabricated transaction ID.

A direct vault donation blocks closure even if recorded collateral is zero.
`closePosition` does not repay debt, reconcile collateral, sweep donations or
submit any preliminary transactions. Use the separate `depositCollateral`
zero-amount reconciliation and `withdrawCollateral` workflow to recover a
donation first; withdrawal requires the protocol to be unfrozen.

Stable closure errors are `position_has_debt`, `position_has_collateral` and
`vault_not_empty`, plus the existing numeric, account-read, PDA, ownership and
exact-data errors. Failed preflight submits nothing. Success returns
`{status: "ok", error: "", transactionId: "..."}`; wallet rejection or transport
failure returns `wallet_submission_failed` without a successful ID or retry.
Every invocation checks current wallet ownership and reads current state again.

The pure Rust API exports `close_position_plan(ClosePositionPlanRequest)`; C
callers use `stablecoin_close_position_plan` and release its response with
`stablecoin_free`. Its request additionally contains `stablecoinProgramId` and
the `position`, `vault`, and `protocolParameters` account-read envelopes.

### `setStabilityFeePerMillisecond(request)`

Accepts `adminId` (base58 or hexadecimal account ID) and `newRate` (an exact
`u128` fixed-point per-millisecond multiplier). The inclusive native band is
`FIXED_POINT_ONE <= newRate <= 2 * FIXED_POINT_ONE`. Use decimal strings for
portable integers above `2^53`; lossless JSON integers are also accepted, while
floating-point values and values outside `u128` are rejected.

Each call derives and reads current Protocol Parameters, Stability Fee
Accumulator and canonical `CLOCK_01`. Stablecoin singleton PDAs, ownership and
exact account data are validated. The supplied admin must match the currently
stored admin and be a public account controlled by the active wallet. Admin
rotation takes effect on the next call, including on a reused module instance.

The module submits one `Instruction::SetStabilityFeePerMillisecond { new_rate }`
with these accounts, all with `init = false`:

| Order | Account | Writable | Signer |
| --- | --- | --- | --- |
| 1 | Current admin | No | Yes |
| 2 | Protocol Parameters | Yes | No |
| 3 | Stability Fee Accumulator | Yes | No |
| 4 | Canonical `CLOCK_01` | No | No |

The native instruction first accrues the elapsed interval at the old rate,
then replaces the rate atomically. Subsequent accrual uses the new rate. The
module does not submit a preliminary poke. It checks that the old-rate
projection fits the native `u128` arithmetic before submission, preserving
saturating elapsed-time subtraction and the seven-day compounding-window
clamp. Same-time or inverted-time observations accrue no elapsed interval;
the native instruction still writes the observed clock timestamp as
`lastAccruedAt`.

This setter remains available while frozen. Setting the existing rate again
still submits and advances the accumulator. Successful execution preserves
unrelated parameters and redemption state. A rate inside the allowed band
does not guarantee that every future accrual will fit `u128`.

Out-of-band rates return `stability_fee_out_of_band`; unrepresentable old-rate
accrual returns `stability_fee_arithmetic_error`. Current-admin mismatches return
`admin_mismatch`. Existing numeric, wallet, account-read, PDA, ownership and
exact-data errors also apply. Failed preflight submits nothing. Success returns
`{status: "ok", error: "", transactionId: "..."}`; wallet rejection or transport
failure returns `wallet_submission_failed` without a successful ID or retry.

The Rust API exports
`set_stability_fee_per_millisecond_plan(SetStabilityFeePerMillisecondPlanRequest)`.
C callers use `stablecoin_set_stability_fee_per_millisecond_plan`; add
`stablecoinProgramId` and the `protocolParameters`, `stabilityFeeAccumulator`
and `clock` account-read envelopes to the public request. Free responses with
`stablecoin_free`.

### Admin parameter and role setters

All six methods accept `adminId` (base58 or hexadecimal account ID) and the
additional fields below. Numeric fields accept exact decimal strings or
lossless JSON integers; floating-point values and out-of-type values are
rejected. Decimal strings are the portable format for integers above `2^53`,
including the full `u128`/`i128` domain before the native bands are applied.

| Method | Additional fields | Native limits / behavior |
| --- | --- | --- |
| `setMinimumCollateralizationRatio(request)` | `newRatio` (`u128`) | Inclusive `1.1 * FIXED_POINT_ONE ..= 10 * FIXED_POINT_ONE` |
| `setControllerGains(request)` | `newProportionalGain`, `newIntegralGain` (`i128`) | Either sign; magnitude caps `1000 * FIXED_POINT_ONE` and `FIXED_POINT_ONE` respectively |
| `setTimingParameters(request)` | `newMinimumMillisecondsBetweenRateUpdates`, `newMaximumOraclePriceAgeMilliseconds` (`u64`) | Each inclusive `1 ..= 86_400_000` milliseconds |
| `setAdmin(request)` | `newAdminId` (account ID) | Immediate one-step rotation; new holder is an instruction value, not a signer |
| `setFreezeAuthority(request)` | `newFreezeAuthorityId` (account ID) | Authorized by the admin, not merely the current freeze authority |
| `setMarketPriceOracle(request)` | `newOracleId` (account ID) | Initialized exact `OraclePriceAccount` with the bound stablecoin/collateral asset pair |

Each invocation reads current Protocol Parameters, verifies canonical PDA,
stablecoin ownership and exact data, and requires the currently stored admin to
be a public signer in the active wallet. Only that admin signs. All six setters
remain available while frozen and read no clock, accumulator or redemption
state. They do not automatically advance globals.

Account order for the first five setters is admin (read-only, signer), Protocol
Parameters (writable, nonsigner). Oracle replacement appends the replacement
oracle as the third read-only, nonsigner account and uses the zero-argument
`SetMarketPriceOracle` instruction, not an oracle-ID payload. Every `init` flag
is false. A replacement oracle cannot alias the admin input because public
transaction account IDs must be distinct.

Both gains change atomically without resetting the controller integral or
redemption state. Both timing values also change atomically. Each setter changes
only its named fields, preserving unrelated parameters. Tightening the ratio
does not scan existing positions or prevent the update; affected positions may
need deposits or repayment before borrowing or withdrawing again.

Role replacement IDs accept base58/hex, need not belong to the current wallet,
and are not read or required to co-sign. Admin and freeze-authority handles may
be equal. The new role is effective immediately once committed; subsequent
calls re-read parameters, so the old admin cannot keep authorizing operations.
There is no confirmation or rollback step. Verify the replacement carefully:
an incorrect or unusable handle can permanently remove control. Native role
setters accept the all-zero account-ID value too; the module does not add a
nonzero restriction to these instruction values.

Oracle replacement is producer-agnostic. It does not pin the replacement's
program owner or impose freshness, future-timestamp, nonzero-price or
controller-update-interval gates. A well-formed matching pair from another
producer, including a stale or zero observation, is accepted. Wrong asset pairs,
missing or malformed account data fail preflight.

Stable errors include `admin_mismatch`, `collateralization_ratio_out_of_band`,
`controller_gains_out_of_band`, `timing_parameters_out_of_band`, and existing
numeric, account-read, PDA, ownership and oracle-data/asset errors. Failed
preflight submits nothing. Success returns
`{status: "ok", error: "", transactionId: "..."}`. Wallet rejection or transport
failure returns `wallet_submission_failed` without a successful ID or retry.

The Rust API exports six `set_*_plan` functions and their typed `Set*PlanRequest`
types, sharing `AdminPlanContext`. The JSON C ABI request is flat: add
`stablecoinProgramId` and `protocolParameters` to the public request; oracle
replacement also needs the `newOracle` account-read envelope. C entrypoints are
`stablecoin_set_minimum_collateralization_ratio_plan`,
`stablecoin_set_controller_gains_plan`, `stablecoin_set_timing_parameters_plan`,
`stablecoin_set_admin_plan`, `stablecoin_set_freeze_authority_plan`, and
`stablecoin_set_market_price_oracle_plan`. Release response allocations with
`stablecoin_free`.

### `freeze(request)` and `unfreeze(request)`

Accept `freezeAuthorityId` (base58 or hexadecimal account ID), with no numeric
arguments or caller-selected boolean. The module derives the Protocol Parameters
PDA, reads current parameters for every operation, verifies stablecoin ownership
and exact data, and requires the currently stored freeze authority to be a
public account controlled by the active wallet. Admin status alone is not
sufficient; a shared admin/freeze handle is accepted because it matches the
stored freeze-authority binding. Role rotation is picked up on the next call,
even on a reused module instance.

`freeze` encodes the zero-argument `Instruction::Freeze`; `unfreeze` encodes
`Instruction::Unfreeze`. Both use freeze authority (read-only, signer) followed
by Protocol Parameters (writable, nonsigner), with every `init` flag false.
Only `is_frozen` changes. No clock, oracle, Position, projection or chained Token
call is involved, and callers cannot select another parameters-account ID.

Both operations are idempotent on-chain: repeated freeze of a frozen protocol
and repeated unfreeze of an unfrozen protocol remain valid. Each request still
validates live authority/state and submits once, returning the wallet's actual
transaction result. The module never fabricates an ID for a local no-op and
does not treat the frozen flag as a submission blocker for these operations.

While frozen, `openPosition`, `withdrawCollateral` and `generateDebt` fail with
`protocol_frozen`, including zero-amount calls. `depositCollateral`, `repayDebt`,
`closePosition` and permissionless maintenance remain available when their own
preconditions are met. Unfreeze restores the gated paths; it does not bypass
their ordinary account, balance, health, oracle or wallet checks.

Wrong role binding returns `freeze_authority_mismatch`. Missing parameters
return `not_initialized`; failed reads return `account_read_failed`. Existing
request, account-ID, PDA, ownership and exact-data errors remain
stable. Failed preflight submits nothing. Success returns
`{status: "ok", error: "", transactionId: "..."}`; wallet rejection or transport
failure returns `wallet_submission_failed`, without a successful ID or retry.

The Rust API exports `freeze_plan` and `unfreeze_plan`, sharing
`FreezeAuthorityPlanRequest`. C callers use `stablecoin_freeze_plan` and
`stablecoin_unfreeze_plan`; add `stablecoinProgramId` and the canonical
`protocolParameters` account-read envelope to the public request, and free
responses with `stablecoin_free`.

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
