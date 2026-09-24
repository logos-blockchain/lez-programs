#!/usr/bin/env bash
# Bootstrap stablecoin using the current wallet. See --help for overrides.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="${REPO_ROOT:-$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null || (cd "$script_dir/../../../.." && pwd -P))}"

if [[ -f "$repo_root/scripts/workspace-env.sh" ]]; then
    # shellcheck disable=SC1091
    source "$repo_root/scripts/workspace-env.sh"
fi

exec python3 "$repo_root/scripts/deploy_stablecoin.py" "$@"
