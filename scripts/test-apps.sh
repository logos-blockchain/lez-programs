#!/usr/bin/env bash
# Runs the app test suites: shared wallet C++ and QML, AMM C++ and QML, token QML.
# Qt comes from this flake's nixpkgs pin. The AMM QML tests also need the Logos
# design system: set LOGOS_DESIGN_SYSTEM to a built package to skip building it.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_dir="${APP_TEST_BUILD_DIR:-${repo_root}/target/app-tests}"
design_system_rev="6176f0d7a5dfeb64a7f0f98e7ca2bf71a4804772"

if [[ "${1:-}" != "--in-shell" ]]; then
  design_system="${LOGOS_DESIGN_SYSTEM:-$(nix build --no-link --print-out-paths \
    "github:logos-co/logos-design-system/${design_system_rev}")}"
  exec nix shell --inputs-from "${repo_root}" \
    nixpkgs#qt6.qtbase nixpkgs#qt6.qtdeclarative nixpkgs#qt6.qtsvg \
    nixpkgs#qt6.qtremoteobjects nixpkgs#cmake nixpkgs#ninja \
    -c env LOGOS_DESIGN_SYSTEM="${design_system}" "$0" --in-shell
fi

cd "${repo_root}"
qt_qml="$(dirname "$(dirname "$(command -v qmltestrunner)")")/lib/qt-6/qml"
wallet_qml="${build_dir}/wallet/qml"
export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QT_QUICK_CONTROLS_STYLE=Basic
export QML_IMPORT_PATH="${wallet_qml}:${qt_qml}" QML2_IMPORT_PATH="${wallet_qml}:${qt_qml}"

cmake -S apps/shared/wallet -B "${build_dir}/wallet" -G Ninja \
  -DLOGOS_WALLET_BUILD_ACCESS=OFF -DLOGOS_WALLET_BUILD_QML=ON -DBUILD_TESTING=ON
cmake --build "${build_dir}/wallet"
cmake -S apps/amm/tests/cpp -B "${build_dir}/amm" -G Ninja
cmake --build "${build_dir}/amm"

failed=0
run() {
  local name="$1"
  shift
  echo "=== ${name}"
  if ! "$@"; then
    echo "FAILED: ${name}"
    failed=1
  fi
}

# Run the wallet binaries directly: their ctest environment replaces the QML
# import path set above, which drops Qt Quick Controls.
run "wallet C++" "${build_dir}/wallet/logos_wallet_access_test"
run "wallet QML" "${build_dir}/wallet/logos_wallet_qml_test"
run "wallet AMM QML" "${build_dir}/wallet/logos_wallet_amm_qml_test"
run "AMM C++" ctest --test-dir "${build_dir}/amm" --output-on-failure
run "AMM QML" qmltestrunner -import "${qt_qml}" -import "${LOGOS_DESIGN_SYSTEM}/lib" \
  -import "${wallet_qml}" -input apps/amm/tests/qml
run "token QML" qmltestrunner -import "${qt_qml}" -input apps/token/tests/qml

exit "${failed}"
