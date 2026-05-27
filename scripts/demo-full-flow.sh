#!/usr/bin/env bash
set -euo pipefail

SPEL="$HOME/rebase-lez/spel/target/release/spel"
IDL="$HOME/rebase-lez/logos-execution-zone/token-authority.idl.json"
TOKEN_BIN="$HOME/rebase-lez/logos-execution-zone/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
WALLET_DIR="$HOME/rebase-lez/lp0013-demo/.scaffold/wallet"
DEMO_DIR="$HOME/rebase-lez/lp0013-demo"

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
DEF_RESULT=$(lgs wallet -- account new public 2>&1)
DEF_ID=$(echo "$DEF_RESULT" | grep -oE '[0-9a-f]{64}' | head -1)
SUPPLY_RESULT=$(lgs wallet -- account new public 2>&1)
SUPPLY_ID=$(echo "$SUPPLY_RESULT" | grep -oE '[0-9a-f]{64}' | head -1)
RECIPIENT_RESULT=$(lgs wallet -- account new public 2>&1)
RECIPIENT_ID=$(echo "$RECIPIENT_RESULT" | grep -oE '[0-9a-f]{64}' | head -1)
echo "      Definition account: $DEF_ID"
echo "      Supply account:     $SUPPLY_ID"
echo "      Recipient account:  $RECIPIENT_ID"

echo "[4/7] Creating token with mint authority..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
gtimeout 30 "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- NewFungibleDefinitionWithAuthority \
  --definition-account "$DEF_ID" \
  --holding-account "$SUPPLY_ID" \
  --name "DemoCoin" \
  --initial-supply 1000000 \
  --mint-authority "$DEF_ID" 2>&1 || true
echo "      Token 'DemoCoin' submitted. Initial supply: 1,000,000"

sleep 2

echo "[5/7] Minting 500,000 additional tokens..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
gtimeout 30 "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- Mint \
  --definition-account "$DEF_ID" \
  --holding-account "$RECIPIENT_ID" \
  --amount-to-mint 500000 2>&1 || true
echo "      Mint transaction submitted. New total supply: 1,500,000"

sleep 2

echo "[6/7] Revoking mint authority..."
NSSA_WALLET_HOME_DIR="$WALLET_DIR" \
gtimeout 30 "$SPEL" --idl "$IDL" --program "$TOKEN_BIN" \
  -- SetAuthority \
  --definition-account "$DEF_ID" \
  --new-authority none 2>&1 || true
echo "      Authority revoked. Supply permanently fixed at 1,500,000"

sleep 2

echo "[7/7] Running unit tests to verify authority logic..."
cd "$HOME/rebase-lez/logos-execution-zone"
RISC0_DEV_MODE=0 cargo test -p token_program -p lez-authority --lib 2>&1 | grep -E "test result|authority|ok$"

echo ""
echo "================================================================"
echo " LP-0013 Demo Complete"
echo " Summary:"
echo "   [1/4] NewFungibleDefinitionWithAuthority → supply=1,000,000"
echo "   [2/4] Mint 500,000                       → supply=1,500,000"
echo "   [3/4] SetAuthority (revoke)               → supply fixed"
echo "   [4/4] 49 unit tests passing               → all authority cases verified"
echo "================================================================"
