#!/usr/bin/env bash
# Source from the repository root (or any checkout) before running workspace tools:
#   source scripts/workspace-env.sh

workspace_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

# The current wallet CLI and the deploy script use this variable. Keep any
# caller-provided wallet home unchanged.
export LEE_WALLET_HOME_DIR="${LEE_WALLET_HOME_DIR:-$HOME/.lee/wallet}"

# Move this checkout to the front, even if it was already present later.
# Do not export *_BIN: those variables belong to caller overrides and otherwise
# retain the previous checkout when switching between worktrees.
workspace_path=":$PATH:"
workspace_path="${workspace_path//:$workspace_root\/target\/debug:/:}"
workspace_path="${workspace_path#:}"
workspace_path="${workspace_path%:}"
export PATH="$workspace_root/target/debug:$workspace_path"

unset workspace_root workspace_path
