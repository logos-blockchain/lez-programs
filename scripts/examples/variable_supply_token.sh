#!/usr/bin/env bash
# LP-0013 Example: Variable Supply Token with Authority Rotation
#
# Demonstrates, against a real LEZ sequencer via spel:
#   - creating a fungible token with a mint authority
#   - minting additional supply (authority-gated)
#   - rotating the mint authority to a new key
#
# The guarantee that the OLD authority can no longer mint after rotation —
# and that only the current authority's signer is accepted — is enforced by
# lez-authority's AuthoritySlot and covered by:
#   - token_program unit tests: set_authority_rotate_then_old_cannot_mint,
#     mint_with_wrong_signer_fails, set_authority_wrong_signer_fails
#   - integration test: token_set_authority_revoke
# Run them with: RISC0_DEV_MODE=0 cargo test -p token_program -p lez-authority --lib
#
# Usage (from inside an lgs scaffold project dir):
#   RISC0_DEV_MODE=0 bash <path-to-lez-programs>/scripts/examples/variable_supply_token.sh
set -euo pipefail

SPEL="${SPEL:-$HOME/rebase-lez/spel/target/release/spel}"
LEZ_PROGRAMS="${LEZ_PROGRAMS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
IDL="${IDL:-$LEZ_PROGRAMS/artifacts/token-idl.json}"
TOKEN_BIN="${TOKEN_BIN:-$LEZ_PROGRAMS/target/riscv-guest/token-methods/token-guest/riscv32im-risc0-zkvm-elf/release/token.bin}"
WALLET_DIR="${WALLET_DIR:-$(pwd)/.scaffold/wallet}"

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

echo "=== Variable Supply Token (Authority Rotation) Example ==="

echo "[1/5] Checking localnet..."
lgs localnet status 2>/dev/null | grep -q "ready: true" || lgs localnet start
echo "      Localnet ready."

echo "[2/5] Creating accounts..."
DEF_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
HOLD_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
NEW_AUTH_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
DEF_ID_HEX=$(b58_to_hex "$DEF_ID")
NEW_AUTH_HEX=$(b58_to_hex "$NEW_AUTH_ID")
echo "      Definition: $DEF_ID"
echo "      New authority (rotation target): $NEW_AUTH_ID"

echo "[3/5] Creating token with mint authority (the definition account)..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
"$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- new-fungible-definition \
  --definition-target-account "$DEF_ID" \
  --holding-target-account "$HOLD_ID" \
  --name "VarCoin" \
  --total-supply 100000 \
  --mint-authority "$DEF_ID_HEX"
echo "      Token 'VarCoin' created. Initial supply: 100,000"
sleep 2

echo "[4/5] Minting 50,000 more (authority-gated)..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
"$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- mint \
  --definition-account "$DEF_ID" \
  --authority-account "$DEF_ID" \
  --user-holding-account "$HOLD_ID" \
  --amount-to-mint 50000
echo "      Minted. Total supply: 150,000"
sleep 2

echo "[5/5] Rotating mint authority to a new key..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
"$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- set-authority \
  --definition-account "$DEF_ID" \
  --authority-account "$DEF_ID" \
  --new-authority "$NEW_AUTH_HEX"
echo "      Authority rotated to: $NEW_AUTH_ID"
echo "      After rotation, only the new authority's signer can mint —"
echo "      enforced by lez-authority and covered by unit/integration tests."

echo ""
echo "=== Variable Supply Token Example complete ==="
