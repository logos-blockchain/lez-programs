# Stablecoin core module

`stablecoin_module` is a headless Logos `core` module for the LEZ Stablecoin
Program. It exposes deployment discovery, protocol-parameter and position
reads, and protocol initialization through the same universal API used by
`logoscore` and UI modules.

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

### `positionAccount(request)`

Required request fields:

| Field | Type |
| --- | --- |
| `ownerId` | base58 or 64-character hexadecimal account ID |
| `positionNonce` | exact `u64` decimal string |

The module derives the position PDA from `(ownerId, positionNonce)`, derives
the position's collateral-vault PDA, and performs one direct public-account
read. It does not enumerate wallet or global accounts.

On success, the response adds a `position` object containing the owner,
position, and vault IDs in base58 and lowercase hexadecimal form. It also
returns `positionNonce`, `collateralAmount`, `normalizedDebtAmount`, and
`openedAt` as exact decimal strings. The account owner, address, stored owner,
stored nonce, and stored vault must all match the derived identity.

When the derived position account does not exist, the method returns
`{ "status": "error", "error": "not_found" }` and still adds `position` with
the derived owner, position, and vault IDs. No account decoder runs for this
ordinary absence case.

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
