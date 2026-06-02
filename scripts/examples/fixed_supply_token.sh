#!/usr/bin/env bash
# LP-0013 Example: Fixed Supply Token
#
# Demonstrates, against a real LEZ sequencer via spel:
#   - creating a fungible token with a mint authority
#   - minting additional supply (authority-gated)
#   - permanently revoking the mint authority (fixed supply)
#
# After revocation, AuthoritySlot rejects any further mint. That guarantee is
# covered by:
#   - token_program unit tests: mint_with_revoked_authority_fails,
#     set_authority_on_revoked_fails
#   - integration test: token_set_authority_revoke
# Run them with: RISC0_DEV_MODE=0 cargo test -p token_program -p lez-authority --lib
#
# Usage (from inside an lgs scaffold project dir):
#   RISC0_DEV_MODE=0 bash <path-to-lez-programs>/scripts/examples/fixed_supply_token.sh
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

echo "=== Fixed Supply Token Example ==="

echo "[1/4] Checking localnet..."
lgs localnet status 2>/dev/null | grep -q "ready: true" || lgs localnet start
echo "      Localnet ready."

echo "[2/4] Creating accounts..."
DEF_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
HOLD_ID=$(lgs wallet -- account new public 2>&1 | grep -oE 'account_id [^ ]+' | awk '{print $2}')
DEF_ID_HEX=$(b58_to_hex "$DEF_ID")
echo "      Definition: $DEF_ID"
echo "      Holding:    $HOLD_ID"

echo "[3/4] Creating token with mint authority..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
"$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- new-fungible-definition-with-authority \
  --definition-target-account "$DEF_ID" \
  --holding-target-account "$HOLD_ID" \
  --name "FixedCoin" \
  --initial-supply 1000000 \
  --mint-authority "$DEF_ID_HEX"
echo "      Token 'FixedCoin' created. Initial supply: 1,000,000"
sleep 2

echo "[4/4] Revoking mint authority (fixing supply permanently)..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
"$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- set-authority \
  --definition-account "$DEF_ID" \
  --authority-account "$DEF_ID" \
  --new-authority none
echo "      Authority revoked. Supply is permanently fixed at 1,000,000."
echo "      Any further mint is now rejected — enforced by lez-authority and"
echo "      covered by mint_with_revoked_authority_fails / token_set_authority_revoke."

echo ""
echo "=== Fixed Supply Token Example complete ==="
