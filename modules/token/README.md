# Token core module

`token_module` is a headless Logos `core` module for the LEZ Token Program. It
has no UI. Basecamp UI modules and `logoscore` use the same generated API.

The module supports all current Token Program instructions, including fixed or
mintable fungibles, metadata-backed fungibles, non-fungible definitions, NFT
printing, transfers, minting, burning, and authority changes. It also decodes
Token Definition, Token Holding, and Token Metadata accounts.

## Architecture

```text
Basecamp UI / logoscore
          |
     token_module       Qt-free C++ transport/orchestration
       /      \
token_ffi      logos_execution_zone
Rust codecs    shared wallet reads, account listing, transaction submission
and planners
```

`token_ffi` constructs `token_core::Instruction` values directly and serializes
them with RISC Zero. This is intentional: the current Token IDL cannot fully
describe `new_definition_with_metadata`, and its final three instruction
indexes do not match the Rust enum's serialized order.

The module reuses the host-loaded `logos_execution_zone` instance. It never
opens a second wallet and never creates account keys. Callers create fresh
public accounts through the wallet module, then pass their IDs to Token
operations.

## API

Every method returns a map. Success starts with:

```json
{ "status": "ok", "error": "" }
```

Failure is:

```json
{ "status": "error", "error": "<stable_code>" }
```

Mutating success adds `transactionId`.

### Read and discovery

| Method | Arguments | Result payload |
| --- | --- | --- |
| `programInfo` | none | Token Program ID in base58 and hex |
| `inspectDefinition` | definition ID | decoded fungible or non-fungible definition |
| `inspectHolding` | holding ID | decoded fungible, NFT master, or NFT printed-copy holding |
| `inspectMetadata` | metadata ID | decoded standard, URI, creators, and primary-sale value |
| `walletTokenAccounts` | none | all uniquely decodable Token-owned accounts in the connected wallet |

Read results expose operator-facing base58 IDs and matching lowercase `*Hex`
fields. Raw supplies, balances, print balances, and primary-sale values are
decimal strings.

There is no global token registry. Explicit inspect methods can read any public
account. `walletTokenAccounts` discovers only accounts present in the connected
wallet.

### Definition creation

| Method | Arguments |
| --- | --- |
| `createFungible` | definition target, holding target, name, total supply raw, mint authority |
| `createFungibleWithMetadata` | definition target, holding target, metadata target, name, total supply raw, mint authority, standard, URI, creators |
| `createNonFungible` | definition target, master holding target, metadata target, name, printable supply raw, standard, URI, creators |

Mint-authority values:

- `none` creates a permanently fixed supply;
- `self` uses the definition account itself;
- an account ID assigns an external authority.

Metadata standard is `simple` or `expanded`.

### Holding and supply operations

| Method | Arguments | Notes |
| --- | --- | --- |
| `initializeHolding` | definition, fresh holding target | NFT definitions create an unowned printed-copy holding |
| `transfer` | sender holding, recipient holding, amount raw | supports fungible, NFT master, and printed-copy rules |
| `burn` | definition, holding, amount raw | supports fungible and NFT variants |
| `mint` | definition, holding, amount raw | self-authority fungible path |
| `mintWithAuthority` | definition, holding, current authority, amount raw | external-authority fungible path |
| `setAuthority` | definition, new authority | self-authority path |
| `setAuthorityWithAuthority` | definition, current authority, new authority | external-authority path |
| `printNft` | master holding, fresh printed holding target | target must not be initialized first |

`new authority` accepts the same `none`, `self`, or account-ID values as
definition creation. Revocation with `none` is permanent.

## Account ID and amount conventions

Account inputs accept base58 or 64 hexadecimal characters. Hex is normalized
to lowercase.

Raw `u128` arguments are exposed as JSON-compatible values:

- small values may be passed as bare integers, for example `1000`;
- large values must be passed to `logoscore` as quote-wrapped decimal strings,
  for example `'"12345678901234567890123456"'`.

The CLI converts large bare numbers to floating point. The module rejects all
floats instead of submitting a rounded amount. A UI should always pass raw
amounts as decimal strings.

## Fresh account workflow

Creation, explicit initialization, and NFT printing require fresh public wallet
accounts. Create each one before calling the token method:

```bash
logoscore call logos_execution_zone create_account_public --json
```

For transfer and mint, an initialized destination needs no destination
signature. A fresh destination is accepted only when the connected wallet owns
its key, so it can authorize the Token Program claim.

The execution-zone provider may represent an absent public account as an
all-zero owner/balance/nonce with empty data. The module treats that exact
response as `not_found`, then still requires the target ID to belong to the
connected wallet before submitting.

## Build and test

Build from repository root; the root flake supplies `token_ffi`:

```bash
RISC0_DEV_MODE=1 cargo +1.94.0 test -p token_ffi
RISC0_SKIP_BUILD=1 cargo +1.94.0 clippy -p token_ffi --all-targets -- -D warnings
nix build path:.#token_ffi -L
nix build path:.#token-module -L
```

`path:.` is useful while new files are untracked. After files are tracked,
`nix build .#token-module` is equivalent.

## Runtime configuration

Set either `TOKEN_PROGRAM_ID` or `TOKEN_PROGRAM_BIN` on the process hosting the
module:

```bash
# Use the deployed Token Program ID directly (base58 or 64-character hex).
TOKEN_PROGRAM_ID=F8sGbDbjcxvJHpUQJcArEaY7EbLMVmqZgRm3fXPw3jb3 \
  logoscore -D -m ./modules

# Or derive the ID from the exact deployable binary.
TOKEN_PROGRAM_BIN=/absolute/path/to/token.bin logoscore -D -m ./modules
```

If both variables are set, they must resolve to the same program ID. The binary
must be the exact deployable `.bin` running on the target sequencer. Its RISC
Zero image ID is the Token Program ID. Rebuilding the guest changes that
identity; accounts owned by an older deployment must be read with the matching
program-ID configuration.

Set `TOKEN_DEBUG=1` on the daemon to emit safe adapter diagnostics to module
stderr. Debug logging never includes wallet storage or recovery material.

## Headless `logoscore` smoke test

### 1. Build both modules

```bash
# Token module (from the repo root; output under result/lib/).
nix build .#token-module -L
ls result/lib/          # token_module_plugin.dylib  libtoken_ffi.dylib

# The wallet module it depends on — the SAME pin the repo-root flake and
# amm_module use, with its inner monorepo input overridden to the rev the target
# sequencer runs (415964d7). See apps/amm/README.md for the fuller build notes.
nix build 'github:gravityblast/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr' \
  --override-input logos-execution-zone \
  'github:logos-blockchain/logos-execution-zone?rev=415964d7f9043a1bfe28da8d0e8b3a6f64abb258' \
  --out-link result-lez
ls result-lez/lib/      # logos_execution_zone_plugin.dylib  libwallet_ffi.dylib
```

### 2. Stage a modules directory

Core modules get no `.lgx` from the builder, so stage a directory by hand — one
subdir per module with `manifest.json` + `variant` + the plugin dylib and its
sibling FFI dylib (rpath is `@loader_path`, so the FFI lib must sit beside the
plugin). The daemon discovers modules from this layout:

```
modules/
  token_module/
    token_module_plugin.dylib
    libtoken_ffi.dylib
    variant                     # one line: darwin-arm64-dev
    manifest.json
  logos_execution_zone/
    logos_execution_zone_plugin.dylib
    libwallet_ffi.dylib
    variant
    manifest.json
```

`token_module/manifest.json`:

```json
{
  "name": "token_module", "type": "core", "version": "0.1.0",
  "manifestVersion": "0.2.0", "dependencies": ["logos_execution_zone"],
  "main": { "darwin-arm64-dev": "token_module_plugin.dylib" }
}
```

Copy the dylibs into place (`result/lib/*` → `modules/token_module/`,
`result-lez/lib/*` → `modules/logos_execution_zone/`) and write each `variant`
as a single line (`darwin-arm64-dev` on arm64 macOS). The
`logos_execution_zone/manifest.json` mirrors this with its own name and plugin.

### 3. Start the daemon and load (dependency first)

Set `TOKEN_PROGRAM_ID` or `TOKEN_PROGRAM_BIN` **on the daemon**, then load the
dependency before the module:

```bash
TOKEN_PROGRAM_ID=F8sGbDbjcxvJHpUQJcArEaY7EbLMVmqZgRm3fXPw3jb3 \
logoscore -D -m ./modules --persistence-path ./data

logoscore load-module logos_execution_zone   # dependency first
logoscore load-module token_module
logoscore module-info token_module --json
logoscore call token_module programInfo --json
logoscore call token_module inspectDefinition deadbeef --json
```

The final call must return `invalid_account_id` without crashing, and
`programInfo` should return the configured or binary-derived ID.

### 4. Chain reads

Open a wallet configured for the target sequencer, then pass a real account ID:

```bash
logoscore call logos_execution_zone open \
  /path/to/wallet_config.json /path/to/storage.json "$WALLET_PASSWORD" --json

logoscore call token_module inspectDefinition \
  7b464ff9dd0d3bc07f7e2e0b0667ccd066d85ad12be4c79fc55687a863910aa6 --json
```

That example ID was a fixed-supply fungible on a historical testnet deployment;
verify `programInfo` matches the intended deployment before interpreting it.
Do not run mutating examples on a shared network without explicit operator
authorization.

## Current limitations

- Public wallet accounts only; no private/shielded token flow.
- No token registry, HTTP metadata fetch, symbol, or decimals model.
- On-chain state can change between a read/preflight and transaction inclusion;
  the Token Program remains final authority.
