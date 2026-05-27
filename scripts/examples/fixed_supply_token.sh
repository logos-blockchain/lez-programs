#!/usr/bin/env bash
# LP-0013 Example 1: Fixed Supply Token
# Creates a token, mints initial supply, then permanently revokes mint authority.
# After revocation, any further minting is rejected.
set -euo pipefail

echo "=== Fixed Supply Token Example ==="
echo ""

# 1. Start localnet if not running
echo "[1/6] Checking localnet..."
lgs localnet status --json 2>/dev/null | grep -q '"running":true' || lgs localnet start
echo "      Localnet ready."

# 2. Create definition and holding accounts
echo "[2/6] Creating accounts..."
DEF_ID=$(lgs wallet -- account new --public | grep "account_id" | awk '{print $2}')
HOLD_ID=$(lgs wallet -- account new --public | grep "account_id" | awk '{print $2}')
echo "      Definition: $DEF_ID"
echo "      Holding:    $HOLD_ID"

# 3. Create token WITH mint authority (so we can mint more later)
echo "[3/6] Creating token with mint authority..."
lgs wallet -- token new-with-authority \
    --definition "$DEF_ID" \
    --holding "$HOLD_ID" \
    --name "FixedCoin" \
    --initial-supply 1000000 \
    --mint-authority "$(lgs wallet -- account default)"
echo "      Token created. Initial supply: 1,000,000"

# 4. Mint additional tokens
echo "[4/6] Minting 500,000 additional tokens..."
MINT_HOLD_ID=$(lgs wallet -- account new --public | grep "account_id" | awk '{print $2}')
lgs wallet -- token mint \
    --definition "$DEF_ID" \
    --holding "$MINT_HOLD_ID" \
    --amount 500000
echo "      Minted. Total supply: 1,500,000"

# 5. Revoke mint authority (fix the supply permanently)
echo "[5/6] Revoking mint authority (fixing supply permanently)..."
lgs wallet -- token set-authority \
    --definition "$DEF_ID" \
    --new-authority none
echo "      Authority revoked. Supply is now permanently fixed."

# 6. Verify: minting now fails
echo "[6/6] Verifying minting is rejected after revocation..."
EXTRA_HOLD=$(lgs wallet -- account new --public | grep "account_id" | awk '{print $2}')
if lgs wallet -- token mint \
    --definition "$DEF_ID" \
    --holding "$EXTRA_HOLD" \
    --amount 1 2>&1 | grep -q "revoked\|fixed supply"; then
    echo "      ✓ Minting correctly rejected: authority revoked"
else
    echo "      ✗ FAIL: Expected rejection after authority revocation"
    exit 1
fi

echo ""
echo "=== Fixed Supply Token Example PASSED ==="
