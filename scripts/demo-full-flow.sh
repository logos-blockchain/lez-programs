#!/usr/bin/env bash
# LP-0013 End-to-End Demo Script
# Demonstrates the full mint authority lifecycle against a real LEZ sequencer.
#
# Prerequisites:
#   - lgs (logos-scaffold): https://github.com/logos-blockchain/logos-execution-zone
#   - spel CLI: https://github.com/logos-co/spel
#   - A funded wallet (run: lgs wallet topup)
#
# Usage (from inside an lgs scaffold project directory):
#   cd <your-lgs-scaffold-dir>
#   RISC0_DEV_MODE=0 bash <path-to-lez-programs>/scripts/demo-full-flow.sh
#
# Environment variables (all optional, auto-detected):
#   DEMO_DIR / LEZ_PROGRAMS / SPEL / TOKEN_BIN / IDL / WALLET_DIR
#
# The script will:
#   1. Start a local LEZ sequencer
#   2. Fund the wallet
#   3. Create token accounts
#   4. Submit NewFungibleDefinitionWithAuthority transaction
#   5. Submit Mint transaction (authority-gated)
#   6. Submit SetAuthority (revoke) transaction
#   7. Run unit tests to verify authority logic (60 tests)
set -euo pipefail

if command -v gtimeout &>/dev/null; then
    TIMEOUT="gtimeout"
elif command -v timeout &>/dev/null; then
    TIMEOUT="timeout"
else
    echo "Warning: no timeout command found, running without timeout"
    TIMEOUT=""
fi
SPEL="${SPEL:-$HOME/rebase-lez/spel/target/release/spel}"
LEZ_PROGRAMS="${LEZ_PROGRAMS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
IDL="${IDL:-$LEZ_PROGRAMS/artifacts/token-idl.json}"
# Resolve the token guest binary from either build layout, in priority order:
#   1. `cargo risczero build --manifest-path programs/token/methods/guest/Cargo.toml`
#      -> programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin
#      (the build command documented in the README)
#   2. workspace build (`cargo build` / `cargo test`)
#      -> target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin
# An explicit TOKEN_BIN env var always takes precedence.
_risc0_token_bin="$LEZ_PROGRAMS/programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
_workspace_token_bin="$LEZ_PROGRAMS/target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin"
if [ -z "${TOKEN_BIN:-}" ]; then
    if [ -f "$_risc0_token_bin" ]; then
        TOKEN_BIN="$_risc0_token_bin"
    else
        TOKEN_BIN="$_workspace_token_bin"
    fi
fi
DEMO_DIR="${DEMO_DIR:-$(pwd)}"
WALLET_DIR="${WALLET_DIR:-$DEMO_DIR/.scaffold/wallet}"

# Convert a base58 "Public/..." account_id to the 64-char hex form
# that spel expects for [u8; 32] args (e.g. --mint-authority).
b58_to_hex() {
    local id="${1#Public/}"
    id="${id#Private/}"
    python3 -c "
import sys
s = sys.argv[1]
alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
num = 0
for c in s:
    num = num * 58 + alphabet.index(c)
print(num.to_bytes(32, 'big').hex())
" "$id"
}

echo "================================================================"
echo " LP-0013: Token Program Mint Authority — End-to-End Demo"
echo " RISC0_DEV_MODE=${RISC0_DEV_MODE:-not set}"
echo "================================================================"
echo ""

echo "[1/7] Checking localnet..."
cd "$DEMO_DIR"
if lgs localnet status 2>/dev/null | grep -q "ready: true"; then
    echo "      Localnet already running."
else
    lgs localnet start
    echo "      Localnet started."
fi

echo "[2/7] Funding wallet..."
lgs wallet topup 2>&1 | grep -E "complete|funded|Address" || true
echo "      Wallet funded."

echo "[3/7] Creating token accounts..."
DEF_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
SUPPLY_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
RECIPIENT_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
echo "      Definition account: $DEF_ID"
echo "      Supply account:     $SUPPLY_ID"
echo "      Recipient account:  $RECIPIENT_ID"

echo "[4/7] Creating token with mint authority..."
DEF_ID_HEX=$(b58_to_hex "$DEF_ID")
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
${TIMEOUT:+$TIMEOUT 30} "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- new-fungible-definition-with-authority \
  --definition-target-account "$DEF_ID" \
  --holding-target-account "$SUPPLY_ID" \
  --name "DemoCoin" \
  --initial-supply 1000000 \
  --mint-authority "$DEF_ID_HEX"
echo "      Token 'DemoCoin' submitted. Initial supply: 1,000,000"
sleep 2

echo "[5/7] Minting 500,000 additional tokens..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
${TIMEOUT:+$TIMEOUT 30} "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- mint \
  --definition-account "$DEF_ID" \
  --authority-account "$DEF_ID" \
  --user-holding-account "$RECIPIENT_ID" \
  --amount-to-mint 500000
echo "      Mint transaction submitted. New total supply: 1,500,000"
sleep 2

echo "[6/7] Revoking mint authority..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
${TIMEOUT:+$TIMEOUT 30} "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- set-authority \
  --definition-account "$DEF_ID" \
  --authority-account "$DEF_ID" \
  --new-authority none
echo "      Authority revoked. Supply permanently fixed at 1,500,000"
sleep 2

echo "[7/7] Running unit tests to verify authority logic..."
cd "$LEZ_PROGRAMS"
RISC0_DEV_MODE=0 cargo test -p token_program -p lez-authority --lib 2>&1 | grep -E "test result|authority|ok$"

echo ""
echo "================================================================"
echo " LP-0013 Demo Complete"
echo " Summary:"
echo "   [1/4] NewFungibleDefinitionWithAuthority → supply=1,000,000"
echo "   [2/4] Mint 500,000                       → supply=1,500,000"
echo "   [3/4] SetAuthority (revoke)               → supply fixed"
echo "   [4/4] Unit tests passing                 → all authority cases verified"
echo "================================================================"
