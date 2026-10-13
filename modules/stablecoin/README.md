# Stablecoin core module

`stablecoin_module` is a headless Logos `core` module for the LEZ Stablecoin
Program. It exposes deployment discovery, protocol-parameter reads, and
protocol initialization through the same universal API used by `logoscore` and
UI modules.

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
ID is returned in base58 and lowercase hexadecimal form. `imageIdHex` is the
configured binary's ImageID, or empty when no binary is configured.

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
| `feePayerId` | funded public wallet account that pays the transaction fee |

The module verifies all five derived target PDAs are uninitialized, validates
the collateral definition, oracle asset pair, and clock accounts, then submits
the exact nine-account instruction. Only `adminId` signs. LEZ charges public
transactions, so `feePayerId` must name a funded account in the connected
wallet: the admin itself or a separate account, which co-signs the fee. A
non-signing participant is rejected with `invalid_fee_payer`. Success adds
`transactionId` to the response envelope.

Pass numeric values as decimal strings. JSON integers are accepted when their
exact value survives parsing. JSON floating-point values are always rejected.

## Runtime configuration

Set `STABLECOIN_PROGRAM_ID` on the process hosting the module, optionally with
`STABLECOIN_PROGRAM_BIN` to check the deployed bytecode:

```bash
STABLECOIN_PROGRAM_ID=<base58-or-hex-program-id>
STABLECOIN_PROGRAM_BIN=/absolute/path/to/stablecoin.bin
```

Since LEZ v0.2.5 a program lives at the account its `program_loader` header
was deployed to, chosen at deploy time, so only `STABLECOIN_PROGRAM_ID` can
name it. The binary's RISC Zero ImageID identifies bytecode, not an address:
when `STABLECOIN_PROGRAM_BIN` is set, the module reads the header at
`STABLECOIN_PROGRAM_ID` and requires its `image_id` to match
(`program_binary_mismatch`, or `program_header_unavailable` when there is no
loader-owned header there). A binary alone leaves the module `config_missing`.

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
