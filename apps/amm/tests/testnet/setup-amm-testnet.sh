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
# Program IDs and all AMM PDAs (config, pool, vaults, LP, tick) are DERIVED at
# runtime from the deployed binaries + the token definition accounts, so this
# script stays correct even if you rebuild the guest binaries (new image id =>
# new program id => new PDAs). Only the input accounts below are hand-picked.
#
# TEST WALLET (isolated): by default this script points the wallet at a
# dedicated, git-ignored home under this folder (apps/amm/tests/testnet/.wallet)
# so it never touches your personal ~/.lee/wallet. Override with TEST_WALLET_HOME.
#
# Prerequisites (all managed by you, outside this script):
#   - `wallet` and `spel` on PATH (from the SPEL toolchain)
#   - a wallet initialized at TEST_WALLET_HOME, funded and synced, that OWNS the
#     account ids configured below (see "TEST ACCOUNTS" — these must be derived
#     from / created in the test wallet; see docs/testnet-runbook.md §2)
#   - `cargo` (to run the amm_pdas example)
#   - the guest .bin files already built (`make build-programs`)
#   - the IDL files under artifacts/ (`make idl`)
#
# Usage (from anywhere in the repo):
#   apps/amm/tests/testnet/setup-amm-testnet.sh
#   TEST_WALLET_HOME=/tmp/lez-test-wallet apps/amm/tests/testnet/setup-amm-testnet.sh
#
set -euo pipefail

# Resolve the repo root robustly regardless of where the script lives / is called
# from. Prefer git; fall back to walking up from this script's directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${REPO_ROOT:-$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || cd "$SCRIPT_DIR/../../../.." && pwd)}"
cd "$REPO_ROOT"

###############################################################################
# TEST WALLET — isolated from your personal wallet
###############################################################################

# Dedicated wallet home for tests. Kept out of git (see .gitignore). Exported
# under both env var names so whichever the installed `wallet`/`spel` expects
# is satisfied.
TEST_WALLET_HOME="${TEST_WALLET_HOME:-$SCRIPT_DIR/.wallet}"
export LEE_WALLET_HOME_DIR="$TEST_WALLET_HOME"
export NSSA_WALLET_HOME_DIR="$TEST_WALLET_HOME"

# The deterministic test mnemonic. A wallet restored from this seed always
# yields the same account ids, which is why the ids below can be pinned and
# shared with the team. Keep in sync with the wallet at TEST_WALLET_HOME.
TEST_MNEMONIC="${TEST_MNEMONIC:-test test test test test test test test test test test junk}"

###############################################################################
# CONFIG — edit these
###############################################################################

# --- Program binaries (docker release builds; image ids must match deployment) ---
TOKEN_BIN="programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
AMM_BIN="programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin"
TWAP_BIN="programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin"

# --- IDLs ---
TOKEN_IDL="artifacts/token-idl.json"
AMM_IDL="artifacts/amm-idl.json"

# --- TEST ACCOUNTS (must be owned by the wallet at TEST_WALLET_HOME) ----------
# These are the deterministic accounts the test suite relies on. See the
# "Deterministic test accounts" note at the top of the file / the PR discussion:
# once the test wallet is restored from TEST_MNEMONIC, capture the ids it derives
# and pin them here so amm-tokens.json and swap.mjs can reference known accounts.

# --- Token A ---
TOKEN_A_NAME="TOKEN A"
TOKEN_A_SUPPLY="1000000000000000000000"
TOKEN_A_DEF="6gQMF38wEDPEnUamLmNqN8opss5xvr9w55eddddMmpEK"      # --definition-target-account
TOKEN_A_HOLDING="EF6c3VtgzUzpxfEKpvR3ZxsCch8y5YqazmWQu6Bybvyq"  # --holding-target-account
TOKEN_A_MINT_AUTH="EF6c3VtgzUzpxfEKpvR3ZxsCch8y5YqazmWQu6Bybvyq"

# --- Token B ---
TOKEN_B_NAME="TOKEN B"
TOKEN_B_SUPPLY="1000000000000000000000"
TOKEN_B_DEF="Hw65ZimauFtHtvmDvWb24aWi1E6uRCifdyEn46Ei8wAc"
TOKEN_B_HOLDING="L3hr1xABT3gkSYR1Fztu2smyJ1ftXcivjM8LguMjAsp"
TOKEN_B_MINT_AUTH="L3hr1xABT3gkSYR1Fztu2smyJ1ftXcivjM8LguMjAsp"

# --- AMM / pool inputs ---
AMM_AUTHORITY="EF6c3VtgzUzpxfEKpvR3ZxsCch8y5YqazmWQu6Bybvyq"
USER_HOLDING_A="$TOKEN_A_HOLDING"
USER_HOLDING_B="$TOKEN_B_HOLDING"
USER_HOLDING_LP="7taJs6YpzBcWc5jiZcnZ817nU1G5WnQtBHskbcMDUc6A"
CLOCK_ACCOUNT="4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU"

POOL_TOKEN_A_AMOUNT="10000"
POOL_TOKEN_B_AMOUNT="10000"
POOL_FEES="1"
POOL_DEADLINE="18446744073709551615"

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

  # The confirmation line printed by spel is:
  #   ✅ Transaction confirmed — included in a block.
  # Match on the two stable substrings to be robust to emoji/dash encoding.
  if grep -q "Transaction confirmed" "$tmp" && grep -q "included in a block" "$tmp"; then
    log "${GRN}✅ CONFIRMED — included in a block: ${desc}${RST}"
    rm -f "$tmp"
    return 0
  fi

  rm -f "$tmp"
  if [ "$mode" = "soft" ]; then
    log "${YEL}⚠ no '✅ Transaction confirmed — included in a block.' marker for: ${desc}"
    log "  (continuing — 'wallet deploy-program' may not print the spel marker; verify manually)${RST}"
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

###############################################################################
# 0. Preflight
###############################################################################
sec "Preflight"
require_cmd wallet
require_cmd spel
require_cmd cargo
require_file "$TOKEN_BIN"; require_file "$AMM_BIN"; require_file "$TWAP_BIN"
require_file "$TOKEN_IDL"; require_file "$AMM_IDL"
kv "repo root"        "$REPO_ROOT"
kv "test wallet home" "$TEST_WALLET_HOME"
if [ ! -d "$TEST_WALLET_HOME" ]; then
  die "test wallet home not found: $TEST_WALLET_HOME
    Initialize a wallet there from the test mnemonic, then create/pin the TEST
    ACCOUNTS above. See docs/testnet-runbook.md §2. Mnemonic:
    \"$TEST_MNEMONIC\""
fi
kv "token bin" "$TOKEN_BIN"
kv "amm bin"   "$AMM_BIN"
kv "twap bin"  "$TWAP_BIN"

###############################################################################
# 1. Deploy programs
###############################################################################
run_tx soft "deploy token program"       -- wallet deploy-program "$TOKEN_BIN"
run_tx soft "deploy amm program"         -- wallet deploy-program "$AMM_BIN"
run_tx soft "deploy twap_oracle program" -- wallet deploy-program "$TWAP_BIN"

###############################################################################
# 2. Derive program IDs
###############################################################################
sec "Program IDs (derived from the deployed binaries)"
TOKEN_PID="$(program_id "$TOKEN_BIN")"; kv "token program id" "$TOKEN_PID"
AMM_PID="$(program_id "$AMM_BIN")";     kv "amm program id"   "$AMM_PID"
TWAP_PID="$(program_id "$TWAP_BIN")";   kv "twap program id"  "$TWAP_PID"

###############################################################################
# 3. Create token definitions (mint supply to the holding accounts)
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
# 4. Verify token definitions & holdings
###############################################################################
inspect "$TOKEN_IDL" "$TOKEN_A_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_A_HOLDING" "TokenHolding"
inspect "$TOKEN_IDL" "$TOKEN_B_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_B_HOLDING" "TokenHolding"

###############################################################################
# 5. Derive AMM PDAs from the program ids + token pair
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
kv "user_holding_a"       "$USER_HOLDING_A"
kv "user_holding_b"       "$USER_HOLDING_B"
kv "user_holding_lp"      "$USER_HOLDING_LP"
kv "clock"                "$CLOCK_ACCOUNT"

###############################################################################
# 6. Initialize the AMM
###############################################################################
run_tx strict "initialize AMM config" -- \
  spel --idl "$AMM_IDL" --program "$AMM_BIN" -- initialize \
    --config "$CONFIG" \
    --token-program-id "$TOKEN_PID" \
    --twap-oracle-program-id "$TWAP_PID" \
    --authority "$AMM_AUTHORITY"

###############################################################################
# 7. Create the pool (seed initial liquidity)
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
# 8. Verify the pool
###############################################################################
inspect "$AMM_IDL" "$POOL" "PoolDefinition"

sec "Done"
log "${GRN}✅ Setup complete.${RST}"
kv "AMM program id"  "$AMM_PID"
kv "TWAP program id" "$TWAP_PID"
kv "pool"            "$POOL"
log ""
log "For the UI / swap test, set:"
log "  ${DIM}AMM_PROGRAM_BIN=$REPO_ROOT/$AMM_BIN${RST}"
log "  ${DIM}TOKENS_CONFIG   (apps/amm/amm-tokens.json with def ids $TOKEN_A_DEF / $TOKEN_B_DEF)${RST}"
