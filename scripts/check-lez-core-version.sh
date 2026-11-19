#!/usr/bin/env bash
# Checks that every module and app builds against one lez_core rev, and that each
# metadata.json declares the version that rev's own metadata.json declares. The builder
# ignores dependency versions, so nothing else catches a rev bump that leaves them stale.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

repo="logos-blockchain/logos-execution-zone-module"

# Every rev pinned for lez_core: flake inputs, their locks, and the Basecamp runner.
revs="$(
  {
    grep -ohE "${repo}\?rev=[0-9a-f]{40}" flake.nix apps/*/flake.nix modules/*/flake.nix \
      apps/*/tests/*.sh | sed 's/.*rev=//'
    python3 - flake.lock apps/*/flake.lock modules/*/flake.lock <<'EOF'
import json, sys
for path in sys.argv[1:]:
    for node in json.load(open(path))["nodes"].values():
        locked = node.get("locked", {})
        if f"{locked.get('owner')}/{locked.get('repo')}" == "logos-blockchain/logos-execution-zone-module":
            print(locked["rev"])
EOF
  } | sort -u
)"
if [[ -z "${revs}" ]]; then
  echo "No lez_core rev found in the flakes" >&2
  exit 1
fi
if [[ "$(wc -l <<<"${revs}")" -ne 1 ]]; then
  printf 'lez_core is pinned at more than one rev:\n%s\n' "${revs}" >&2
  grep -nE "${repo}\?rev=" flake.nix apps/*/flake.nix modules/*/flake.nix apps/*/tests/*.sh >&2 || true
  exit 1
fi
rev="${revs}"

source_path="$(nix flake prefetch --json "github:${repo}?rev=${rev}" \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["storePath"])')"

python3 - "${source_path}/metadata.json" apps/*/metadata.json modules/*/metadata.json <<EOF
import json, sys

expected = json.load(open(sys.argv[1]))["version"]
stale = []
for path in sys.argv[2:]:
    for dependency in json.load(open(path)).get("dependencies", []):
        if dependency == "lez_core":
            stale.append(f"{path}: missing lez_core version")
        elif isinstance(dependency, dict) and dependency.get("name") == "lez_core":
            if dependency.get("version") != expected:
                stale.append(f"{path}: {dependency.get('version')}")
if stale:
    print(f"lez_core at ${rev} is version {expected}, but:", file=sys.stderr)
    print("\n".join(stale), file=sys.stderr)
    sys.exit(1)
print(f"lez_core ${rev}: every metadata.json declares {expected}")
EOF
