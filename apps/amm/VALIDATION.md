# AMM UI Validation

Validation for the New Position flow covers the shipped Rust client, QML
syntax, and the complete module build.

## Commands

Run from the repository root:

```bash
cargo +1.94.0 test -p amm_client
cargo +1.94.0 clippy -p amm_client --all-targets -- -D warnings
logos_qml=$(nix build github:logos-co/logos-design-system/6176f0d7a5dfeb64a7f0f98e7ca2bf71a4804772 --no-link --print-out-paths)
amm_qml=$(nix build ./apps/amm#packages.x86_64-linux.default --no-link --print-out-paths)
qt_qml=$(nix-store --query --requisites "$amm_qml" | rg -m1 -- '-qtdeclarative-[0-9]')
qt_svg=$(nix-store --query --requisites "$amm_qml" | rg -m1 -- '-qtsvg-[0-9]')
export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_PLUGIN_PATH="$qt_svg/lib/qt-6/plugins"
amm_import="$amm_qml/lib/qml"
test -f "$amm_import/Logos/Wallet/qmldir"
"$qt_qml/bin/qmltestrunner" -import "$qt_qml/lib/qt-6/qml" -import "$logos_qml/lib" -import "$amm_import" -input apps/shared/wallet/tests/qml
"$qt_qml/bin/qmltestrunner" -import "$qt_qml/lib/qt-6/qml" -import "$logos_qml/lib" -import "$amm_import" -input apps/amm/tests/qml
"$qt_qml/bin/qmllint" -I "$qt_qml/lib/qt-6/qml" -I "$logos_qml/lib" -I "$amm_qml/lib" -I "$amm_import" apps/amm/qml/pages/LiquidityPage.qml apps/amm/qml/components/liquidity/NewPositionForm.qml apps/amm/qml/components/liquidity/LiquidityConfirmationSummary.qml
```

When shared AMM program types or instruction signatures change, also run the
affected Rust checks:

```bash
RISC0_DEV_MODE=1 cargo +1.94.0 test -p amm_program
cargo +1.94.0 build -p idl-gen --release
./target/release/idl-gen programs/amm/methods/guest/src/bin/amm.rs > /tmp/amm-idl.json
diff artifacts/amm-idl.json /tmp/amm-idl.json
```

Live wallet/sequencer validation uses the configured Network and an External
Wallet Provider. Open the AMM UI, select two TokenProgram-scoped fungible token
definitions, preview an active or missing Pool, submit, and verify the returned
transaction ID is displayed.

## Acceptance Checklist

- Context exposes Wallet-Scoped Holdings and configured token definitions.
- Unsupported fee tiers are disabled and explain why.
- Active pool fee tier is fixed to the stored pool fee.
- Missing pool flow accepts an editable `X Token A = Y Token B` ratio and
  scales either deposit from the minimum that mints more than
  `MINIMUM_LIQUIDITY`.
- Active Pool flow keeps the reserve ratio and previews expected LP output.
- `new_definition` and `add_liquidity` account lists match the committed AMM IDL
  order.
- Submit re-quotes and surfaces `quote_changed` when the request no longer
  matches the preview hash.
