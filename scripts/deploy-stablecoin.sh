#!/usr/bin/env bash
#
# deploy-stablecoin.sh
# --------------------
# Deploy the stablecoin program and bootstrap it — collateral token, market price
# oracle account and `initialize_program` — against whatever sequencer the active
# `wallet` config points at. Network-agnostic: the same script serves a local dev
# sequencer, the AMM test environment (setup-amm-testnet.sh calls it), and
# testnet.
#
# BOOTSTRAP ORDER: `initialize_program` requires an ALREADY-INITIALIZED
# `OraclePriceAccount` whose base asset is the stablecoin definition — a PDA that
# `initialize_program` itself creates. The PDA is deterministic, so this script
# derives it first, creates the oracle price account quoting it, and only then
# initializes the program.
#
# THE ORACLE IS STATIC: `create_oracle_price_account` only requires its price
# source to sign, so a plain wallet account (STABLECOIN_ORACLE_SOURCE) owns the
# price. It starts at STABLECOIN_ORACLE_INITIAL_PRICE and never moves.
# `generate_debt` rejects it once it is older than
# STABLECOIN_MAXIMUM_ORACLE_PRICE_AGE_MS (default: one day, the §8 maximum). Fine
# for dev and tests; a production deployment wants an AMM pool as the source.
#
# PRICE SCALE: the initial price is 27-decimal fixed point (1.0 = 10^27) because
# that is how the stablecoin reads `OraclePriceAccount::price`. Do NOT refresh it
# with the TWAP oracle's `publish_price`: that writes Q64.64 (1.0 = 2^64), which
# the stablecoin currently misreads as ~1.8e-8.
#
# COLLATERAL: with COLLATERAL_HOLDING set, the script creates the collateral token
# at COLLATERAL_DEFINITION (deploying the token program first). If that fails
# because the token already exists, it is reused: the script confirms the
# definition is on-chain before skipping, so an unrelated failure (unreachable
# sequencer, unfunded account, confirmation timeout) still stops the run. Without
# COLLATERAL_HOLDING it uses an existing token — e.g. a canonical testnet one —
# and never deploys the token program.
#
# Program ids and every PDA are DERIVED at runtime from the binaries, so this
# stays correct across guest rebuilds (new ImageID => new program id => new PDAs).
#
# Prerequisites (managed by you, outside this script):
#   - `wallet` and `spel` on PATH (from the SPEL toolchain), `cargo`
#   - the wallet (LEE_WALLET_HOME_DIR) configured, funded, and holding the admin
#     and oracle-source accounts
#   - guest binaries built (`make build-programs`) and IDLs present (`make idl`)
#
# Required inputs (each accepts a base58 account id or a wallet account label):
#   STABLECOIN_ADMIN            signs initialize_program; becomes the admin
#   STABLECOIN_ORACLE_SOURCE    signs create_oracle_price_account; owns the price
#   COLLATERAL_DEFINITION       the fungible collateral token definition
#
# Optional:
#   COLLATERAL_HOLDING          create the collateral token, minting its supply
#                               here. Both it and COLLATERAL_DEFINITION must then
#                               be wallet accounts: the token program has both sign.
#
# Usage (from anywhere in the repo):
#   # create (or reuse) a collateral token held by wallet accounts:
#   STABLECOIN_ADMIN=admin STABLECOIN_ORACLE_SOURCE=oracle-source \
#     COLLATERAL_DEFINITION=collateral-def COLLATERAL_HOLDING=collateral-holding \
#     scripts/deploy-stablecoin.sh
#   # use an existing collateral token:
#   STABLECOIN_ADMIN=admin STABLECOIN_ORACLE_SOURCE=oracle-source \
#     COLLATERAL_DEFINITION=<base58> scripts/deploy-stablecoin.sh
#
# Everything else has a default; see CONFIG below. Re-running against a sequencer
# where the stablecoin is already initialized fails at the oracle / initialize
# step (those PDAs already exist) — rebuild the binary or use a fresh sequencer.
# The collateral step alone is safe to repeat.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${REPO_ROOT:-$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || (cd "$SCRIPT_DIR/.." && pwd))}"
cd "$REPO_ROOT"

###############################################################################
# CONFIG
###############################################################################

# `make build-programs` writes target/guest/<program>.bin; older per-guest
# `cargo risczero build` output lives under methods/guest/target/.../docker/.
# Prefer the former, fall back to the latter.
guest_bin() {
  local program="$1"
  local shared="target/guest/$program.bin"
  local legacy="programs/$program/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/$program.bin"
  if [ -f "$shared" ] || [ ! -f "$legacy" ]; then printf '%s' "$shared"; else printf '%s' "$legacy"; fi
}

STABLECOIN_BIN="${STABLECOIN_BIN:-$(guest_bin stablecoin)}"
TWAP_BIN="${TWAP_BIN:-$(guest_bin twap_oracle)}"
TOKEN_BIN="${TOKEN_BIN:-$(guest_bin token)}"
STABLECOIN_IDL="${STABLECOIN_IDL:-artifacts/stablecoin-idl.json}"
TWAP_IDL="${TWAP_IDL:-artifacts/twap_oracle-idl.json}"
TOKEN_IDL="${TOKEN_IDL:-artifacts/token-idl.json}"

# The twap_oracle program must be deployed before its oracle price account can be
# created. Callers that already deployed it (setup-amm-testnet.sh) set this to 1.
SKIP_TWAP_DEPLOY="${SKIP_TWAP_DEPLOY:-0}"
# Only consulted when the collateral is being created (COLLATERAL_HOLDING set).
SKIP_TOKEN_DEPLOY="${SKIP_TOKEN_DEPLOY:-0}"

# --- collateral token (created only when COLLATERAL_HOLDING is set) ---
COLLATERAL_HOLDING="${COLLATERAL_HOLDING:-}"
COLLATERAL_NAME="${COLLATERAL_NAME:-COLLATERAL}"
COLLATERAL_SUPPLY="${COLLATERAL_SUPPLY:-1000000000000000000000}"
# Who may mint more collateral later. Defaults to the admin when unset.
COLLATERAL_MINT_AUTHORITY="${COLLATERAL_MINT_AUTHORITY:-}"

# Canonical LEZ system clock.
CLOCK_ACCOUNT="${CLOCK_ACCOUNT:-4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU}"

# Every rate, ratio and price below is 27-decimal fixed point: 1.0 = 10^27.
FIXED_POINT_ONE="1000000000000000000000000000"

# --- initialize_program parameters (§8 bands are enforced on-chain) ---
# Defaults to the admin when unset.
STABLECOIN_FREEZE_AUTHORITY="${STABLECOIN_FREEZE_AUTHORITY:-}"
# ~5%/year: 1 + 1.5e-12 per millisecond.
STABLECOIN_STABILITY_FEE_PER_MS="${STABLECOIN_STABILITY_FEE_PER_MS:-1000000000001500000000000000}"
STABLECOIN_PROPORTIONAL_GAIN="${STABLECOIN_PROPORTIONAL_GAIN:-1000000000}"
STABLECOIN_INTEGRAL_GAIN="${STABLECOIN_INTEGRAL_GAIN:-0}"
# 150%.
STABLECOIN_MINIMUM_COLLATERALIZATION_RATIO="${STABLECOIN_MINIMUM_COLLATERALIZATION_RATIO:-1500000000000000000000000000}"
STABLECOIN_MINIMUM_MS_BETWEEN_RATE_UPDATES="${STABLECOIN_MINIMUM_MS_BETWEEN_RATE_UPDATES:-60000}"
# One day: the §8 maximum, since nothing refreshes the wallet-driven oracle.
STABLECOIN_MAXIMUM_ORACLE_PRICE_AGE_MS="${STABLECOIN_MAXIMUM_ORACLE_PRICE_AGE_MS:-86400000}"
STABLECOIN_INITIAL_REDEMPTION_PRICE="${STABLECOIN_INITIAL_REDEMPTION_PRICE:-$FIXED_POINT_ONE}"
STABLECOIN_NAME="${STABLECOIN_NAME:-LEZ Stablecoin}"

# --- market price oracle ---
# Collateral per stablecoin, in the 27-decimal fixed point the stablecoin reads
# (not the TWAP oracle's Q64.64 — see PRICE SCALE above). Starts at parity with
# the initial redemption price.
STABLECOIN_ORACLE_INITIAL_PRICE="${STABLECOIN_ORACLE_INITIAL_PRICE:-$FIXED_POINT_ONE}"
# Must be >= the TWAP oracle's OBSERVATIONS_CAPACITY (2048).
STABLECOIN_ORACLE_WINDOW_DURATION="${STABLECOIN_ORACLE_WINDOW_DURATION:-3600000}"

# Everything a client, module or test needs to talk to this deployment.
STABLECOIN_MANIFEST_OUT="${STABLECOIN_MANIFEST_OUT:-target/stablecoin-deployment.json}"

###############################################################################
# Helpers
###############################################################################

BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; CYN=$'\033[36m'; RST=$'\033[0m'

hr()  { printf '%s\n' "${DIM}────────────────────────────────────────────────────────────────────────${RST}"; }
log() { printf '%s\n' "$*"; }
sec() { hr; printf '%s\n' "${BOLD}${CYN}==> $*${RST}"; hr; }
kv()  { printf '  %-28s %s\n' "$1" "$2"; }
die() { printf '%s\n' "${RED}✗ $*${RST}" >&2; exit 1; }

require_cmd()  { command -v "$1" >/dev/null 2>&1 || die "required command not found on PATH: $1"; }
require_file() { [ -f "$1" ] || die "required file not found: $1 (cwd=$(pwd))"; }
require_var()  { [ -n "${!1:-}" ] || die "$1 is required (a base58 account id or a wallet label) — see the header"; }

# Run a transaction command, streaming its output, then assert the sequencer
# confirmation marker is present. Same contract as setup-amm-testnet.sh.
#   run_tx <strict|soft> "<description>" -- <cmd> [args...]
run_tx() {
  local mode="$1"; shift
  local desc="$1"; shift
  [ "$1" = "--" ] && shift

  sec "TX: $desc"
  log "${DIM}\$ $*${RST}"
  local tmp; tmp="$(mktemp)"

  set +e
  "$@" 2>&1 | tee "$tmp"
  local rc=${PIPESTATUS[0]}
  set -e

  if [ "$rc" -ne 0 ]; then
    rm -f "$tmp"
    die "command exited with status $rc — $desc"
  fi

  if grep -q "Transaction confirmed" "$tmp" && grep -q "included in a block" "$tmp"; then
    log "${GRN}✅ CONFIRMED — included in a block: ${desc}${RST}"
    rm -f "$tmp"
    return 0
  fi

  rm -f "$tmp"
  if [ "$mode" = "soft" ]; then
    log "${YEL}⚠ no '✅ Transaction confirmed — included in a block.' marker for: ${desc}"
    log "  (continuing — some commands don't print the spel marker; verify manually)${RST}"
    return 0
  fi
  die "NOT CONFIRMED — expected '✅ Transaction confirmed — included in a block.' for: $desc"
}

# Like run_tx, but reports failure instead of dying: 0 if the sequencer
# confirmed the transaction, 1 otherwise. For steps whose failure is recoverable.
#   try_tx "<description>" -- <cmd> [args...]
try_tx() {
  local desc="$1"; shift
  [ "$1" = "--" ] && shift

  sec "TX: $desc"
  log "${DIM}\$ $*${RST}"
  local tmp rc; tmp="$(mktemp)"

  set +e
  "$@" 2>&1 | tee "$tmp"
  rc=${PIPESTATUS[0]}
  set -e

  if [ "$rc" -eq 0 ] && grep -q "Transaction confirmed" "$tmp" && grep -q "included in a block" "$tmp"; then
    rm -f "$tmp"
    log "${GRN}✅ CONFIRMED — included in a block: ${desc}${RST}"
    return 0
  fi
  rm -f "$tmp"
  return 1
}

# Deploy a program; a failure is skipped as "already deployed". Nothing is taken
# on trust: every program is exercised by a later step that fails loudly if it
# is missing (token: the collateral definition check; twap_oracle: the oracle
# account; stablecoin: initialize_program).
deploy_program() {
  local name="$1" bin="$2"
  try_tx "deploy $name program" -- wallet deploy-program "$bin" \
    || log "${YEL}⚠ deploy failed — assuming $name is already deployed (verified by a later step)${RST}"
}

# Whether a token definition is initialized on-chain. Relies on `spel inspect`
# failing for an account that holds no TokenDefinition; if a spel version ever
# exits 0 there instead, initialize_program still rejects a missing collateral
# ("Collateral definition account must be initialized"), just one step later.
token_definition_exists() {
  spel --idl "$TOKEN_IDL" inspect "$1" --type TokenDefinition >/dev/null 2>&1
}

inspect() {
  local idl="$1" addr="$2" type="$3"
  sec "INSPECT: $type @ $addr"
  log "${DIM}\$ spel --idl $idl inspect $addr --type $type${RST}"
  spel --idl "$idl" inspect "$addr" --type "$type"
}

# Extract a 64-char hex program id from `spel -- program-id <bin>`.
program_id() {
  local bin="$1" out pid
  out="$(spel -- program-id "$bin" 2>&1)" || { echo "$out" >&2; die "spel program-id failed for $bin"; }
  pid="$(printf '%s' "$out" | grep -oiE '[0-9a-f]{64}' | head -n1 || true)"
  [ -n "$pid" ] || { echo "$out" >&2; die "could not parse a 64-char program id from spel output for $bin"; }
  printf '%s' "$pid"
}

# Resolve an input that is either a base58 account id or a wallet label.
resolve_account() {
  local value="$1" out
  if [[ "$value" =~ ^[1-9A-HJ-NP-Za-km-z]{32,44}$ ]]; then
    printf '%s' "$value"
    return 0
  fi
  out="$(wallet account id --account-id "$value" 2>/dev/null)" || return 1
  printf '%s' "$out" | grep -oE '[1-9A-HJ-NP-Za-km-z]{32,44}' | head -n1
}

# Absolute form of a path that may be repo-relative.
abs_path() { case "$1" in /*) printf '%s' "$1" ;; *) printf '%s' "$REPO_ROOT/$1" ;; esac; }

# Pick the value for key $1 out of "key value" lines in $2.
field() { printf '%s' "$2" | awk -v k="$1" '$1==k {print $2; exit}'; }

###############################################################################
# 0. Preflight
###############################################################################
sec "Preflight"
require_cmd wallet
require_cmd spel
require_cmd cargo
require_var STABLECOIN_ADMIN
require_var STABLECOIN_ORACLE_SOURCE
require_var COLLATERAL_DEFINITION
require_file "$STABLECOIN_BIN"; require_file "$TWAP_BIN"
require_file "$STABLECOIN_IDL"; require_file "$TWAP_IDL"; require_file "$TOKEN_IDL"
if [ -n "$COLLATERAL_HOLDING" ]; then require_file "$TOKEN_BIN"; fi
kv "repo root"       "$REPO_ROOT"
kv "wallet home"     "${LEE_WALLET_HOME_DIR:-<wallet default>}"
kv "stablecoin bin"  "$STABLECOIN_BIN"
kv "twap_oracle bin" "$TWAP_BIN"
if [ -n "$COLLATERAL_HOLDING" ]; then kv "token bin" "$TOKEN_BIN"; fi

###############################################################################
# 1. Resolve accounts
###############################################################################
sec "Resolve accounts"
ADMIN="$(resolve_account "$STABLECOIN_ADMIN")" || die "cannot resolve STABLECOIN_ADMIN=$STABLECOIN_ADMIN"
ORACLE_SOURCE="$(resolve_account "$STABLECOIN_ORACLE_SOURCE")" \
  || die "cannot resolve STABLECOIN_ORACLE_SOURCE=$STABLECOIN_ORACLE_SOURCE"
COLLATERAL_DEF="$(resolve_account "$COLLATERAL_DEFINITION")" \
  || die "cannot resolve COLLATERAL_DEFINITION=$COLLATERAL_DEFINITION"
FREEZE_AUTHORITY="$ADMIN"
if [ -n "$STABLECOIN_FREEZE_AUTHORITY" ]; then
  FREEZE_AUTHORITY="$(resolve_account "$STABLECOIN_FREEZE_AUTHORITY")" \
    || die "cannot resolve STABLECOIN_FREEZE_AUTHORITY=$STABLECOIN_FREEZE_AUTHORITY"
fi
COLLATERAL_HOLDING_ID=""; COLLATERAL_MINT_AUTHORITY_ID=""
if [ -n "$COLLATERAL_HOLDING" ]; then
  COLLATERAL_HOLDING_ID="$(resolve_account "$COLLATERAL_HOLDING")" \
    || die "cannot resolve COLLATERAL_HOLDING=$COLLATERAL_HOLDING"
  COLLATERAL_MINT_AUTHORITY_ID="$ADMIN"
  if [ -n "$COLLATERAL_MINT_AUTHORITY" ]; then
    COLLATERAL_MINT_AUTHORITY_ID="$(resolve_account "$COLLATERAL_MINT_AUTHORITY")" \
      || die "cannot resolve COLLATERAL_MINT_AUTHORITY=$COLLATERAL_MINT_AUTHORITY"
  fi
fi
for v in ADMIN ORACLE_SOURCE COLLATERAL_DEF FREEZE_AUTHORITY; do
  [ -n "${!v}" ] || die "failed to resolve account id for $v"
done
kv "admin"                 "$ADMIN"
kv "freeze authority"      "$FREEZE_AUTHORITY"
kv "oracle source"         "$ORACLE_SOURCE"
kv "collateral definition" "$COLLATERAL_DEF"
if [ -n "$COLLATERAL_HOLDING_ID" ]; then
  kv "collateral holding"        "$COLLATERAL_HOLDING_ID"
  kv "collateral mint authority" "$COLLATERAL_MINT_AUTHORITY_ID"
fi

###############################################################################
# 2. Deploy programs
###############################################################################
# The token program is only needed here to create the collateral; an existing
# collateral token already lives under its own token program.
if [ -n "$COLLATERAL_HOLDING_ID" ] && [ "$SKIP_TOKEN_DEPLOY" != "1" ]; then
  deploy_program token "$TOKEN_BIN"
fi
if [ "$SKIP_TWAP_DEPLOY" != "1" ]; then
  deploy_program twap_oracle "$TWAP_BIN"
fi
deploy_program stablecoin "$STABLECOIN_BIN"

###############################################################################
# 3. Collateral token — create it, or confirm the existing one
###############################################################################
if [ -n "$COLLATERAL_HOLDING_ID" ]; then
  if try_tx "create collateral token: $COLLATERAL_NAME" -- \
       spel --idl "$TOKEN_IDL" --program "$TOKEN_BIN" -- new-fungible-definition \
         --name "$COLLATERAL_NAME" --total-supply "$COLLATERAL_SUPPLY" \
         --definition-target-account "$COLLATERAL_DEF" \
         --holding-target-account "$COLLATERAL_HOLDING_ID" \
         --mint-authority "$COLLATERAL_MINT_AUTHORITY_ID"; then
    :
  elif token_definition_exists "$COLLATERAL_DEF"; then
    log "${YEL}⚠ collateral token already exists at $COLLATERAL_DEF — reusing it${RST}"
  else
    die "could not create the collateral token, and none exists at $COLLATERAL_DEF (see the output above)"
  fi
else
  sec "Collateral token (existing)"
  token_definition_exists "$COLLATERAL_DEF" \
    || die "no token definition at COLLATERAL_DEFINITION=$COLLATERAL_DEF — set COLLATERAL_HOLDING (a wallet account) to create one"
  kv "found" "$COLLATERAL_DEF"
fi
inspect "$TOKEN_IDL" "$COLLATERAL_DEF" "TokenDefinition"

###############################################################################
# 4. Derive program ids and PDAs
###############################################################################
sec "Program ids (derived from the binaries)"
STABLECOIN_PID="$(program_id "$STABLECOIN_BIN")"; kv "stablecoin program id"  "$STABLECOIN_PID"
TWAP_PID="$(program_id "$TWAP_BIN")";             kv "twap_oracle program id" "$TWAP_PID"

sec "Stablecoin globals (stablecoin_pdas example)"
log "${DIM}\$ cargo run -q -p stablecoin_program --example stablecoin_pdas -- $STABLECOIN_PID globals${RST}"
GLOBALS="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p stablecoin_program --example stablecoin_pdas -- \
             "$STABLECOIN_PID" globals)"
printf '%s\n' "$GLOBALS"
PROTOCOL_PARAMETERS="$(field protocol_parameters "$GLOBALS")"
STABILITY_FEE_ACCUMULATOR="$(field stability_fee_accumulator "$GLOBALS")"
REDEMPTION_PRICE_STATE="$(field redemption_price_state "$GLOBALS")"
STABLECOIN_DEFINITION="$(field stablecoin_definition "$GLOBALS")"
STABLECOIN_MASTER_HOLDING="$(field stablecoin_master_holding "$GLOBALS")"

sec "Market price oracle account (twap_oracle_pdas example)"
log "${DIM}\$ cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- $TWAP_PID $ORACLE_SOURCE $STABLECOIN_ORACLE_WINDOW_DURATION${RST}"
ORACLE_PDAS="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- \
                 "$TWAP_PID" "$ORACLE_SOURCE" "$STABLECOIN_ORACLE_WINDOW_DURATION")"
printf '%s\n' "$ORACLE_PDAS"
MARKET_PRICE_ORACLE="$(field oracle_price_account "$ORACLE_PDAS")"

for v in PROTOCOL_PARAMETERS STABILITY_FEE_ACCUMULATOR REDEMPTION_PRICE_STATE \
         STABLECOIN_DEFINITION STABLECOIN_MASTER_HOLDING MARKET_PRICE_ORACLE; do
  [ -n "${!v}" ] || die "failed to parse PDA $v from example output"
done

###############################################################################
# 5. Create the market price oracle account — BEFORE initialize_program, which
#    requires it to exist and to quote the stablecoin definition derived above.
###############################################################################
run_tx strict "create market price oracle account" -- \
  spel --idl "$TWAP_IDL" --program "$TWAP_BIN" -- create-oracle-price-account \
    --oracle-price-account "$MARKET_PRICE_ORACLE" \
    --price-source "$ORACLE_SOURCE" \
    --clock "$CLOCK_ACCOUNT" \
    --base-asset "$STABLECOIN_DEFINITION" \
    --quote-asset "$COLLATERAL_DEF" \
    --initial-price "$STABLECOIN_ORACLE_INITIAL_PRICE" \
    --window-duration "$STABLECOIN_ORACLE_WINDOW_DURATION"

inspect "$TWAP_IDL" "$MARKET_PRICE_ORACLE" "OraclePriceAccount"

###############################################################################
# 6. Initialize the stablecoin program
###############################################################################
run_tx strict "initialize stablecoin program" -- \
  spel --idl "$STABLECOIN_IDL" --program "$STABLECOIN_BIN" -- initialize-program \
    --admin "$ADMIN" \
    --protocol-parameters "$PROTOCOL_PARAMETERS" \
    --stability-fee-accumulator "$STABILITY_FEE_ACCUMULATOR" \
    --redemption-price-state "$REDEMPTION_PRICE_STATE" \
    --stablecoin-definition "$STABLECOIN_DEFINITION" \
    --stablecoin-master-holding "$STABLECOIN_MASTER_HOLDING" \
    --collateral-definition "$COLLATERAL_DEF" \
    --market-price-oracle "$MARKET_PRICE_ORACLE" \
    --clock "$CLOCK_ACCOUNT" \
    --freeze-authority-account-id "$FREEZE_AUTHORITY" \
    --initial-stability-fee-per-millisecond "$STABLECOIN_STABILITY_FEE_PER_MS" \
    --initial-controller-proportional-gain "$STABLECOIN_PROPORTIONAL_GAIN" \
    --initial-controller-integral-gain "$STABLECOIN_INTEGRAL_GAIN" \
    --initial-minimum-collateralization-ratio "$STABLECOIN_MINIMUM_COLLATERALIZATION_RATIO" \
    --minimum-milliseconds-between-rate-updates "$STABLECOIN_MINIMUM_MS_BETWEEN_RATE_UPDATES" \
    --maximum-oracle-price-age-milliseconds "$STABLECOIN_MAXIMUM_ORACLE_PRICE_AGE_MS" \
    --initial-redemption-price "$STABLECOIN_INITIAL_REDEMPTION_PRICE" \
    --stablecoin-name "$STABLECOIN_NAME"

###############################################################################
# 7. Verify
###############################################################################
inspect "$STABLECOIN_IDL" "$PROTOCOL_PARAMETERS"       "ProtocolParameters"
inspect "$STABLECOIN_IDL" "$STABILITY_FEE_ACCUMULATOR" "StabilityFeeAccumulator"
inspect "$STABLECOIN_IDL" "$REDEMPTION_PRICE_STATE"    "RedemptionPriceState"
inspect "$STABLECOIN_IDL" "$STABLECOIN_DEFINITION"     "TokenDefinition"

###############################################################################
# 8. Write the deployment manifest
###############################################################################
sec "Write deployment manifest -> $STABLECOIN_MANIFEST_OUT"
mkdir -p "$(dirname "$STABLECOIN_MANIFEST_OUT")"
cat > "$STABLECOIN_MANIFEST_OUT" <<JSON
{
  "stablecoinBin": "$STABLECOIN_BIN",
  "stablecoinIdl": "$STABLECOIN_IDL",
  "stablecoinProgramId": "$STABLECOIN_PID",
  "twapOracleBin": "$TWAP_BIN",
  "twapOracleIdl": "$TWAP_IDL",
  "twapOracleProgramId": "$TWAP_PID",
  "admin": "$ADMIN",
  "freezeAuthority": "$FREEZE_AUTHORITY",
  "protocolParameters": "$PROTOCOL_PARAMETERS",
  "stabilityFeeAccumulator": "$STABILITY_FEE_ACCUMULATOR",
  "redemptionPriceState": "$REDEMPTION_PRICE_STATE",
  "stablecoinDefinition": "$STABLECOIN_DEFINITION",
  "stablecoinMasterHolding": "$STABLECOIN_MASTER_HOLDING",
  "collateralDefinition": "$COLLATERAL_DEF",
  "collateralHolding": "$COLLATERAL_HOLDING_ID",
  "marketPriceOracle": "$MARKET_PRICE_ORACLE",
  "marketPriceOracleSource": "$ORACLE_SOURCE",
  "marketPriceOracleWindowDuration": $STABLECOIN_ORACLE_WINDOW_DURATION,
  "clock": "$CLOCK_ACCOUNT"
}
JSON
kv "wrote" "$STABLECOIN_MANIFEST_OUT"

sec "Done"
log "${GRN}✅ Stablecoin deployed and initialized.${RST}"
kv "stablecoin program id" "$STABLECOIN_PID"
kv "stablecoin definition" "$STABLECOIN_DEFINITION"
kv "market price oracle"   "$MARKET_PRICE_ORACLE"
log ""
log "Point the stablecoin module at this deployment with either of:"
log "  ${DIM}STABLECOIN_PROGRAM_BIN=$(abs_path "$STABLECOIN_BIN")${RST}"
log "  ${DIM}STABLECOIN_PROGRAM_ID=$STABLECOIN_PID${RST}"
log ""
log "The oracle price is static and goes stale after ${STABLECOIN_MAXIMUM_ORACLE_PRICE_AGE_MS} ms;"
log "generate_debt rejects it past that. Do not refresh it with the TWAP oracle's"
log "publish_price — it writes Q64.64, which the stablecoin misreads (see the header)."
