#!/usr/bin/env bash
#
# setup-amm-testnet.sh
# --------------------
# Deploy the token/amm/twap/token-mint-authority programs, mint four fungible
# tokens, initialize the AMM, and create the A/B pool — from scratch — against
# whatever sequencer your `wallet` / `spel` config points at. This is the
# prerequisite state the AMM UI tests exercise: swap.mjs swaps against the seeded
# A/B pool, create-pool.mjs creates the (deliberately unseeded) A/C pool, and
# custom-token.mjs adds token D by id. Run it once, then launch the UI / run the
# tests.
#
# FAUCET MINT AUTHORITY: every test token's `mint_authority` is set — at
# NewFungibleDefinition time — to the token-mint-authority (faucet) program's
# singleton mint-authority PDA, derived from the deployed faucet binary's ImageID.
# The initial supply is still minted to the holding accounts at creation (that's
# independent of mint_authority), so the pool still seeds normally; but AFTER that
# the only way to mint more of these tokens is through the faucet. That's what
# lets a brand-new account fund itself before swapping — the follow-up faucet e2e
# test relies on this wiring.
#
# Token D is created ON-CHAIN but deliberately LEFT OUT of the written token config
# (amm-tokens.json) — it is the "custom" token the custom-token.mjs test pastes by
# id to confirm the liquidity view resolves and adds an unlisted token. Its id is
# written to custom-token.json for that test to read.
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
# script stays correct even if you rebuild the guest binaries: since LEZ v0.2.5 a
# program is addressed by the account id of its deployed `ProgramHeader`, which this
# script creates from a fixed label, so the address survives a rebuild. What a rebuild
# CAN change is the segment count (bigger binary => more 96 KiB chunks), and segments
# are write-once, so a re-deploy needs a fresh set of segment accounts.
#
# Prerequisites (managed by you, outside this script):
#   - `wallet` and `spel` on PATH (from the SPEL toolchain)
#   - a reachable, funded sequencer (set TEST_SEQUENCER_ADDR, or pre-configure
#     the wallet). Account creation is local, but deploys/mints need funds.
#     Deploys additionally need a funded PAYER, because v0.2.5 writes them into
#     freshly-claimed header/segment accounts that cannot self-pay. That is this
#     wallet's own root account (see DEPLOY_PAYER below); the chain's genesis must
#     give it a supply.
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

# Dedicated wallet home for tests. Kept out of git (see .gitignore). The v0.2.x
# wallet/spel read LEE_WALLET_HOME_DIR (the old NSSA_WALLET_HOME_DIR is unused).
TEST_WALLET_HOME="${TEST_WALLET_HOME:-$SCRIPT_DIR/.wallet}"
export LEE_WALLET_HOME_DIR="$TEST_WALLET_HOME"

# Silence the wallet's `Transaction data is {tx:?}` dump, which Debug-prints the whole
# transaction -- for a deploy that is the entire 96 KiB segment as a byte array, once per
# segment. The hash and the "included in block" line are printed outside this guard, so
# nothing needed for diagnosis is lost. Set SHOW_TX_DATA=1 to get the dump back.
[ "${SHOW_TX_DATA:-0}" = "1" ] || export SUPPRESS_VERBOSE_PRINTS=1

# The deterministic test seed. A wallet restored from this mnemonic yields the
# same account ids every time, which is why they can be shared/pinned.
TEST_MNEMONIC="${TEST_MNEMONIC:-test test test test test test test test test test test junk}"
TEST_WALLET_PASSWORD="${TEST_WALLET_PASSWORD:-test}"
TEST_WALLET_DEPTH="${TEST_WALLET_DEPTH:-3}"
# Optional: point the test wallet's config at your sequencer. Leave empty to use
# whatever the wallet home is already configured with.
TEST_SEQUENCER_ADDR="${TEST_SEQUENCER_ADDR:-}"

# Sequencer confirmation poll. The wallet poller (lee `lez/wallet/src/poller.rs`)
# calls get_transaction up to `seq_tx_poll_max_blocks` times, sleeping
# `seq_poll_timeout` BETWEEN polls — despite its name that field is the inter-poll
# DELAY, not a total timeout, so total wait ≈ blocks × delay. The wallet defaults
# (5 × 12s ≈ 48s) can abort a tx that confirms a little later. Widen the window by
# raising the NUMBER of polls and keeping the delay short so detection stays
# responsive (40 × 3s ≈ 2min). Do NOT inflate the delay — a large delay just polls
# rarely and appears to hang. Override via env.
TEST_SEQ_TX_POLL_MAX_BLOCKS="${TEST_SEQ_TX_POLL_MAX_BLOCKS:-40}"
TEST_SEQ_POLL_TIMEOUT="${TEST_SEQ_POLL_TIMEOUT:-3s}"

# Deterministic accounts, created in THIS fixed order after a fresh restore so
# their ids are reproducible. Resolved to ids at runtime via `wallet account id`.
# token-c-*/token-d-*/holder2-* are APPENDED (not inserted) so the pre-existing a/b/lp ids don't shift.
# Token C has no seeded pool — the create-pool UI test (apps/amm/tests/create-pool.mjs)
# creates the A/C pool itself, minting its own LP holding via the app.
# Token D is created but LEFT OUT of the token config — the custom-token UI test
# (apps/amm/tests/custom-token.mjs) adds it by id.
# holder2 / holder2-a-holding are the "Token A Holder 2" pair for the faucet-swap UI
# test (apps/amm/tests/faucet-swap.mjs): holder2 is the faucet recipient/signer and
# rate-limit subject; holder2-a-holding is its (initially empty) token A holding that
# the faucet mints into. They deliberately start with NO token A so the test can prove
# the account only appears in the swap picker after a faucet mint + UI refresh. The
# faucet requires recipient != user_holding, hence two accounts.
# `amm-owner` is a dedicated account that signs `initialize` — the AMM instance's
# namespace owner. All three are appended last so the existing accounts keep their ids.
ACCOUNT_LABELS=(token-a-def token-a-holding token-b-def token-b-holding lp-holding token-c-def token-c-holding token-d-def token-d-holding holder2 holder2-a-holding amm-owner)

# --- Deploy accounts (LEZ v0.2.5) ---
# `wallet deploy-program` is gone: a program is now a chain of write-once segment
# accounts plus a header account, and every one of them is an explicit argument
# (`wallet program-loader` never generates keys). So each program needs one header
# account and one account per 96 KiB chunk of its binary.
#
# DEPLOY_SEGMENT_SLOTS is a fixed reservation, not the actual count. The count comes
# from the binary at deploy time; reserving a fixed number of labels keeps every
# account id in this script deterministic even when a rebuild changes a binary's size.
# Only the first N slots of each program are passed to the deploy. The protocol cap is
# MAX_PROGRAM_SEGMENTS = 20; today's binaries need 3-6.
DEPLOY_PROGRAMS=(token amm twap mint-authority)
DEPLOY_SEGMENT_SLOTS=10

for _prog in "${DEPLOY_PROGRAMS[@]}"; do
  ACCOUNT_LABELS+=("${_prog}-header")
  for _i in $(seq 1 "$DEPLOY_SEGMENT_SLOTS"); do
    ACCOUNT_LABELS+=("${_prog}-seg-${_i}")
  done
done
unset _prog _i

###############################################################################
# CONFIG — non-account parameters (edit freely)
###############################################################################

# --- Program binaries (docker release builds; image ids must match deployment) ---
TOKEN_BIN="programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
AMM_BIN="programs/amm/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/amm.bin"
TWAP_BIN="programs/twap_oracle/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/twap_oracle.bin"
# The faucet (token-mint-authority) binary. Its ImageID determines the mint-authority
# PDA every test token is minted against, so it MUST be the exact bin deployed below.
MINT_AUTHORITY_BIN="programs/token_mint_authority/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token_mint_authority.bin"

# --- IDLs ---
TOKEN_IDL="artifacts/token-idl.json"
AMM_IDL="artifacts/amm-idl.json"
# The faucet IDL — not used by this setup, but the follow-up faucet e2e test reads it.
MINT_AUTHORITY_IDL="artifacts/token_mint_authority-idl.json"

# --- Token metadata ---
TOKEN_A_NAME="TOKEN A"; TOKEN_A_SYMBOL="TKA"; TOKEN_A_SUPPLY="1000000000000000000000"; TOKEN_A_DECIMALS=18
TOKEN_B_NAME="TOKEN B"; TOKEN_B_SYMBOL="TKB"; TOKEN_B_SUPPLY="1000000000000000000000"; TOKEN_B_DECIMALS=18
TOKEN_C_NAME="TOKEN C"; TOKEN_C_SYMBOL="TKC"; TOKEN_C_SUPPLY="1000000000000000000000"; TOKEN_C_DECIMALS=18
# Token D is the "custom" token: created on-chain but NOT written to the token config.
TOKEN_D_NAME="TOKEN D"; TOKEN_D_SYMBOL="TKD"; TOKEN_D_SUPPLY="1000000000000000000000"; TOKEN_D_DECIMALS=18

# --- Pool inputs ---
CLOCK_ACCOUNT="4BdcjoXkq786TMWcBGGHqcxeLYMZmn17rL4eM9ZyRWNU"  # canonical LEZ system clock
POOL_TOKEN_A_AMOUNT="10000"
POOL_TOKEN_B_AMOUNT="10000"
# Instance-wide swap fee (basis points), set once at `initialize` and stored in the AMM
# config — no longer a per-pool value. Every swap in this namespace uses it.
SWAP_FEE_BPS="1"
# Protocol fee as a fraction of the swap fee (basis points), also set once at `initialize`.
# On each swap, this % of the swap fee is diverted to the instance's protocol-fee holding;
# the rest stays with LPs. 0 = no protocol fee.
PROTOCOL_FEE_BPS="0"
POOL_DEADLINE="18446744073709551615"

# Where the UI token config is written for TESTS ONLY (git-ignored). This is
# deliberately NOT apps/amm/amm-tokens.json — that file is your personal local
# config with your own accounts. Tests stay fully isolated: pass this path as
# TOKENS_CONFIG when launching the UI for a test run.
TOKENS_CONFIG_OUT="apps/amm/tests/testnet/amm-tokens.json"

# Where the UI known-pools config is written (git-ignored, tests only). Pass this
# path as AMM_POOLS_CONFIG when launching the UI; the Pools page renders one row
# per entry. More seeded pools = more entries here, no app change.
POOLS_CONFIG_OUT="apps/amm/tests/testnet/amm-pools.json"

# Single-file multi-network registry (git-ignored, tests only) — the same tokens
# and pools in the remote-registry shape, so the
# AMM_REGISTRY_URL path can be exercised against this local sequencer without
# hosting anything (point AMM_REGISTRY_URL at this file via a file:// URL).
REGISTRY_CONFIG_OUT="apps/amm/tests/testnet/amm-registry.json"

# Isolated custom-token store for TESTS ONLY (git-ignored). Pass this path as
# CUSTOM_TOKEN_CONFIG when launching the UI so custom-token.mjs controls it instead of
# the app's default per-user store. Initialized empty so a test run starts clean.
CUSTOM_TOKEN_CONFIG_OUT="apps/amm/tests/testnet/custom-tokens.json"

# Faucet manifest for the faucet-swap UI test (git-ignored, tests only). Everything
# apps/amm/tests/faucet-swap.mjs needs to (a) submit a FaucetMint via spel and (b)
# drive the swap from the freshly-funded holder2 account: bin/IDL paths, the six
# FaucetMint account ids (recipient/allowance/holding/definition/authority/clock),
# the token A definition id, and the fee payer every spel submission declares
# (since v0.2.5 a public transaction without a fee declaration is rejected).
# Written at the end from the derived values.
FAUCET_MANIFEST_OUT="apps/amm/tests/testnet/faucet.json"

###############################################################################
# Helpers
###############################################################################

BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; CYN=$'\033[36m'; RST=$'\033[0m'

# Diagnostics go to STDERR so a function can return a VALUE on stdout without its
# own progress output being captured with it -- `deploy_program` prints the header
# id, and its caller discards that with `>/dev/null`. When these wrote to stdout
# that redirect swallowed the whole transaction log too, hiding real failures.
hr()  { printf '%s\n' "${DIM}────────────────────────────────────────────────────────────────────────${RST}" >&2; }
log() { printf '%s\n' "$*" >&2; }
sec() { hr; printf '%s\n' "${BOLD}${CYN}==> $*${RST}" >&2; hr; }
kv()  { printf '  %-22s %s\n' "$1" "$2" >&2; }
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
  "$@" 2>&1 | tee "$tmp" >&2
  local rc=${PIPESTATUS[0]}
  set -e

  if [ "$rc" -ne 0 ]; then
    rm -f "$tmp"
    die "command exited with status $rc — $desc"
  fi

  # Two tools, two phrasings for the same fact, so accept either:
  #   spel:   "✅ Transaction confirmed — included in a block."
  #   wallet: "Transaction is included in block 27"
  # Recognising only spel's is why the wallet-driven steps (deploy, funding) used to
  # run `soft` and warn on every success -- which also meant a REAL failure there was
  # indistinguishable from the usual noise.
  if { grep -q "Transaction confirmed" "$tmp" && grep -q "included in a block" "$tmp"; } \
     || grep -q "Transaction is included in block" "$tmp"; then
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

# How many 96 KiB segments a binary splits into. Mirrors the wallet's own chunking
# (`bytecode.chunks(MAX_SEGMENT_DATA_LEN)`), so the --segments list length matches
# exactly; a mismatch is rejected with SegmentCountMismatch.
MAX_SEGMENT_DATA_LEN=98304
segment_count() {
  local bin="$1" size
  size="$(wc -c < "$bin" | tr -d ' ')"
  [ "$size" -gt 0 ] || die "program binary is empty: $bin"
  printf '%s' $(( (size + MAX_SEGMENT_DATA_LEN - 1) / MAX_SEGMENT_DATA_LEN ))
}

# Deploy one program and echo its account id (the header account's).
#
# Since LEZ v0.2.5 deployment is ordinary program execution against the `program_loader`
# pseudo-program: upload one write-once segment account per chunk, then create a header
# pointing at the chain. The header's account id IS the program's address from then on --
# there is nothing to derive from the binary any more.
#
# Segments are write-once and the flow is not resumable: if this fails partway, the
# segments it did land stay claimed and re-running with the same labels fails.
# FORCE_BOOTSTRAP=1 does NOT recover from that -- the wallet restores from a FIXED
# mnemonic, so a re-restore derives the very same ids and lands on the very same claimed
# accounts. Recovery is to reset the CHAIN (for a local sequencer: stop it, delete its
# rocksdb-* store, restart so genesis re-applies) or to move the labels to fresh slots.
#
# Account arguments are `CliAccountMention`s, whose grammar is `Public/<id>` or `Private/<id>`
# -- a BARE base58 id fails to parse as an id and is silently retried as a LABEL, so it dies
# with "No account found for label `<id>`". Hence the `Public/` prefixes below. (`acct_id`
# deliberately returns the bare form; spel's `--program` wants that one.)
deploy_program() {
  local name="$1" bin="$2" header n i segs=()
  [ -f "$bin" ] || die "program binary not found: $bin (run 'make build-programs')"
  header="$(acct_id "${name}-header")" || die "missing account label: ${name}-header"
  n="$(segment_count "$bin")"
  [ "$n" -le "$DEPLOY_SEGMENT_SLOTS" ] \
    || die "$name needs $n segments but only $DEPLOY_SEGMENT_SLOTS labels are reserved; raise DEPLOY_SEGMENT_SLOTS"
  for i in $(seq 1 "$n"); do
    segs+=("Public/$(acct_id "${name}-seg-${i}")") || die "missing account label: ${name}-seg-${i}"
  done
  log "${DIM}  $bin -> $n segment(s), header ${header}${RST}"
  run_tx strict "deploy $name program" -- wallet program-loader deploy \
    --elf "$bin" --header "Public/$header" --segments "${segs[@]}" --immutable \
    --payer "Public/$DEPLOY_PAYER"
  printf '%s' "$header"
}

# Compute the faucet's singleton mint-authority PDA (base58) from the faucet program's
# account id. Delegates to the token_mint_authority `mint_authority` example. Since
# v0.2.5 PDAs derive from the deployed header's account id, not the binary's ImageID,
# so this takes an account id and is stable across rebuilds — but changes if the
# program is deployed to a different header.
mint_authority_pda() {
  local program="$1" out pda
  out="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p token_mint_authority_program \
           --example mint_authority -- "$program" 2>&1)" \
    || { echo "$out" >&2; die "mint_authority example failed for $program"; }
  # The example prints a line like:  base58: <account id>
  pda="$(printf '%s' "$out" | awk -F'base58:[[:space:]]*' 'NF>1 {print $2; exit}' \
           | grep -oE '[1-9A-HJ-NP-Za-km-z]{32,44}' | head -n1 || true)"
  [ -n "$pda" ] || { echo "$out" >&2; die "could not parse base58 mint-authority PDA from example output"; }
  printf '%s' "$pda"
}

# Compute the faucet's per-(recipient, definition) mint-allowance PDA (base58).
# Delegates to the token_mint_authority `faucet_allowance` example. This is the
# rate-limit account FaucetMint claims/reads, and a required instruction input, so
# it must be derived up front. Derives from the faucet's deployed account id.
#   mint_allowance_pda <faucet_program_id> <recipient_base58> <definition_base58>
mint_allowance_pda() {
  local program="$1" recipient="$2" definition="$3" out pda
  out="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p token_mint_authority_program \
           --example faucet_allowance -- "$program" "$recipient" "$definition" 2>&1)" \
    || { echo "$out" >&2; die "faucet_allowance example failed for $program"; }
  pda="$(printf '%s' "$out" | awk -F'base58:[[:space:]]*' 'NF>1 {print $2; exit}' \
           | grep -oE '[1-9A-HJ-NP-Za-km-z]{32,44}' | head -n1 || true)"
  [ -n "$pda" ] || { echo "$out" >&2; die "could not parse base58 mint-allowance PDA from example output"; }
  printf '%s' "$pda"
}

# Resolve a wallet account id (bare base58) from its label. Deterministic under
# the test mnemonic. Returns non-zero if the label isn't registered yet.
acct_id() {
  local label="$1" out
  out="$(wallet account id --account-id "$label" 2>/dev/null)" || return 1
  printf '%s' "$out" | grep -oE '[1-9A-HJ-NP-Za-km-z]{32,44}' | head -n1
}

# Restore the isolated test wallet from the fixed mnemonic. `restore-keys`
# REWRITES storage (safe here — it's a throwaway test home). Only the key
# material is restored here; account registration is a separate idempotent step
# (ensure_accounts) so adding a label doesn't require a full re-restore.
restore_test_wallet() {
  sec "Bootstrap deterministic test wallet"
  kv "wallet home" "$TEST_WALLET_HOME"
  mkdir -p "$TEST_WALLET_HOME"

  # The sequencer + poll config is written by write_wallet_config() before this
  # runs (v0.2.1 schema; `wallet config set` can't set the sequencer).

  # On a FRESH home (no storage.json) `wallet` runs a first-run setup BEFORE the
  # subcommand: main.rs reads a password (`Input password:`), auto-generates a
  # throwaway wallet, stores it, and only THEN runs `restore-keys`, which reads the
  # recovery phrase (`Input recovery phrase:`) and its own password. So a fresh home
  # consumes THREE stdin lines in this order — setup-password, recovery-phrase,
  # restore-password — not two. (An already-initialised home skips the setup read
  # and takes only two, which is why re-running a failed bootstrap "worked".)
  #
  # Force the deterministic fresh-home path by clearing storage, then feed all three
  # lines. restore-keys REWRITES storage, so the final wallet is the deterministic
  # $TEST_MNEMONIC one with password $TEST_WALLET_PASSWORD.
  rm -f "$TEST_WALLET_HOME/storage.json" "$TEST_WALLET_HOME/statistics.json"
  log "${DIM}\$ printf '<password>\\n<mnemonic>\\n<password>\\n' | wallet restore-keys --depth $TEST_WALLET_DEPTH${RST}"
  printf '%s\n%s\n%s\n' "$TEST_WALLET_PASSWORD" "$TEST_MNEMONIC" "$TEST_WALLET_PASSWORD" \
    | wallet restore-keys --depth "$TEST_WALLET_DEPTH" \
    || die "wallet restore-keys failed"
}

# Register the deterministic accounts, in ACCOUNT_LABELS order. Idempotent —
# skips accounts that already resolve, so newly-appended labels (e.g. token-c-*)
# are created on an existing wallet WITHOUT re-restoring keys. Their ids stay
# deterministic because they're appended after the pre-existing accounts.
ensure_accounts() {
  sec "Ensure deterministic test accounts"
  local label
  for label in "${ACCOUNT_LABELS[@]}"; do
    if acct_id "$label" >/dev/null 2>&1; then
      kv "exists" "$label"
    else
      log "${DIM}\$ wallet account new public --label $label${RST}"
      # Feed the password in case the wallet prompts to unlock before writing.
      printf '%s\n' "$TEST_WALLET_PASSWORD" \
        | wallet account new public --label "$label" \
        || die "failed to create account: $label (try FORCE_BOOTSTRAP=1 to re-restore)"
    fi
  done
}

# Write the wallet config in the v0.2.1 schema. That version replaced the old
# `sequencer_addr` string with a `sequencers` LIST and `wallet config set` refuses
# the sequencer field ("Unknown field"), so we write the file directly — which
# also replaces any stale old-format config the new wallet can't deserialize
# ("missing field `sequencers`"). The widened poll window (see TEST_SEQ_* above)
# is baked in here. Runs on every setup, before any wallet command reads config.
write_wallet_config() {
  local addr="${TEST_SEQUENCER_ADDR:-http://127.0.0.1:3040}"
  sec "Write wallet config (v0.2.1 schema)"
  mkdir -p "$TEST_WALLET_HOME"
  kv "sequencer_addr"         "$addr"
  kv "seq_tx_poll_max_blocks" "$TEST_SEQ_TX_POLL_MAX_BLOCKS"
  kv "seq_poll_timeout"       "$TEST_SEQ_POLL_TIMEOUT"
  cat > "$TEST_WALLET_HOME/wallet_config.json" <<JSON
{
  "sequencers": [{ "sequencer_addr": "$addr" }],
  "seq_poll_timeout": "$TEST_SEQ_POLL_TIMEOUT",
  "seq_tx_poll_max_blocks": $TEST_SEQ_TX_POLL_MAX_BLOCKS,
  "seq_poll_max_retries": 5,
  "seq_block_poll_max_amount": 100
}
JSON
}

###############################################################################
# 0. Preflight + wallet bootstrap
###############################################################################
sec "Preflight"
require_cmd wallet
require_cmd spel
require_cmd cargo
require_file "$TOKEN_BIN"; require_file "$AMM_BIN"; require_file "$TWAP_BIN"; require_file "$MINT_AUTHORITY_BIN"
require_file "$TOKEN_IDL"; require_file "$AMM_IDL"; require_file "$MINT_AUTHORITY_IDL"
kv "repo root"        "$REPO_ROOT"
kv "token bin" "$TOKEN_BIN"; kv "amm bin" "$AMM_BIN"; kv "twap bin" "$TWAP_BIN"
kv "mint-authority bin" "$MINT_AUTHORITY_BIN"

# Decide whether keys need restoring from the key material (storage.json), NOT the
# home dir — write_wallet_config below creates the dir, so a dir check would always
# read as "already bootstrapped".
NEEDS_KEY_RESTORE=0
if [ ! -f "$TEST_WALLET_HOME/storage.json" ] || [ "${FORCE_BOOTSTRAP:-0}" = "1" ]; then
  NEEDS_KEY_RESTORE=1
fi

# Write the v0.2.1 wallet config FIRST so every wallet command below can read it
# (and any stale old-format config is replaced). Also sets the sequencer + widened
# poll window.
write_wallet_config

if [ "$NEEDS_KEY_RESTORE" = "1" ]; then
  restore_test_wallet
else
  kv "test wallet" "reusing $TEST_WALLET_HOME (FORCE_BOOTSTRAP=1 to re-restore keys)"
fi
# Always register accounts — creates any newly-added labels (e.g. token-c-*) on
# an existing wallet without a full key re-restore.
ensure_accounts

###############################################################################
# 1. Resolve the deterministic test accounts
###############################################################################
sec "Resolve deterministic test accounts (from the test mnemonic)"
TOKEN_A_DEF="$(acct_id token-a-def)"        || die "token-a-def not registered — run with FORCE_BOOTSTRAP=1"
TOKEN_A_HOLDING="$(acct_id token-a-holding)" || die "token-a-holding not registered"
TOKEN_B_DEF="$(acct_id token-b-def)"        || die "token-b-def not registered"
TOKEN_B_HOLDING="$(acct_id token-b-holding)" || die "token-b-holding not registered"
USER_HOLDING_LP="$(acct_id lp-holding)"     || die "lp-holding not registered"
TOKEN_C_DEF="$(acct_id token-c-def)"        || die "token-c-def not registered"
TOKEN_C_HOLDING="$(acct_id token-c-holding)" || die "token-c-holding not registered"
TOKEN_D_DEF="$(acct_id token-d-def)"        || die "token-d-def not registered"
TOKEN_D_HOLDING="$(acct_id token-d-holding)" || die "token-d-holding not registered"
# "Token A Holder 2" — the faucet recipient/signer and its (initially empty) token A holding.
HOLDER2="$(acct_id holder2)"                 || die "holder2 not registered — run with FORCE_BOOTSTRAP=1"
HOLDER2_A_HOLDING="$(acct_id holder2-a-holding)" || die "holder2-a-holding not registered"
# `amm-owner` signs initialize — the AMM instance's namespace owner.
AMM_OWNER="$(acct_id amm-owner)"            || die "amm-owner not registered"
for v in TOKEN_A_DEF TOKEN_A_HOLDING TOKEN_B_DEF TOKEN_B_HOLDING USER_HOLDING_LP TOKEN_C_DEF TOKEN_C_HOLDING TOKEN_D_DEF TOKEN_D_HOLDING HOLDER2 HOLDER2_A_HOLDING AMM_OWNER; do
  [ -n "${!v}" ] || die "failed to resolve account id for $v"
done

# Derived roles: the holding accounts sign creation and seed the pool; the AMM
# authority is the A holding. Mint authority is deliberately NOT the holding — it's
# the faucet PDA, set in step 3 once the faucet binary is deployed and its ImageID
# (hence the PDA) is known.
AMM_AUTHORITY="$TOKEN_A_HOLDING"
# Namespace of the AMM instance: `amm-owner` signs initialize; the all-zero nonce is
# its default instance. The config PDA (below) is derived from (AMM_OWNER, AMM_NONCE).
AMM_NONCE="0000000000000000000000000000000000000000000000000000000000000000"
USER_HOLDING_A="$TOKEN_A_HOLDING"; USER_HOLDING_B="$TOKEN_B_HOLDING"

kv "token-a-def"     "$TOKEN_A_DEF"
kv "token-a-holding" "$TOKEN_A_HOLDING"
kv "token-b-def"     "$TOKEN_B_DEF"
kv "token-b-holding" "$TOKEN_B_HOLDING"
kv "lp-holding"      "$USER_HOLDING_LP"
kv "token-c-def"     "$TOKEN_C_DEF"
kv "token-c-holding" "$TOKEN_C_HOLDING"
kv "token-d-def"     "$TOKEN_D_DEF"
kv "token-d-holding" "$TOKEN_D_HOLDING"
kv "holder2"         "$HOLDER2"
kv "holder2-a-holding" "$HOLDER2_A_HOLDING"
kv "amm-owner"       "$AMM_OWNER"

###############################################################################
# 2. Deploy programs
###############################################################################
# The payer for every deploy. Header and segment accounts are freshly claimed and hold
# no balance, so unlike every other transaction here they cannot self-pay.
#
# The default is THIS wallet's root public account (the one with an empty chain index,
# shown as `/ Public/...` by `wallet account list`) -- the account the wallet already
# self-pays every other transaction in this script from. It is derived from
# TEST_MNEMONIC, so it is fixed as long as that is, but it is NOT funded by default:
# whatever chain this runs against must give it a genesis supply
# (`genesis[].supply_account` in the sequencer config).
#
# Override with TEST_DEPLOY_PAYER when running against a different chain, or if
# TEST_MNEMONIC is overridden -- a different mnemonic derives a different root account
# and this constant no longer matches.
DEPLOY_PAYER="${TEST_DEPLOY_PAYER:-EcduC1KZwTN7Z1LknQg8LUMtayiaJPthAsHCneX5MimP}"
sec "Deploy payer"
kv "payer" "$DEPLOY_PAYER"

deploy_program token          "$TOKEN_BIN"          >/dev/null
deploy_program amm            "$AMM_BIN"            >/dev/null
deploy_program twap           "$TWAP_BIN"           >/dev/null
deploy_program mint-authority "$MINT_AUTHORITY_BIN" >/dev/null

###############################################################################
# 3. Program IDs
###############################################################################
# A program is addressed by its deployed `ProgramHeader` account, so its id is just
# the header account this script created and deployed into -- base58, like any other
# account id, and stable across guest rebuilds.
sec "Program IDs (the deployed header accounts)"
TOKEN_PID="$(acct_id token-header)";                     kv "token program id"          "$TOKEN_PID"
AMM_PID="$(acct_id amm-header)";                         kv "amm program id"            "$AMM_PID"
TWAP_PID="$(acct_id twap-header)";                       kv "twap program id"           "$TWAP_PID"
MINT_AUTHORITY_PID="$(acct_id mint-authority-header)";   kv "mint-authority program id" "$MINT_AUTHORITY_PID"

# The faucet's singleton mint-authority PDA is derived from the DEPLOYED faucet
# binary's ImageID. Every faucet token's definition sets its mint_authority to this
# PDA, so afterwards only the faucet — via its program seed — can mint more. Derived
# from the exact bin deployed above, so it stays correct across guest rebuilds.
MINT_AUTHORITY_PDA="$(mint_authority_pda "$MINT_AUTHORITY_PID")"
kv "faucet mint-authority PDA" "$MINT_AUTHORITY_PDA"
TOKEN_A_MINT_AUTH="$MINT_AUTHORITY_PDA"; TOKEN_B_MINT_AUTH="$MINT_AUTHORITY_PDA"
TOKEN_C_MINT_AUTH="$MINT_AUTHORITY_PDA"; TOKEN_D_MINT_AUTH="$MINT_AUTHORITY_PDA"

# The faucet-swap test's FaucetMint needs holder2's per-(recipient, token A) allowance
# PDA — the rate-limit account it claims on first mint. Derived here (recipient=holder2,
# definition=token A) so the test can pass it straight to spel without touching cargo.
HOLDER2_ALLOWANCE_PDA="$(mint_allowance_pda "$MINT_AUTHORITY_PID" "$HOLDER2" "$TOKEN_A_DEF")"
kv "holder2 allowance PDA" "$HOLDER2_ALLOWANCE_PDA"

###############################################################################
# 4. Create token definitions (mint supply to the holding accounts)
###############################################################################
run_tx strict "create fungible definition: $TOKEN_A_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_PID" --fee-payer "$DEPLOY_PAYER" -- new-fungible-definition \
    --name "$TOKEN_A_NAME" --total-supply "$TOKEN_A_SUPPLY" \
    --definition-target-account "$TOKEN_A_DEF" \
    --holding-target-account "$TOKEN_A_HOLDING" \
    --mint-authority "$TOKEN_A_MINT_AUTH"

run_tx strict "create fungible definition: $TOKEN_B_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_PID" --fee-payer "$DEPLOY_PAYER" -- new-fungible-definition \
    --name "$TOKEN_B_NAME" --total-supply "$TOKEN_B_SUPPLY" \
    --definition-target-account "$TOKEN_B_DEF" \
    --holding-target-account "$TOKEN_B_HOLDING" \
    --mint-authority "$TOKEN_B_MINT_AUTH"

# Token C has no seeded pool — the create-pool UI test creates the A/C pool.
run_tx strict "create fungible definition: $TOKEN_C_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_PID" --fee-payer "$DEPLOY_PAYER" -- new-fungible-definition \
    --name "$TOKEN_C_NAME" --total-supply "$TOKEN_C_SUPPLY" \
    --definition-target-account "$TOKEN_C_DEF" \
    --holding-target-account "$TOKEN_C_HOLDING" \
    --mint-authority "$TOKEN_C_MINT_AUTH"

# Token D is deliberately LEFT OUT of the token config below — the custom-token UI
# test pastes its id to add it as a custom token. Its definition must exist on-chain
# so the app can resolve it.
run_tx strict "create fungible definition: $TOKEN_D_NAME" -- \
  spel --idl "$TOKEN_IDL" --program "$TOKEN_PID" --fee-payer "$DEPLOY_PAYER" -- new-fungible-definition \
    --name "$TOKEN_D_NAME" --total-supply "$TOKEN_D_SUPPLY" \
    --definition-target-account "$TOKEN_D_DEF" \
    --holding-target-account "$TOKEN_D_HOLDING" \
    --mint-authority "$TOKEN_D_MINT_AUTH"

###############################################################################
# 5. Verify token definitions & holdings
###############################################################################
inspect "$TOKEN_IDL" "$TOKEN_A_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_A_HOLDING" "TokenHolding"
inspect "$TOKEN_IDL" "$TOKEN_B_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_B_HOLDING" "TokenHolding"
inspect "$TOKEN_IDL" "$TOKEN_C_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_C_HOLDING" "TokenHolding"
inspect "$TOKEN_IDL" "$TOKEN_D_DEF"     "TokenDefinition"
inspect "$TOKEN_IDL" "$TOKEN_D_HOLDING" "TokenHolding"

###############################################################################
# 6. Derive AMM PDAs from the program ids + token pair
###############################################################################
sec "Deriving AMM PDAs (amm_pdas example)"
log "${DIM}\$ cargo run -q -p amm_program --example amm_pdas -- $AMM_PID $AMM_OWNER $TWAP_PID $TOKEN_A_DEF $TOKEN_B_DEF${RST}"
PDAS="$(RISC0_DEV_MODE=1 RISC0_SKIP_BUILD=1 cargo run -q -p amm_program --example amm_pdas -- \
          "$AMM_PID" "$AMM_OWNER" "$TWAP_PID" "$TOKEN_A_DEF" "$TOKEN_B_DEF")"
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
  spel --idl "$AMM_IDL" --program "$AMM_PID" --fee-payer "$DEPLOY_PAYER" -- initialize \
    --owner "$AMM_OWNER" \
    --config "$CONFIG" \
    --nonce "$AMM_NONCE" \
    --token-program-id "$TOKEN_PID" \
    --twap-oracle-program-id "$TWAP_PID" \
    --authority "$AMM_AUTHORITY" \
    --swap-fee-bps "$SWAP_FEE_BPS" \
    --protocol-fee-bps "$PROTOCOL_FEE_BPS"

###############################################################################
# 8. Create the pool (seed initial liquidity)
###############################################################################
run_tx strict "create pool + seed liquidity" -- \
  spel --idl "$AMM_IDL" --program "$AMM_PID" --fee-payer "$DEPLOY_PAYER" -- new-definition \
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
    --deadline "$POOL_DEADLINE"

###############################################################################
# 9. Verify the pool
###############################################################################
inspect "$AMM_IDL" "$POOL" "PoolDefinition"

###############################################################################
# 10. Fund the UI-signing accounts so they can pay transaction fees
###############################################################################
# Since LEZ v0.2.5 every public transaction reserves a fee, and the UI submits with
# an EMPTY payer, which wallet-ffi reads as "self-pay from the first signing
# account". For a swap that is `user_input_holding` (the only signer -- see
# modules/amm/ffi/src/api/swap.rs); for add/remove liquidity it is `user_a`. Those
# are the holding accounts below, and they are created with a zero native balance,
# so without this step the sequencer refuses the transaction outright
# ("Incorrect fee" / PayerCannotFund) -- before execution, so nothing reaches a
# block and the UI just sees a failed submit.
#
# This MUST run after section 8: the creating instructions (`new_fungible_definition`,
# the pool's `new_definition`) assert their targets equal `Account::default()`, and a
# balance is enough to break that -- permanently, since signing the reverted tx still
# burns the account's nonce.
#
# `holder2-a-holding` is deliberately NOT funded: it does not exist yet. faucet-swap.mjs
# creates it with the token program's `initialize_account`, which asserts
# `Account::default()` too. It needs funding between that step and its first swap.
FUND_AMOUNT="${TEST_FUND_AMOUNT:-1000000000000}"
FUND_ACCOUNTS=(token-a-holding token-b-holding token-c-holding token-d-holding lp-holding holder2)

# Idempotent: re-running tops up only what has fallen below the target, so repeated
# setups neither drain the payer nor pile up balance.
# Prints the account's native balance, or nothing at all when the account does not
# exist on chain yet (which is the normal state for an account nothing has funded).
# Careful with `set -euo pipefail` here: a `grep` that matches nothing exits 1, which
# pipefail promotes to the whole pipeline and `set -e` turns into an aborted run. `sed`
# alone always exits 0, and the read itself is tolerated with `|| true`, so a missing
# account yields an empty string rather than killing the script.
native_balance() {
  local out
  out="$(wallet account get -a "Public/$1" 2>/dev/null || true)"
  printf '%s' "$out" | sed -n 's/.*"balance":\([0-9][0-9]*\).*/\1/p' | head -1
}

fund_account() {
  local label="$1" id bal
  id="$(acct_id "$label")" || die "missing account label: $label"
  bal="$(native_balance "$id")"
  bal="${bal:-0}"
  if [ "$bal" -ge "$FUND_AMOUNT" ] 2>/dev/null; then
    kv "$label" "$id (already funded: $bal)"
    return 0
  fi
  run_tx strict "fund $label for fees" -- wallet auth-transfer send \
    --from "Public/$DEPLOY_PAYER" --to "Public/$id" --amount "$FUND_AMOUNT"
  kv "$label" "$id (funded $FUND_AMOUNT)"
}

sec "Fund UI-signing accounts (native balance for fees)"
kv "from payer" "$DEPLOY_PAYER"
kv "amount each" "$FUND_AMOUNT"
for _acct in "${FUND_ACCOUNTS[@]}"; do
  fund_account "$_acct"
done
unset _acct

###############################################################################
# 11. Write the UI token config from the deterministic accounts
###############################################################################
# NOTE: token D is intentionally NOT written here — it is the "custom" token the
# custom-token.mjs test adds by id, so it must be absent from the known list.
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
  },
  {
    "symbol": "$TOKEN_C_SYMBOL",
    "name": "$TOKEN_C_NAME",
    "definitionId": "$TOKEN_C_DEF",
    "holding": "$TOKEN_C_HOLDING",
    "decimals": $TOKEN_C_DECIMALS
  }
]
JSON
kv "wrote" "$TOKENS_CONFIG_OUT"

###############################################################################
# 12. Write the UI known-pools config from the seeded pool(s)
###############################################################################
sec "Write UI pools config -> $POOLS_CONFIG_OUT"
# One row per seeded pool: "SYMBOL_A SYMBOL_B POOL_ID DEF_A DEF_B". The swap fee is
# instance-wide (AMM config), not per pool, so it is no longer part of a pool entry.
# Add a line here for each new seeded pool — nothing else (script or app) needs
# to change; the Pools page renders one row per entry generically.
POOL_SPECS=(
  "$TOKEN_A_SYMBOL $TOKEN_B_SYMBOL $POOL $TOKEN_A_DEF $TOKEN_B_DEF"
)

pool_entry() {
  cat <<JSON
  {
    "tokenA": "$1",
    "tokenB": "$2",
    "poolId": "$3",
    "tokenADefinitionId": "$4",
    "tokenBDefinitionId": "$5"
  }
JSON
}

{
  echo "["
  for i in "${!POOL_SPECS[@]}"; do
    [ "$i" -gt 0 ] && echo "  ,"
    # shellcheck disable=SC2086 # deliberate word-split of the spec into fields
    set -- ${POOL_SPECS[$i]}
    pool_entry "$1" "$2" "$3" "$4" "$5"
  done
  echo "]"
} > "$POOLS_CONFIG_OUT"
kv "wrote" "$POOLS_CONFIG_OUT"

###############################################################################
# 11b. Write the single-file registry (for testing the AMM_REGISTRY_URL path)
###############################################################################
sec "Write UI registry config -> $REGISTRY_CONFIG_OUT"
# Same tokens/pools in the remote-registry shape: one "local" network, holding-
# agnostic tokens (the app resolves holdings from the wallet). programIds carry the
# freshly deployed ids so the lone-network rule auto-selects "local" and the app
# adopts its amm program id — no AMM_PROGRAM_BIN needed.
{
  cat <<JSON
{
  "name": "AMM local registry",
  "version": "0.1.0",
  "networks": [
    { "id": "local", "name": "Local", "programIds": { "amm": "$AMM_PID", "token": "$TOKEN_PID", "tokenMintAuthority": "$MINT_AUTHORITY_PID" }, "ammConfigId": "$CONFIG" }
  ],
  "tokens": [
    { "network": "local", "symbol": "$TOKEN_A_SYMBOL", "name": "$TOKEN_A_NAME", "definitionId": "$TOKEN_A_DEF" },
    { "network": "local", "symbol": "$TOKEN_B_SYMBOL", "name": "$TOKEN_B_NAME", "definitionId": "$TOKEN_B_DEF" },
    { "network": "local", "symbol": "$TOKEN_C_SYMBOL", "name": "$TOKEN_C_NAME", "definitionId": "$TOKEN_C_DEF" }
  ],
  "pools": [
JSON
  for i in "${!POOL_SPECS[@]}"; do
    [ "$i" -gt 0 ] && echo "    ,"
    # shellcheck disable=SC2086 # deliberate word-split of the spec into fields
    set -- ${POOL_SPECS[$i]}
    cat <<JSON
    { "network": "local", "tokenA": "$1", "tokenB": "$2", "poolId": "$3", "tokenADefinitionId": "$4", "tokenBDefinitionId": "$5" }
JSON
  done
  cat <<JSON
  ]
}
JSON
} > "$REGISTRY_CONFIG_OUT"
kv "wrote" "$REGISTRY_CONFIG_OUT"

###############################################################################
# 13. Initialize the isolated custom-token store (empty)
###############################################################################
sec "Write custom-token store -> $CUSTOM_TOKEN_CONFIG_OUT"
# Initialize the isolated custom-token store empty so a test run starts with no
# custom tokens. custom-token.mjs adds token D by id and clears this again after.
printf '%s\n' "[]" > "$CUSTOM_TOKEN_CONFIG_OUT"
kv "wrote" "$CUSTOM_TOKEN_CONFIG_OUT (empty)"

###############################################################################
# 14. Write the faucet manifest for the faucet-swap UI test
###############################################################################
sec "Write faucet manifest -> $FAUCET_MANIFEST_OUT"
# Everything apps/amm/tests/faucet-swap.mjs needs. The six faucet accounts are the
# exact, ordered FaucetMint inputs (recipient, mint_allowance, user_holding,
# token_definition, mint_authority, clock). tokenProgramId/tokenIdl let the test first
# `initialize_account` holder2's token A holding (the faucet only mints into an
# EXISTING holding — it doesn't sign user_holding, so it can't create a fresh one),
# then FaucetMint into it. Since v0.2.5 spel names a program by its deployed header
# account id, so the manifest carries ids rather than binary paths; IDL paths stay
# repo-relative (resolved against the repo root).
cat > "$FAUCET_MANIFEST_OUT" <<JSON
{
  "tokenProgramId": "$TOKEN_PID",
  "tokenIdl": "$TOKEN_IDL",
  "faucetProgramId": "$MINT_AUTHORITY_PID",
  "faucetIdl": "$MINT_AUTHORITY_IDL",
  "recipient": "$HOLDER2",
  "mintAllowance": "$HOLDER2_ALLOWANCE_PDA",
  "userHolding": "$HOLDER2_A_HOLDING",
  "tokenDefinition": "$TOKEN_A_DEF",
  "mintAuthority": "$MINT_AUTHORITY_PDA",
  "clock": "$CLOCK_ACCOUNT",
  "feePayer": "$DEPLOY_PAYER",
  "fundAmount": "$FUND_AMOUNT",
  "tokenASymbol": "$TOKEN_A_SYMBOL",
  "tokenBSymbol": "$TOKEN_B_SYMBOL"
}
JSON
kv "wrote" "$FAUCET_MANIFEST_OUT"

sec "Done"
log "${GRN}✅ Setup complete.${RST}"
kv "AMM program id"            "$AMM_PID"
kv "TWAP program id"           "$TWAP_PID"
kv "mint-authority program id" "$MINT_AUTHORITY_PID"
kv "faucet mint-authority PDA" "$MINT_AUTHORITY_PDA"
kv "pool"                      "$POOL"
log ""
log "All test tokens' mint_authority is the faucet PDA above — mint more of any of"
log "them through the token-mint-authority program (${DIM}FaucetMint${RST}). A brand-new account"
log "can fund itself this way before swapping."
log ""
log "Launch the UI against the ISOLATED test wallet + test token config:"
log "${YEL}AMM_CONFIG_ID is required on this path:${RST} the local token/pool files carry no"
log "namespace, so without it the app has no AMM instance to derive pool, vault and"
log "holding PDAs from — balances and positions come up empty and swap stays disabled."
log "  ${DIM}LEE_WALLET_HOME_DIR=$TEST_WALLET_HOME \\${RST}"
log "  ${DIM}  AMM_PROGRAM_BIN=$REPO_ROOT/$AMM_BIN \\${RST}"
log "  ${DIM}  AMM_CONFIG_ID=$CONFIG \\${RST}"
log "  ${DIM}  TOKENS_CONFIG=$REPO_ROOT/$TOKENS_CONFIG_OUT \\${RST}"
log "  ${DIM}  AMM_POOLS_CONFIG=$REPO_ROOT/$POOLS_CONFIG_OUT \\${RST}"
log "  ${DIM}  CUSTOM_TOKEN_CONFIG=$REPO_ROOT/$CUSTOM_TOKEN_CONFIG_OUT \\${RST}"
log "  ${DIM}  nix run .#amm-ui${RST}"
log ""
log "Or exercise the remote-registry path against the same sequencer — omit"
log "TOKENS_CONFIG/AMM_POOLS_CONFIG so the local files don't take precedence. No"
log "AMM_PROGRAM_BIN: the registry carries the program ids and the app adopts them."
log "  ${DIM}LEE_WALLET_HOME_DIR=$TEST_WALLET_HOME \\${RST}"
log "  ${DIM}  AMM_REGISTRY_URL=file://$REPO_ROOT/$REGISTRY_CONFIG_OUT \\${RST}"
log "  ${DIM}  CUSTOM_TOKEN_CONFIG=$REPO_ROOT/$CUSTOM_TOKEN_CONFIG_OUT \\${RST}"
log "  ${DIM}  nix run .#amm-ui${RST}"
log ""
log "Token D was created ON-CHAIN but left out of the token config (the ${DIM}custom${RST}"
log "token). Its id: ${DIM}$TOKEN_D_DEF${RST}"
log ""
log "Then in another terminal: ${DIM}node apps/amm/tests/swap.mjs${RST}  (swap A/B)"
log "                   or:     ${DIM}node apps/amm/tests/create-pool.mjs${RST}  (create A/C pool)"
log "                   or:     ${DIM}node apps/amm/tests/custom-token.mjs${RST}  (add token D by id)"
log "                   or:     ${DIM}node apps/amm/tests/faucet-swap.mjs${RST}  (faucet-mint TKA to holder2, then swap)"
log ""
log "The faucet-swap test reads ${DIM}$FAUCET_MANIFEST_OUT${RST} and needs ${DIM}spel${RST} +"
log "${DIM}LEE_WALLET_HOME_DIR=$TEST_WALLET_HOME${RST} in its environment (same isolated wallet)."
