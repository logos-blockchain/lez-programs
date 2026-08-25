#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd -P)"
output_dir="${TOKEN_E2E_OUTPUT:-${repo_root}/.3esmit/projects/lez-programs/docs/token-basecamp-e2e}"
run_root="${TOKEN_E2E_RUN_ROOT:-$(mktemp -d "${TMPDIR:-/tmp}/token-basecamp-e2e.XXXXXX")}"
inspector_port="${QML_INSPECTOR_PORT:-3768}"
basecamp_pid=""

cleanup() {
  local status=$?
  set +e
  if [[ -n "${basecamp_pid}" ]] && kill -0 "${basecamp_pid}" 2>/dev/null; then
    kill "${basecamp_pid}" 2>/dev/null
    wait "${basecamp_pid}" 2>/dev/null
  fi
  if (( status != 0 )); then
    printf 'Basecamp E2E run retained for diagnosis: %s\n' "${run_root}" >&2
    if [[ -f "${run_root}/basecamp.log" ]]; then
      tail -120 "${run_root}/basecamp.log" >&2
    fi
  else
    printf 'Basecamp E2E run: %s\n' "${run_root}"
  fi
  exit "${status}"
}
trap cleanup EXIT

mkdir -p "${output_dir}" "${run_root}/user/modules" "${run_root}/user/plugins"
wallet_home="${run_root}/wallet"
mkdir -p "${wallet_home}"

build_if_missing() {
  local override="$1"
  local output="$2"
  shift 2
  if [[ -n "${override}" ]]; then
    printf '%s\n' "${override}"
    return
  fi
  nix build "$@" -o "${output}"
  printf '%s\n' "${output}"
}

mcp_root="$(build_if_missing "${LOGOS_QT_MCP:-}" "${run_root}/result-mcp" .#test-framework)"
bundle_root="$(build_if_missing "${TOKEN_BASECAMP_BUNDLE:-}" "${run_root}/result-bundle" github:logos-co/logos-basecamp#bin-bundle-dir-inspector)"
wallet_install="$(build_if_missing "${TOKEN_WALLET_INSTALL:-}" "${run_root}/wallet-install" \
  'github:gravityblast/logos-execution-zone-module?ref=fix/generic-tx-instruction-bstr#install-portable' \
  --override-input logos-execution-zone \
  'github:logos-blockchain/logos-execution-zone?rev=415964d7f9043a1bfe28da8d0e8b3a6f64abb258')"
token_install="$(build_if_missing "${TOKEN_MODULE_INSTALL:-}" "${run_root}/token-install" .#install-portable)"
ui_install="$(build_if_missing "${TOKEN_UI_INSTALL:-}" "${run_root}/token-ui-install" .#token-ui-install-portable)"

cp -RL "${wallet_install}/modules/." "${run_root}/user/modules/"
cp -RL "${token_install}/modules/." "${run_root}/user/modules/"
cp -RL "${ui_install}/plugins/." "${run_root}/user/plugins/"

basecamp_bin="${bundle_root}/bin/LogosBasecamp"
if [[ ! -x "${basecamp_bin}" ]]; then
  printf 'Basecamp binary unavailable: %s\n' "${basecamp_bin}" >&2
  exit 2
fi

QT_QPA_PLATFORM=offscreen \
QT_FORCE_STDERR_LOGGING=1 \
QML_DISABLE_DISK_CACHE=1 \
QML_INSPECTOR_PORT="${inspector_port}" \
LEE_WALLET_HOME_DIR="${wallet_home}" \
LD_LIBRARY_PATH="${bundle_root}/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}" \
"${basecamp_bin}" --user-dir "${run_root}/user" -platform offscreen \
  >"${run_root}/basecamp.log" 2>&1 &
basecamp_pid=$!

inspector_ready=0
for _ in $(seq 1 120); do
  if (exec 3<>"/dev/tcp/127.0.0.1/${inspector_port}") 2>/dev/null; then
    exec 3>&- 3<&-
    inspector_ready=1
    break
  fi
  if ! kill -0 "${basecamp_pid}" 2>/dev/null; then
    printf 'Basecamp exited before inspector startup\n' >&2
    exit 1
  fi
  sleep 0.5
done

if (( inspector_ready != 1 )); then
  printf 'Inspector did not become ready on port %s\n' "${inspector_port}" >&2
  exit 1
fi

LOGOS_QT_MCP="${mcp_root}" \
QML_INSPECTOR_PORT="${inspector_port}" \
TOKEN_E2E_OUTPUT="${output_dir}" \
node "${repo_root}/apps/token/tests/token-definition.mjs"
