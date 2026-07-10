# AMM UI Validation

Validation for the New Position flow covers deterministic quote math, account
construction, QML checks, and the module build.

## Commands

Run from the repository root:

```bash
node apps/amm/tests/new-position-validation.mjs
qmllint apps/amm/qml/pages/LiquidityPage.qml apps/amm/qml/components/liquidity/NewPositionForm.qml apps/amm/qml/components/liquidity/NewPositionConfirmationDialog.qml
nix build ./apps/amm#packages.x86_64-linux.default --no-link
```

When shared AMM program types or instruction signatures change, also run the
affected Rust checks:

```bash
RISC0_DEV_MODE=1 cargo +1.94.0 test -p amm_program
cargo +1.94.0 build -p idl-gen --release
./target/release/idl-gen programs/amm/methods/guest/src/bin/amm.rs > /tmp/amm-idl.json
diff artifacts/amm-idl.json /tmp/amm-idl.json
```

Live wallet/sequencer validation is manual until transaction dispatch has real
token holding IDs. Use a local devnet wallet/sequencer when available, open the
AMM UI, create or adopt one active account, preview both the active LOGOS/USDC
pool and the missing USDC/WETH pool, then submit and verify either a successful
transaction or the deterministic `submit_unavailable` backend response.

## Acceptance Checklist

- Active wallet context exposes only holdings for the active account.
- Unsupported fee tiers are disabled and explain why.
- Active pool fee tier is fixed to the stored pool fee.
- Missing pool flow locks the user-selected price and scales from the minimum
  deposit that mints more than `MINIMUM_LIQUIDITY`.
- Active pool flow keeps the reserve ratio and previews LP with
  `floor(min(supply * amount_a / reserve_a, supply * amount_b / reserve_b))`.
- Decimal inference keeps small supplies at 0 decimals, medium supplies at 6,
  large supplies at 9, and massive supplies at 18.
- Human amount parse/format is reversible for 0, 6, 9, and 18 decimals.
- `new_definition` and `add_liquidity` account lists match the committed AMM IDL
  order.
- Submit re-quotes and surfaces `quote_changed` when the request no longer
  matches the preview hash.
