#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Run a two-account stablecoin borrow/transfer/repay cycle against an already-running sequencer.

Usage:
  e2e-stablecoin-market.sh --sequencer-url URL --collateral-holding ID [options]

Options (matching environment variables may be used instead):
  --logoscore PATH        Logoscore executable (LOGOSCORE_BIN, default: logoscore)
  --curl PATH             curl executable (CURL_BIN, default: curl)
  --jq PATH               jq executable (JQ_BIN, default: jq)
  --config-dir DIR        Logoscore config dir (LOGOSCORE_CONFIG_DIR)
  --sequencer-url URL     Running sequencer JSON-RPC URL (SEQUENCER_ADDR)
  --collateral-holding ID Existing funded collateral holding (COLLATERAL_HOLDING_ID)
  --output FILE           Markdown transcript (OUTPUT_FILE; default: ./stablecoin-market-<UTC>.md)
  -h, --help              Show this help

The Logoscore daemon must already be running with lez_core, token_module, and
stablecoin_module loaded, and lez_core must have an open wallet. The wallet
needs the admin collateral holding key and will retain the generated test keys.
This script never starts or stops a daemon or sequencer.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

LOGOSCORE_BIN=${LOGOSCORE_BIN:-logoscore}
CURL_BIN=${CURL_BIN:-curl}
JQ_BIN=${JQ_BIN:-jq}
LOGOSCORE_CONFIG_DIR=${LOGOSCORE_CONFIG_DIR:-}
SEQUENCER_ADDR=${SEQUENCER_ADDR:-}
COLLATERAL_HOLDING_ID=${COLLATERAL_HOLDING_ID:-}
OUTPUT_FILE=${OUTPUT_FILE:-"stablecoin-market-$(date -u +%Y%m%dT%H%M%SZ).md"}

while (($#)); do
  case "$1" in
    --logoscore) LOGOSCORE_BIN=${2:?missing path}; shift 2 ;;
    --curl) CURL_BIN=${2:?missing path}; shift 2 ;;
    --jq) JQ_BIN=${2:?missing path}; shift 2 ;;
    --config-dir) LOGOSCORE_CONFIG_DIR=${2:?missing directory}; shift 2 ;;
    --sequencer-url) SEQUENCER_ADDR=${2:?missing URL}; shift 2 ;;
    --collateral-holding) COLLATERAL_HOLDING_ID=${2:?missing account ID}; shift 2 ;;
    --output) OUTPUT_FILE=${2:?missing output path}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done

resolve_bin() {
  local candidate=$1
  if [[ "$candidate" == */* ]]; then
    [[ -x "$candidate" ]] || die "not executable: $candidate"
    printf '%s\n' "$candidate"
  else
    command -v "$candidate" || die "not found in PATH: $candidate"
  fi
}

LOGOSCORE_BIN=$(resolve_bin "$LOGOSCORE_BIN")
CURL_BIN=$(resolve_bin "$CURL_BIN")
JQ_BIN=$(resolve_bin "$JQ_BIN")
command -v date >/dev/null || die "date is required"
command -v mktemp >/dev/null || die "mktemp is required"
command -v sleep >/dev/null || die "sleep is required"
command -v sed >/dev/null || die "sed is required"
[[ -n "$SEQUENCER_ADDR" ]] || die "set SEQUENCER_ADDR or pass --sequencer-url"
[[ -n "$COLLATERAL_HOLDING_ID" ]] || die "set COLLATERAL_HOLDING_ID or pass --collateral-holding"
[[ ! -e "$OUTPUT_FILE" ]] || die "output already exists: $OUTPUT_FILE"

mkdir -p "$(dirname "$OUTPUT_FILE")"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT
touch "$OUTPUT_FILE"

CURRENT_STEP='Preflight'
RPC_ID=0
LAST_STDOUT=
LAST_STDERR=
LAST_RC=0
RPC_STDOUT=

{
  printf '# Stablecoin two-account market E2E\n\n'
  printf 'Started: %s UTC\n\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'Sequencer: `%s`\n\n' "$(printf '%s' "$SEQUENCER_ADDR" | sed -E 's#(https?://)[^/@]*@#\1[REDACTED]@#')"
  printf 'This run uses an already-running sequencer and Logoscore daemon. Each call records argv, stdout, stderr, and exit code. JSON-RPC wallet-sync and transaction-receipt calls are recorded too. Amounts are raw token units.\n'
} >> "$OUTPUT_FILE"

step() {
  CURRENT_STEP=$1
  printf '\n## %s\n\n' "$CURRENT_STEP" >> "$OUTPUT_FILE"
  printf '\n== %s ==\n' "$CURRENT_STEP"
}

log_stream() {
  local label=$1 file=$2 fence=$3
  printf '%s:\n\n```%s\n' "$label" "$fence" >> "$OUTPUT_FILE"
  if [[ -s "$file" ]]; then
    cat "$file" >> "$OUTPUT_FILE"
    cat "$file"
  else
    printf '(empty)\n' >> "$OUTPUT_FILE"
    printf '(empty)\n'
  fi
  printf '\n```\n\n' >> "$OUTPUT_FILE"
}

run_cli() {
  local description=$1
  shift
  local out_file="$TMP_DIR/cli.out" err_file="$TMP_DIR/cli.err" rc
  local -a command=("$LOGOSCORE_BIN")
  [[ -z "$LOGOSCORE_CONFIG_DIR" ]] || command+=(--config-dir "$LOGOSCORE_CONFIG_DIR")
  command+=(--json "$@")

  {
    printf '### %s\n\nInput:\n\n```console\n$' "$description"
    printf ' %q' "${command[@]}"
    printf '\n```\n\n'
  } >> "$OUTPUT_FILE"
  if "${command[@]}" >"$out_file" 2>"$err_file"; then rc=0; else rc=$?; fi
  LAST_STDOUT=$(<"$out_file")
  LAST_STDERR=$(<"$err_file")
  LAST_RC=$rc
  log_stream stdout "$out_file" json
  log_stream stderr "$err_file" text
  printf 'exit_code: %s\n\n' "$rc" >> "$OUTPUT_FILE"
  printf 'exit_code: %s\n' "$rc"
}

require_cli_ok() {
  [[ "$LAST_RC" == 0 ]] || die "Logoscore command failed; see $OUTPUT_FILE"
  printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.status == "ok"' >/dev/null \
    || die "Logoscore returned an error envelope; see $OUTPUT_FILE"
}

module_read() {
  local module=$1 method=$2
  shift 2
  run_cli "$module.$method" call "$module" "$method" "$@"
  require_cli_ok
  printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.result.status == "ok"' >/dev/null \
    || die "$module.$method failed; see $OUTPUT_FILE"
}

module_tx() {
  local module=$1 method=$2
  shift 2
  sync_wallet
  run_cli "$module.$method" call "$module" "$method" "$@"
  require_cli_ok
  printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.result.status == "ok"' >/dev/null \
    || die "$module.$method failed; see $OUTPUT_FILE"
  local txid
  txid=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.transactionId')
  confirm_tx "$txid"
}

core_tx() {
  local method=$1
  shift
  sync_wallet
  run_cli "lez_core.$method" call lez_core "$method" "$@"
  require_cli_ok
  local core_result txid
  core_result=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result | fromjson')
  printf '%s' "$core_result" | "$JQ_BIN" -e '.success == true' >/dev/null \
    || die "lez_core.$method failed; see $OUTPUT_FILE"
  txid=$(printf '%s' "$core_result" | "$JQ_BIN" -er '.tx_hash')
  confirm_tx "$txid"
}

rpc_call() {
  local method=$1 params=$2 body safe_endpoint cmd out_file="$TMP_DIR/rpc.out" err_file="$TMP_DIR/rpc.err" rc
  RPC_ID=$((RPC_ID + 1))
  body=$("$JQ_BIN" -cn --argjson id "$RPC_ID" --arg method "$method" --argjson params "$params" \
    '{jsonrpc:"2.0",id:$id,method:$method,params:$params}')
  safe_endpoint=$(printf '%s' "$SEQUENCER_ADDR" | sed -E 's#(https?://)[^/@]*@#\1[REDACTED]@#')
  printf -v cmd '%q ' "$CURL_BIN" --silent --show-error --fail --header 'Content-Type: application/json' --data-binary "$body" "$safe_endpoint"
  {
    printf '### JSON-RPC %s\n\nInput:\n\n```console\n$ %s\n' "$method" "$cmd"
    printf 'POST body:\n%s\n```\n\n' "$body"
  } >> "$OUTPUT_FILE"
  if "$CURL_BIN" --silent --show-error --fail --header 'Content-Type: application/json' \
      --data-binary "$body" "$SEQUENCER_ADDR" >"$out_file" 2>"$err_file"; then rc=0; else rc=$?; fi
  RPC_STDOUT=$(<"$out_file")
  log_stream response "$out_file" json
  log_stream stderr "$err_file" text
  printf 'exit_code: %s\n\n' "$rc" >> "$OUTPUT_FILE"
  printf 'exit_code: %s\n' "$rc"
  [[ "$rc" == 0 ]] || die "sequencer RPC $method failed; see $OUTPUT_FILE"
  printf '%s' "$RPC_STDOUT" | "$JQ_BIN" -e '.error == null' >/dev/null \
    || die "sequencer RPC $method returned an error; see $OUTPUT_FILE"
}

sync_wallet() {
  rpc_call getLastBlockId '[]'
  local height
  height=$(printf '%s' "$RPC_STDOUT" | "$JQ_BIN" -er '.result')
  [[ "$height" =~ ^[0-9]+$ ]] || die "sequencer returned an invalid block height"
  run_cli 'lez_core.sync_to_block' call lez_core sync_to_block "json:$height"
  require_cli_ok
  printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.result == 0' >/dev/null \
    || die "wallet sync failed; see $OUTPUT_FILE"
}

confirm_tx() {
  local txid=$1 params deadline block
  [[ "$txid" =~ ^(0x)?[0-9a-fA-F]{64}$ ]] \
    || die "unexpected transaction ID format: $txid"
  txid=${txid#0x}
  txid=$(printf '%s' "$txid" | tr '[:upper:]' '[:lower:]')
  params=$("$JQ_BIN" -cn --arg txid "$txid" '[$txid]')
  deadline=$(($(date +%s) + 120))
  while [[ "$(date +%s)" -lt "$deadline" ]]; do
    rpc_call getTransaction "$params"
    if printf '%s' "$RPC_STDOUT" | "$JQ_BIN" -e '.result != null' >/dev/null; then
      block=$(printf '%s' "$RPC_STDOUT" | "$JQ_BIN" -er '.result[1]')
      printf 'Confirmed in block %s\n\n' "$block" >> "$OUTPUT_FILE"
      printf 'confirmed in block %s\n' "$block"
      return 0
    fi
    sleep 0.2
  done
  die "transaction $txid was not included within 120 seconds"
}

string_arg() {
  printf 'json:%s' "$("$JQ_BIN" -cn --arg value "$1" '$value')"
}

request_arg() {
  printf 'json:%s' "$1"
}

read_holding() {
  local label=$1 holding=$2
  module_read token_module inspectHolding "$(string_arg "$holding")"
  printf '%s\n' "$LAST_STDOUT" | "$JQ_BIN" -r --arg label "$label" \
    '"\($label) balance_raw=\(.result.holding.balanceRaw) definition=\(.result.holding.definitionId)"'
}

read_supply() {
  module_read token_module inspectDefinition "$(string_arg "$STABLECOIN_DEFINITION_ID")"
  SUPPLY_RAW=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.definition.totalSupplyRaw')
  printf 'stablecoin total_supply_raw=%s\n' "$SUPPLY_RAW"
}

read_health() {
  local label=$1 owner=$2 request
  request=$("$JQ_BIN" -cn --arg owner "$owner" '{ownerId:$owner,positionNonce:"0"}')
  module_read stablecoin_module positionHealth "$(request_arg "$request")"
  printf '%s\n' "$LAST_STDOUT" | "$JQ_BIN" -r --arg label "$label" \
    '"\($label) collateral=\(.result.collateralAmount) debt=\(.result.nominalDebt) collateralized=\(.result.isCollateralized)"'
}

step 'Step 00 — Preflight and initial state'
run_cli 'Logoscore daemon status' status
[[ "$LAST_RC" == 0 ]] || die "Logoscore status failed; see $OUTPUT_FILE"
printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.daemon.status == "running"' >/dev/null \
  || die "Logoscore daemon is not running; this script does not start it"
rpc_call getLastBlockId '[]'
module_read stablecoin_module programInfo
STABLECOIN_INFO=$LAST_STDOUT
module_read stablecoin_module protocolParameters
PROTOCOL=$LAST_STDOUT
module_read token_module programInfo
run_cli 'lez_core.get_current_block_height' call lez_core get_current_block_height
require_cli_ok

ADMIN_ID=${ADMIN_ID:-$(printf '%s' "$PROTOCOL" | "$JQ_BIN" -er '.result.protocolParameters.adminId')}
COLLATERAL_DEFINITION_ID=${COLLATERAL_DEFINITION_ID:-$(printf '%s' "$PROTOCOL" | "$JQ_BIN" -er '.result.protocolParameters.collateralDefinitionId')}
STABLECOIN_DEFINITION_ID=${STABLECOIN_DEFINITION_ID:-$(printf '%s' "$STABLECOIN_INFO" | "$JQ_BIN" -er '.result.stablecoinDefinitionId')}
IS_FROZEN=$(printf '%s' "$PROTOCOL" | "$JQ_BIN" -r '.result.protocolParameters.isFrozen')
[[ "$IS_FROZEN" == false ]] || die "protocol is frozen"
read_holding Admin "$COLLATERAL_HOLDING_ID"
INITIAL_COLLATERAL_RAW=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.holding.balanceRaw')
HOLDING_DEFINITION=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.holding.definitionId')
[[ "$HOLDING_DEFINITION" == "$COLLATERAL_DEFINITION_ID" ]] \
  || die "collateral holding does not match protocol collateral definition"
read_supply
INITIAL_SUPPLY_RAW=$SUPPLY_RAW
printf 'admin=%s\ncollateral_definition=%s\nstablecoin_definition=%s\n' \
  "$ADMIN_ID" "$COLLATERAL_DEFINITION_ID" "$STABLECOIN_DEFINITION_ID"

create_user() {
  local name=$1 owner_var=$2 collateral_var=$3 stable_var=$4 owner collateral stable
  run_cli "Create $name identity" call lez_core create_account_public
  require_cli_ok
  owner=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result')
  core_tx register_public_account "$(string_arg "$owner")"
  run_cli "Create $name collateral holding" call lez_core create_account_public
  require_cli_ok
  collateral=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result')
  run_cli "Create $name stablecoin holding" call lez_core create_account_public
  require_cli_ok
  stable=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result')
  printf -v "$owner_var" '%s' "$owner"
  printf -v "$collateral_var" '%s' "$collateral"
  printf -v "$stable_var" '%s' "$stable"
}

step 'Step 01 — Account A: identity and holdings'
create_user 'Account A' A_OWNER A_COLLATERAL A_STABLE
module_tx token_module initializeHolding "$(string_arg "$COLLATERAL_DEFINITION_ID")" "$(string_arg "$A_COLLATERAL")"
read_holding 'Account A collateral' "$A_COLLATERAL"
module_tx token_module initializeHolding "$(string_arg "$STABLECOIN_DEFINITION_ID")" "$(string_arg "$A_STABLE")"
read_holding 'Account A stablecoin' "$A_STABLE"

step 'Step 02 — Account B: identity and holdings'
create_user 'Account B' B_OWNER B_COLLATERAL B_STABLE
module_tx token_module initializeHolding "$(string_arg "$COLLATERAL_DEFINITION_ID")" "$(string_arg "$B_COLLATERAL")"
read_holding 'Account B collateral' "$B_COLLATERAL"
module_tx token_module initializeHolding "$(string_arg "$STABLECOIN_DEFINITION_ID")" "$(string_arg "$B_STABLE")"
read_holding 'Account B stablecoin' "$B_STABLE"

step 'Step 03 — Admin funds collateral holdings'
module_tx token_module transfer "$(string_arg "$COLLATERAL_HOLDING_ID")" "$(string_arg "$A_COLLATERAL")" "$(string_arg 1000)"
read_holding Admin "$COLLATERAL_HOLDING_ID"
read_holding 'Account A collateral' "$A_COLLATERAL"
module_tx token_module transfer "$(string_arg "$COLLATERAL_HOLDING_ID")" "$(string_arg "$B_COLLATERAL")" "$(string_arg 2000)"
read_holding Admin "$COLLATERAL_HOLDING_ID"
read_holding 'Account B collateral' "$B_COLLATERAL"

step 'Step 04 — Account A: open position and borrow'
REQUEST=$("$JQ_BIN" -cn --arg owner "$A_OWNER" --arg holding "$A_COLLATERAL" \
  '{ownerId:$owner,positionNonce:"0",userCollateralHoldingId:$holding,initialCollateralAmount:"1000"}')
module_tx stablecoin_module openPosition "$(request_arg "$REQUEST")"
read_health 'Account A' "$A_OWNER"
read_holding 'Account A collateral' "$A_COLLATERAL"
REQUEST=$("$JQ_BIN" -cn --arg owner "$A_OWNER" --arg holding "$A_STABLE" \
  '{ownerId:$owner,positionNonce:"0",userStablecoinHoldingId:$holding,amount:"400"}')
module_tx stablecoin_module generateDebt "$(request_arg "$REQUEST")"
read_health 'Account A' "$A_OWNER"
read_holding 'Account A stablecoin' "$A_STABLE"
read_supply
module_read stablecoin_module currentGlobalState

step 'Step 05 — Account B: open position and borrow'
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" --arg holding "$B_COLLATERAL" \
  '{ownerId:$owner,positionNonce:"0",userCollateralHoldingId:$holding,initialCollateralAmount:"2000"}')
module_tx stablecoin_module openPosition "$(request_arg "$REQUEST")"
read_health 'Account B' "$B_OWNER"
read_holding 'Account B collateral' "$B_COLLATERAL"
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" --arg holding "$B_STABLE" \
  '{ownerId:$owner,positionNonce:"0",userStablecoinHoldingId:$holding,amount:"800"}')
module_tx stablecoin_module generateDebt "$(request_arg "$REQUEST")"
read_health 'Account B' "$B_OWNER"
read_holding 'Account B stablecoin' "$B_STABLE"
read_supply
module_read stablecoin_module currentGlobalState

step 'Step 06 — Market transfer: Account A to Account B'
module_tx token_module transfer "$(string_arg "$A_STABLE")" "$(string_arg "$B_STABLE")" "$(string_arg 100)"
read_holding 'Account A stablecoin' "$A_STABLE"
read_holding 'Account B stablecoin' "$B_STABLE"
read_supply

step 'Step 07 — Account B: partial repayment'
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" --arg holding "$B_STABLE" \
  '{ownerId:$owner,positionNonce:"0",userStablecoinHoldingId:$holding,amount:"200"}')
module_tx stablecoin_module repayDebt "$(request_arg "$REQUEST")"
read_health 'Account B' "$B_OWNER"
read_holding 'Account B stablecoin' "$B_STABLE"
read_supply
module_read stablecoin_module currentGlobalState

step 'Step 08 — Account B returns funds; both accounts repay'
module_tx token_module transfer "$(string_arg "$B_STABLE")" "$(string_arg "$A_STABLE")" "$(string_arg 100)"
read_holding 'Account A stablecoin' "$A_STABLE"
read_holding 'Account B stablecoin' "$B_STABLE"
read_supply
REQUEST=$("$JQ_BIN" -cn --arg owner "$A_OWNER" --arg holding "$A_STABLE" \
  '{ownerId:$owner,positionNonce:"0",userStablecoinHoldingId:$holding,amount:"400"}')
module_tx stablecoin_module repayDebt "$(request_arg "$REQUEST")"
read_health 'Account A' "$A_OWNER"
read_holding 'Account A stablecoin' "$A_STABLE"
read_supply
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" --arg holding "$B_STABLE" \
  '{ownerId:$owner,positionNonce:"0",userStablecoinHoldingId:$holding,amount:"600"}')
module_tx stablecoin_module repayDebt "$(request_arg "$REQUEST")"
read_health 'Account B' "$B_OWNER"
read_holding 'Account B stablecoin' "$B_STABLE"
read_supply

step 'Step 09 — Account A: withdraw collateral and close'
REQUEST=$("$JQ_BIN" -cn --arg owner "$A_OWNER" --arg holding "$A_COLLATERAL" \
  '{ownerId:$owner,positionNonce:"0",userCollateralHoldingId:$holding,amount:"1000"}')
module_tx stablecoin_module withdrawCollateral "$(request_arg "$REQUEST")"
read_health 'Account A' "$A_OWNER"
read_holding 'Account A collateral' "$A_COLLATERAL"
REQUEST=$("$JQ_BIN" -cn --arg owner "$A_OWNER" '{ownerId:$owner,positionNonce:"0"}')
module_tx stablecoin_module closePosition "$(request_arg "$REQUEST")"
run_cli 'Account A closed-position read' call stablecoin_module positionHealth "$(request_arg "$REQUEST")"
require_cli_ok
printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.result.error == "invalid_position_data"' >/dev/null \
  || die "Account A position was not confirmed closed; see $OUTPUT_FILE"

step 'Step 10 — Account B: withdraw collateral and close'
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" --arg holding "$B_COLLATERAL" \
  '{ownerId:$owner,positionNonce:"0",userCollateralHoldingId:$holding,amount:"2000"}')
module_tx stablecoin_module withdrawCollateral "$(request_arg "$REQUEST")"
read_health 'Account B' "$B_OWNER"
read_holding 'Account B collateral' "$B_COLLATERAL"
REQUEST=$("$JQ_BIN" -cn --arg owner "$B_OWNER" '{ownerId:$owner,positionNonce:"0"}')
module_tx stablecoin_module closePosition "$(request_arg "$REQUEST")"
run_cli 'Account B closed-position read' call stablecoin_module positionHealth "$(request_arg "$REQUEST")"
require_cli_ok
printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -e '.result.error == "invalid_position_data"' >/dev/null \
  || die "Account B position was not confirmed closed; see $OUTPUT_FILE"

step 'Step 11 — Return collateral to admin and verify final state'
module_tx token_module transfer "$(string_arg "$A_COLLATERAL")" "$(string_arg "$COLLATERAL_HOLDING_ID")" "$(string_arg 1000)"
read_holding 'Account A collateral' "$A_COLLATERAL"
read_holding Admin "$COLLATERAL_HOLDING_ID"
module_tx token_module transfer "$(string_arg "$B_COLLATERAL")" "$(string_arg "$COLLATERAL_HOLDING_ID")" "$(string_arg 2000)"
read_holding 'Account B collateral' "$B_COLLATERAL"
read_holding Admin "$COLLATERAL_HOLDING_ID"
read_holding 'Account A stablecoin' "$A_STABLE"
FINAL_A_STABLE_RAW=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.holding.balanceRaw')
read_holding 'Account B stablecoin' "$B_STABLE"
FINAL_B_STABLE_RAW=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.holding.balanceRaw')
read_supply
FINAL_SUPPLY_RAW=$SUPPLY_RAW
read_holding Admin "$COLLATERAL_HOLDING_ID"
FINAL_ADMIN_COLLATERAL_RAW=$(printf '%s' "$LAST_STDOUT" | "$JQ_BIN" -er '.result.holding.balanceRaw')
[[ "$FINAL_A_STABLE_RAW" == 0 && "$FINAL_B_STABLE_RAW" == 0 ]] \
  || die "user stablecoin holdings were not fully settled"
[[ "$FINAL_ADMIN_COLLATERAL_RAW" == "$INITIAL_COLLATERAL_RAW" ]] \
  || die "admin collateral balance did not return to its initial value"
[[ "$FINAL_SUPPLY_RAW" == "$INITIAL_SUPPLY_RAW" ]] \
  || die "stablecoin supply did not return to its initial value"
[[ "$FINAL_SUPPLY_RAW" == 0 ]] \
  || printf 'note: stablecoin supply had pre-existing balance %s; this run restored it\n' "$FINAL_SUPPLY_RAW"

printf '\n## Result\n\nPASS — Account A and Account B positions closed; collateral and stablecoin supply returned to initial values.\n' >> "$OUTPUT_FILE"
printf '\nPASS. Transcript: %s\n' "$OUTPUT_FILE"
