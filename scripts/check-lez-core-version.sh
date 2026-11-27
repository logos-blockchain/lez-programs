#!/usr/bin/env bash
# Check declarations and locked wallet versions on both sides of the AMM actor migration.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
import json
import re
import subprocess
from pathlib import Path

repo = "logos-blockchain/logos-execution-zone-module"
pattern = re.compile(rf"{repo}\?rev=([0-9a-f]{{40}})")


def locked_inputs(directory):
    lock = json.loads((directory / "flake.lock").read_text())
    nodes = lock["nodes"]
    return nodes, nodes[lock["root"]]["inputs"]


def wallet_pin(directory, name):
    manifest = (directory / "flake.nix").read_text()
    match = re.search(rf'^\s*{name}\.url = "github:{pattern.pattern}";', manifest, re.M)
    nodes, inputs = locked_inputs(directory)
    locked = nodes[inputs[name]]["locked"]
    if not match or f"{locked['owner']}/{locked['repo']}" != repo or match[1] != locked["rev"]:
        raise SystemExit(f"{directory}: {name} declaration and flake.lock must agree")
    return locked["rev"]


root = Path(".")
wallet = wallet_pin(root, "lez_core")
amm_wallet = wallet_pin(root, "amm_lez_core")
versions = {}
for rev in {wallet, amm_wallet}:
    source = json.loads(subprocess.check_output(["nix", "flake", "prefetch", "--json",
                                                 f"github:{repo}?rev={rev}"]))["storePath"]
    metadata = subprocess.check_output(["nix", "store", "cat", f"{source}/metadata.json"])
    versions[rev] = json.loads(metadata)["version"]

directories = [p.parent for group in ("apps", "modules") for p in Path(group).glob("*/flake.nix")]
for directory in [root, *directories]:
    nodes, _ = locked_inputs(directory)
    for node in nodes.values():
        locked = node.get("locked", {})
        if f"{locked.get('owner')}/{locked.get('repo')}" == repo and locked["rev"] not in versions:
            raise SystemExit(f"{directory}/flake.lock: unexpected transitive wallet {locked['rev']}")
    if directory == root:
        continue
    expected = amm_wallet if directory.name == "amm" else wallet
    if wallet_pin(directory, "lez_core") != expected:
        raise SystemExit(f"{directory}: expected wallet revision {expected}")
    for runner in directory.glob("tests/*.sh"):
        if any(rev != expected for rev in pattern.findall(runner.read_text())):
            raise SystemExit(f"{runner}: expected wallet revision {expected}")
    dependencies = json.loads((directory / "metadata.json").read_text())["dependencies"]
    for dependency in dependencies:
        if dependency == "lez_core" or (
            isinstance(dependency, dict) and dependency.get("name") == "lez_core"
            and dependency.get("version") != versions[expected]
        ):
            raise SystemExit(f"{directory}/metadata.json: expected lez_core {versions[expected]}")
print("Wallet revisions and declared dependency versions match.")
PY
