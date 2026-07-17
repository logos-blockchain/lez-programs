#!/usr/bin/env bash

# Verify the host requirements for a supported native macOS AMM build.

set -euo pipefail

failures=0

pass() {
  printf 'ok: %s\n' "$1"
}

fail() {
  printf 'missing: %s\n' "$1" >&2
  failures=$((failures + 1))
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  printf 'missing: this script must run on macOS\n' >&2
  exit 1
fi

architecture="$(uname -m)"
case "$architecture" in
  arm64|x86_64)
    pass "supported architecture $architecture"
    ;;
  *)
    fail "unsupported architecture $architecture"
    ;;
esac

developer_dir="$(xcode-select -p 2>/dev/null || true)"
if [[ "$developer_dir" == *.app/Contents/Developer ]]; then
  pass "full Xcode selected at $developer_dir"
  required_gib=40
else
  fail "full Xcode is not selected (current developer directory: ${developer_dir:-none})"
  required_gib=60
fi

if xcode_version="$(xcodebuild -version 2>/dev/null)"; then
  pass "$(printf '%s' "$xcode_version" | tr '\n' ' ')"
else
  fail "xcodebuild is unavailable; install Xcode.app and select it with xcode-select"
fi

if xcodebuild -checkFirstLaunchStatus >/dev/null 2>&1; then
  pass "Xcode first-launch setup and licence are complete"
else
  fail "Xcode setup is incomplete; run sudo xcodebuild -license accept and -runFirstLaunch"
fi

for tool in metal metallib; do
  if tool_path="$(xcrun --sdk macosx --find "$tool" 2>/dev/null)" &&
     [[ -x "$tool_path" ]]; then
    pass "$tool at $tool_path"
  else
    fail "$tool is unavailable; run xcodebuild -downloadComponent MetalToolchain"
  fi
done

nix_bin="$(command -v nix 2>/dev/null || true)"
if [[ -z "$nix_bin" && -x /nix/var/nix/profiles/default/bin/nix ]]; then
  nix_bin=/nix/var/nix/profiles/default/bin/nix
fi

if [[ -n "$nix_bin" ]]; then
  pass "Nix at $nix_bin ($("$nix_bin" --version))"

  configured_features="$(
    "$nix_bin" config show experimental-features 2>/dev/null ||
      "$nix_bin" show-config 2>/dev/null |
        awk -F ' = ' '$1 == "experimental-features" { print $2 }'
  )"
  if [[ " $configured_features " == *" nix-command "* &&
        " $configured_features " == *" flakes "* ]]; then
    pass "Nix flakes and nix-command are enabled"
  else
    fail "enable experimental-features = nix-command flakes in nix.conf"
  fi

  if "$nix_bin" --extra-experimental-features 'nix-command flakes' \
      store info >/dev/null 2>&1; then
    pass "Nix daemon and store are available"
  else
    fail "Nix cannot contact its daemon or store"
  fi

  sandbox_setting="$("$nix_bin" config show sandbox 2>/dev/null || true)"
  if [[ "$sandbox_setting" == "false" ]]; then
    pass "Nix macOS sandbox is disabled for the host Metal toolchain"
  else
    fail "Nix sandbox must be false for the host Metal toolchain (current: ${sandbox_setting:-unknown})"
  fi
else
  fail "Nix is unavailable; install multi-user Nix and start its daemon"
fi

if [[ -d /nix ]]; then
  available_kib="$(df -Pk /nix | awk 'NR == 2 { print $4 }')"
  required_kib=$((required_gib * 1024 * 1024))
  if [[ "${available_kib:-0}" -ge "$required_kib" ]]; then
    pass "at least ${required_gib} GiB free on the Nix volume"
  else
    fail "less than ${required_gib} GiB free on the Nix volume"
  fi
else
  fail "/nix is not mounted"
fi

if (( failures > 0 )); then
  printf '\nSee MACOS.md for installation and recovery instructions.\n' >&2
  exit 1
fi

printf '\nAll macOS AMM prerequisites are available.\n'
