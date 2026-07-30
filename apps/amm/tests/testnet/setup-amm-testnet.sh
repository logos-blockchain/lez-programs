#!/usr/bin/env bash
#
# setup-amm-testnet.sh
# --------------------
# Deploy the token/amm/twap programs, mint two fungible tokens, initialize the
# AMM, and create a pool — from scratch — against whatever sequencer your
# `wallet` / `spel` config points at. This is the prerequisite state that the
# AMM UI swap test (apps/amm/tests/swap.mjs) exercises; run it once to stand up
# a swappable pool, then launch the UI / run the test.
#
# DETERMINISTIC TEST WALLET: by default the script bootstraps an ISOLATED wallet
# (git-ignored, under this folder) by restoring it from a fixed BIP-39 mnemonic
# and creating its accounts in a fixed order. The wallet's BIP-32 key tree makes
# those account ids reproducible across machines, so the whole team gets the
# same token/holding ids — and the script auto-writes apps/amm/amm-tokens.json
# from them for the UI. It never touches your personal ~/.lee/wallet.
#
# Program IDs and all AMM PDAs (config, pool, vaults, LP, tick) are DERIVED at
# runtime from the deployed binaries + the token definition accounts, so this
# script stays correct even if you rebuild the guest binaries (new image id =>
# new program id => new PDAs).
#
# Prerequisites (managed by you, outside this script):
#   - `wallet` and `spel` on PATH (from the SPEL toolchain)
#   - a reachable, funded sequencer (set TEST_SEQUENCER_ADDR, or pre-configure
#     the wallet). Account creation is local, but deploys/mints need funds.
#   - `cargo` (to run the amm_pdas example)
#   - the guest .bin files already built (`make build-programs`)
#   - the IDL files under artifacts/ (`make idl`)
#
# Usage (from anywhere in the repo):
#   apps/amm/tests/testnet/setup-amm-testnet.sh
#   TEST_SEQUENCER_ADDR=http://127.0.0.1:8080 apps/amm/tests/testnet/setup-amm-testnet.sh
#   FORCE_BOOTSTRAP=1 ...        # re-restore the test wallet (rewrites its storage)
#
set -euo pipefail

# Resolve the repo root robustly regardless of where the script lives / is called
# from. Prefer git; fall back to walking up from this script's directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${REPO_ROOT:-$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || (cd "$SCRIPT_DIR/../../../.." && pwd))}"
cd "$REPO_ROOT"

###############################################################################
# TEST WALLET — isolated + deterministic
###############################################################################

# Dedicated wallet home for tests. Kept out of git (see .gitignore). Exported
# under both env var names so whichever the installed `wallet`/`spel` expects is
# satisfied (the current wallet uses LEE_WALLET_HOME_DIR).
TEST_WALLET_HOME="${TEST_WALLET_HOME:-$SCRIPT_DIR/.wallet}"
export LEE_WALLET_HOME_DIR="$TEST_WALLET_HOME"
export NSSA_WALLET_HOME_DIR="$TEST_WALLET_HOME"

# The deterministic test seed. A wallet restored from this mnemonic yields the
# same account ids every time, which is why they can be shared/pinned.
TEST_MNEMONIC="${TEST_MNEMONIC:-test test test test test test test test test test test junk}"
TEST_WALLET_PASSWORD="${TEST_WALLET_PASSWORD:-test}"
TEST_WALLET_DEPTH="${TEST_WALLET_DEPTH:-3}"
# Optional: point the test wallet's config at your sequencer. Leave empty to use
# whatever the wallet home is already configured with.
TEST_SEQUENCER_ADDR="${TEST_SEQUENCER_ADDR:-}"

# Deterministic accounts, created in THIS fixed order after a fresh restore so
# their ids are reproducible. Resolved to ids at runtime via `wallet account id`.
ACCOUNT_LABELS=(token-a-def token-a-holding token-b-def token-b-holding lp-holding)

###############################################################################
# CONFIG — non-account parameters (edit freely)
###############################################################################

# --- Program binaries (docker release builds; image ids must match deployment) ---
TOKEN_BIN="programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
AMM_BIN="programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin"
TWAP_BIN="programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin"

# --- IDLs ---
TOKEN_IDL="artifacts/token-idl.json"
AMM_IDL="artifacts/amm-idl.json"

# --- Token metadata ---
TOKEN_A_NAME="TOKEN A"; TOKEN_A_SYMBOL="TKA"; TOKEN_A_SUPPLY="1000000000000000000000"; TOKEN_A_DECIMALS=18
TOKEN_B_NAME="TOKEN B"; TOKEN_B_SYMBOL="TKB"; TOKEN_B_SUPPLY="1000000000000000000000"; TOKEN_B_DECIMALS=18

# --- Pool inputs ---
CLOCK_ACCOUNT="4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU"  # canonical LEZ system clock
POOL_TOKEN_A_AMOUNT="10000"
POOL_TOKEN_B_AMOUNT="10000"
POOL_FEES="1"
POOL_DEADLINE="18446744073709551615"

# Where the UI token config is written for TESTS ONLY (git-ignored). This is
# deliberately NOT apps/amm/amm-tokens.json — that file is your personal local
# config with your own accounts. Tests stay fully isolated: pass this path as
# TOKENS_CONFIG when launching the UI for a test run.
TOKENS_CONFIG_OUT="apps/amm/tests/testnet/amm-tokens.json"

###############################################################################
# Helpers
###############################################################################

BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; CYN=$'\033[36m'; RST=$'\033[0m'

hr()  { printf '%s\n' "${DIM}────────────────────────────────────────────────────────────────────────${RST}"; }
log() { printf '%s\n' "$*"; }
sec() { hr; printf '%s\n' "${BOLD}${CYN}==> $*${RST}"; hr; }
kv()  { printf '  %-22s %s\n' "$1" "$2"; }
die() { printf '%s\n' "${RED}✗ $*${RST}" >&2; exit 1; }

require_cmd() { command -v "$1" >/dev/null 2>&1 || die "required command not found on PATH: $1"; }
require_file(){ [ -f "$1" ] || die "required file not found: $1 (cwd=$(pwd))"; }

# Run a transaction command, streaming its output live, then assert the
# sequencer confirmation marker is present.
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

# Read-only account inspection (no confirmation check).
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

# Resolve a wallet account id (bare base58) from its label. Deterministic under
# the test mnemonic. Returns non-zero if the label isn't registered yet.
acct_id() {
  local label="$1" out
  out="$(wallet account id --account-id "$label" 2>/dev/null)" || return 1
  printf '%s' "$out" | grep -oE '[1-9A-HJ-NP-Za-km-z]{32,44}' | head -n1
}

# Restore the isolated test wallet from the fixed mnemonic and register the
# deterministic accounts. `restore-keys` REWRITES storage (safe here — it's a
# throwaway test home). Idempotent: skips accounts that already resolve.
bootstrap_test_wallet() {
  sec "Bootstrap deterministic test wallet"
  kv "wallet home" "$TEST_WALLET_HOME"
  mkdir -p "$TEST_WALLET_HOME"

  if [ -n "$TEST_SEQUENCER_ADDR" ]; then
    log "${DIM}\$ wallet config set sequencer_addr $TEST_SEQUENCER_ADDR${RST}"
    # On a fresh home the first wallet command triggers one-time setup, which
    # reads a password from STDIN ("Input password:") and generates a THROWAWAY
    # random-seed wallet. Feed the password so it never blocks; restore-keys
    # below immediately rewrites storage from the test mnemonic and sets the real
    # password to $TEST_WALLET_PASSWORD.
    printf '%s\n' "$TEST_WALLET_PASSWORD" \
      | wallet config set sequencer_addr "$TEST_SEQUENCER_ADDR" \
      || log "${YEL}⚠ 'wallet config set sequencer_addr' failed — configure the wallet manually.${RST}"
  fi

  # restore-keys reads the mnemonic then the password from stdin (non-interactive)
  # and REWRITES storage — so the wallet's password becomes $TEST_WALLET_PASSWORD.
  log "${DIM}\$ printf '<mnemonic>\\n<password>\\n' | wallet restore-keys --depth $TEST_WALLET_DEPTH${RST}"
  printf '%s\n%s\n' "$TEST_MNEMONIC" "$TEST_WALLET_PASSWORD" \
    | wallet restore-keys --depth "$TEST_WALLET_DEPTH" \
    || die "wallet restore-keys failed"

  local label
  for label in "${ACCOUNT_LABELS[@]}"; do
    if acct_id "$label" >/dev/null 2>&1; then
      kv "exists" "$label"
    else
      log "${DIM}\$ wallet account new public --label $label${RST}"
      wallet account new public --label "$label" || die "failed to create account: $label"
    fi
  done
}

###############################################################################
# 0. Preflight + wallet bootstrap
###############################################################################
sec "Preflight"
require_cmd wallet
require_cmd spel
require_cmd cargo
require_file "$TOKEN_BIN"; require_file "$AMM_BIN"; require_file "$TWAP_BIN"
require_file "$TOKEN_IDL"; require_file "$AMM_IDL"
kv "repo root"        "$REPO_ROOT"
kv "token bin" "$TOKEN_BIN"; kv "amm bin" "$AMM_BIN"; kv "twap bin" "$TWAP_BIN"

if [ ! -d "$TEST_WALLET_HOME" ] || [ "${FORCE_BOOTSTRAP:-0}" = "1" ]; then
  bootstrap_test_wallet
else
  kv "test wallet" "reusing $TEST_WALLET_HOME (FORCE_BOOTSTRAP=1 to re-restore)"
fi

###############################################################################
# 1. Resolve the deterministic test accounts
###############################################################################
sec "Resolve deterministic test accounts (from the test mnemonic)"
TOKEN_A_DEF="$(acct_id token-a-def)"        || die "token-a-def not registered — run with FORCE_BOOTSTRAP=1"
TOKEN_A_HOLDING="$(acct_id token-a-holding)" || die "token-a-holding not registered"
TOKEN_B_DEF="$(acct_id token-b-def)"        || die "token-b-def not registered"
TOKEN_B_HOLDING="$(acct_id token-b-holding)" || die "token-b-holding not registered"
USER_HOLDING_LP="$(acct_id lp-holding)"     || die "lp-holding not registered"
for v in TOKEN_A_DEF TOKEN_A_HOLDING TOKEN_B_DEF TOKEN_B_HOLDING USER_HOLDING_LP; do
  [ -n "${!v}" ] || die "failed to resolve account id for $v"
done

# Derived roles (the input holding signs; mint authority == holding; authority is the A holding).
TOKEN_A_MINT_AUTH="$TOKEN_A_HOLDING"; TOKEN_B_MINT_AUTH="$TOKEN_B_HOLDING"
AMM_AUTHORITY="$TOKEN_A_HOLDING"
USER_HOLDING_A="$TOKEN_A_HOLDING"; USER_HOLDING_B="$TOKEN_B_HOLDING"

kv "token-a-def"     "$TOKEN_A_DEF"
kv "token-a-holding" "$TOKEN_A_HOLDING"
kv "token-b-def"     "$TOKEN_B_DEF"
kv "token-b-holding" "$TOKEN_B_HOLDING"
kv "lp-holding"      "$USER_HOLDING_LP"

###############################################################################
# 2. Deploy programs
###############################################################################
run_tx soft "deploy token program"       -- wallet deploy-program "$TOKEN_BIN"
run_tx soft "deploy amm program"         -- wallet deploy-program "$AMM_BIN"
run_tx soft "deploy twap_oracle program" -- wallet deploy-program "$TWAP_BIN"

###############################################################################
# 3. Derive program IDs
###############################################################################
sec "Program IDs (derived from the deployed binaries)"
TOKEN_PID="$(program_id "$TOKEN_BIN")"; kv "token program id" "$TOKEN_PID"
AMM_PID="$(program_id "$AMM_BIN")";     kv "amm program id"   "$AMM_PID"
TWAP_PID="$(program_id "$TWAP_BIN")";   kv "twap program id"  "$TWAP_PID"

###############################################################################
# 4. Create token definitions (mint supply to the holding accounts)
###############################################################################
run_tx strict "create fungible definition: $TOKEN_A_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_BIN" -- new-fungible-definition \
    --name "$TOKEN_A_NAME" --total-supply "$TOKEN_A_SUPPLY" \
    --definition-target-account "$TOKEN_A_DEF" \
    --holding-target-account "$TOKEN_A_HOLDING" \
    --mint-authority "$TOKEN_A_MINT_AUTH"

run_tx strict "create fungible definition: $TOKEN_B_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_BIN" -- new-fungible-definition \
    --name "$TOKEN_B_NAME" --total-supply "$TOKEN_B_SUPPLY" \
    --definition-target-account "$TOKEN_B_DEF" \
    --holding-target-account "$TOKEN_B_HOLDING" \
    --mint-authority "$TOKEN_B_MINT_AUTH"

###############################################################################
# 5. Verify token definitions & holdings
###############################################################################
inspect "$TOKEN_IDL" "$TOKEN_A_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_A_HOLDING" "TokenHolding"
inspect "$TOKEN_IDL" "$TOKEN_B_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_B_HOLDING" "TokenHolding"

###############################################################################
# 6. Derive AMM PDAs from the program ids + token pair
###############################################################################
sec "Deriving AMM PDAs (amm_pdas example)"
log "${DIM}\$ cargo run -q -p amm_program --example amm_pdas -- $AMM_PID $TWAP_PID $TOKEN_A_DEF $TOKEN_B_DEF${RST}"
PDAS="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p amm_program --example amm_pdas -- \
          "$AMM_PID" "$TWAP_PID" "$TOKEN_A_DEF" "$TOKEN_B_DEF")"
printf '%s\n' "$PDAS"

pda() { printf '%s' "$PDAS" | awk -v k="$1" '$1==k {print $2; exit}'; }
CONFIG="$(pda config)"
POOL="$(pda pool)"
VAULT_A="$(pda vault_a)"
VAULT_B="$(pda vault_b)"
POOL_LP="$(pda pool_definition_lp)"
LP_LOCK="$(pda lp_lock_holding)"
TICK="$(pda current_tick_account)"

for name in CONFIG POOL VAULT_A VAULT_B POOL_LP LP_LOCK TICK; do
  [ -n "${!name}" ] || die "failed to parse PDA '$name' from amm_pdas output"
done

sec "Resolved accounts"
kv "config"               "$CONFIG"
kv "pool"                 "$POOL"
kv "vault_a"              "$VAULT_A"
kv "vault_b"              "$VAULT_B"
kv "pool_definition_lp"   "$POOL_LP"
kv "lp_lock_holding"      "$LP_LOCK"
kv "current_tick_account" "$TICK"

###############################################################################
# 7. Initialize the AMM
###############################################################################
run_tx strict "initialize AMM config" -- \
  spel --idl "$AMM_IDL" --program "$AMM_BIN" -- initialize \
    --config "$CONFIG" \
    --token-program-id "$TOKEN_PID" \
    --twap-oracle-program-id "$TWAP_PID" \
    --authority "$AMM_AUTHORITY"

###############################################################################
# 8. Create the pool (seed initial liquidity)
###############################################################################
run_tx strict "create pool + seed liquidity" -- \
  spel --idl "$AMM_IDL" --program "$AMM_BIN" -- new-definition \
    --config "$CONFIG" \
    --pool "$POOL" \
    --vault-a "$VAULT_A" \
    --vault-b "$VAULT_B" \
    --pool-definition-lp "$POOL_LP" \
    --lp-lock-holding "$LP_LOCK" \
    --user-holding-a "$USER_HOLDING_A" \
    --user-holding-b "$USER_HOLDING_B" \
    --user-holding-lp "$USER_HOLDING_LP" \
    --current-tick-account "$TICK" \
    --clock "$CLOCK_ACCOUNT" \
    --token-a-amount "$POOL_TOKEN_A_AMOUNT" \
    --token-b-amount "$POOL_TOKEN_B_AMOUNT" \
    --fees "$POOL_FEES" \
    --deadline "$POOL_DEADLINE"

###############################################################################
# 9. Verify the pool
###############################################################################
inspect "$AMM_IDL" "$POOL" "PoolDefinition"

###############################################################################
# 10. Write the UI token config from the deterministic accounts
###############################################################################
sec "Write UI token config -> $TOKENS_CONFIG_OUT"
cat > "$TOKENS_CONFIG_OUT" <<JSON
[
  {
    "symbol": "$TOKEN_A_SYMBOL",
    "name": "$TOKEN_A_NAME",
    "definitionId": "$TOKEN_A_DEF",
    "holding": "$TOKEN_A_HOLDING",
    "decimals": $TOKEN_A_DECIMALS
  },
  {
    "symbol": "$TOKEN_B_SYMBOL",
    "name": "$TOKEN_B_NAME",
    "definitionId": "$TOKEN_B_DEF",
    "holding": "$TOKEN_B_HOLDING",
    "decimals": $TOKEN_B_DECIMALS
  }
]
JSON
kv "wrote" "$TOKENS_CONFIG_OUT"

sec "Done"
log "${GRN}✅ Setup complete.${RST}"
kv "AMM program id"  "$AMM_PID"
kv "TWAP program id" "$TWAP_PID"
kv "pool"            "$POOL"
log ""
log "Launch the UI against the ISOLATED test wallet + test token config:"
log "  ${DIM}LEE_WALLET_HOME_DIR=$TEST_WALLET_HOME \\${RST}"
log "  ${DIM}  AMM_PROGRAM_BIN=$REPO_ROOT/$AMM_BIN \\${RST}"
log "  ${DIM}  TOKENS_CONFIG=$REPO_ROOT/$TOKENS_CONFIG_OUT \\${RST}"
log "  ${DIM}  nix run .#amm-ui${RST}"
log "Then in another terminal: ${DIM}node apps/amm/tests/swap.mjs${RST}"
