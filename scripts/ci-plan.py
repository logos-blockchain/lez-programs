#!/usr/bin/env python3
"""Decides which CI components run.

A component runs when a file under one of its paths changed (or a global file
changed, or on pushes to main) and it is not listed in .github/known-broken.txt.
Paths cover each component's own directory plus every in-repo crate it builds
against, so an upstream change reruns its consumers.

Usage:
  scripts/ci-plan.py --all              # every component not known broken
  scripts/ci-plan.py --base <git-ref>   # components touched since <ref>
  scripts/ci-plan.py --files <path>...  # components these paths touch

In GitHub Actions the plan is also written to $GITHUB_OUTPUT and the step summary.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
KNOWN_BROKEN = REPO / ".github" / "known-broken.txt"

# A change to any of these reruns everything.
GLOBAL = [
    ".github/",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".cargo/",
    "clippy.toml",
    "Makefile",
    "flake.nix",
    "flake.lock",
    "scripts/ci-plan.py",
]

PROGRAM_SHARED = ["tools/", "scripts/build-guests.sh", "scripts/build-guests.Dockerfile", ".dockerignore"]


def program(name, *deps):
    """deps: other program directories whose core crate this program uses."""
    methods = name.replace("_", "-") + "-methods"
    return {
        "kind": "program",
        "name": name,
        "crates": f"-p {name}_core -p {name}_program -p {methods}",
        "test_crates": f"-p {name}_core -p {name}_program",
        "paths": [f"programs/{name}/", f"artifacts/{name}-idl.json", *PROGRAM_SHARED]
        + [f"programs/{d}/core/" for d in deps],
    }


def module(name, *paths):
    return {
        "kind": "module",
        "name": name,
        "crate": f"{name}_ffi",
        "paths": [f"modules/{name}/", *paths],
    }


def app(name, *paths):
    return {"kind": "app", "name": name, "paths": [*paths, "scripts/test-apps.sh"]}


COMPONENTS = [
    program("token"),
    program("nft", "token"),
    program("twap_oracle"),
    program("amm", "token", "twap_oracle"),
    program("ata", "token", "nft"),
    program("stablecoin", "token", "twap_oracle"),
    program("token_mint_authority", "token"),
    module("amm", "programs/amm/core/", "programs/token/core/", "programs/twap_oracle/core/"),
    module("oracle", "programs/twap_oracle/core/"),
    # stablecoin_ffi also links token_program and stablecoin_program and tests
    # against the stablecoin guest.
    module("stablecoin", "programs/stablecoin/", "programs/token/", "programs/twap_oracle/core/"),
    module("token", "programs/token/core/"),
    app("wallet", "apps/shared/wallet/"),
    app("amm", "apps/amm/", "apps/shared/wallet/"),
    app("token", "apps/token/"),
    {
        "kind": "single",
        "name": "integration",
        "paths": ["programs/"],
    },
    {
        "kind": "single",
        "name": "benchmark",
        "paths": ["programs/benchmark/", "programs/twap_oracle/core/"],
    },
    {"kind": "single", "name": "tools", "paths": ["tools/"]},
    {
        "kind": "single",
        "name": "lez-core-version",
        "paths": [
            "apps/",
            "modules/",
            "scripts/check-lez-core-version.sh",
        ],
    },
]


def component_id(c):
    return c["name"] if c["kind"] == "single" else f"{c['kind']}:{c['name']}"


def read_known_broken():
    if not KNOWN_BROKEN.exists():
        return {}
    entries = {}
    for line in KNOWN_BROKEN.read_text().splitlines():
        entry, _, reason = line.partition("#")
        entry = entry.strip()
        if entry:
            entries[entry] = reason.strip()
    unknown = set(entries) - {component_id(c) for c in COMPONENTS}
    if unknown:
        valid = ", ".join(component_id(c) for c in COMPONENTS)
        sys.exit(f"{KNOWN_BROKEN.name}: unknown component(s) {sorted(unknown)}; valid: {valid}")
    return entries


def check_coverage():
    """Fails if a program or module directory has no component, so it can't go untested."""
    registered = {(c["kind"], c["name"]) for c in COMPONENTS}
    found = {("program", p.parent.parent.parent.name) for p in REPO.glob("programs/*/methods/guest/Cargo.toml")}
    # The integration component covers its test guests.
    found.discard(("program", "integration_tests"))
    found |={("module", p.parent.parent.name) for p in REPO.glob("modules/*/ffi/Cargo.toml")}
    missing = sorted(f"{kind}:{name}" for kind, name in found - registered)
    if missing:
        sys.exit(f"No CI component for {missing}; add it to COMPONENTS in scripts/ci-plan.py")


def changed_files(base):
    out = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...HEAD"],
        cwd=REPO,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [f for f in out.splitlines() if f]


def touches(files, paths):
    return any(f == p or (p.endswith("/") and f.startswith(p)) for f in files for p in paths)


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--all", action="store_true")
    group.add_argument("--base")
    group.add_argument("--files", nargs="+")
    args = parser.parse_args()

    check_coverage()
    known_broken = read_known_broken()
    files = None if args.all else args.files or changed_files(args.base)
    run_all = files is None or touches(files, GLOBAL)

    selected, skipped_broken = [], []
    for c in COMPONENTS:
        if not (run_all or touches(files, c["paths"])):
            continue
        if component_id(c) in known_broken:
            skipped_broken.append(component_id(c))
        else:
            selected.append(c)

    def matrix(kind):
        return [{k: v for k, v in c.items() if k not in ("kind", "paths")} for c in selected if c["kind"] == kind]

    outputs = {
        "programs": json.dumps(matrix("program")),
        "modules": json.dumps(matrix("module")),
        "apps": json.dumps(matrix("app")),
    }
    singles = {c["name"] for c in selected if c["kind"] == "single"}
    for c in COMPONENTS:
        if c["kind"] == "single":
            outputs[c["name"]] = json.dumps(c["name"] in singles)

    for key, value in outputs.items():
        print(f"{key}={value}")

    summary = ["## CI plan", ""]
    summary.append("Running: " + (", ".join(component_id(c) for c in selected) or "nothing"))
    for cid in skipped_broken:
        reason = known_broken[cid]
        summary.append(f"- Skipped `{cid}` (known broken{': ' + reason if reason else ''})")
        print(f"::warning title=Known broken::{cid} skipped{': ' + reason if reason else ''}")

    if gh_output := os.environ.get("GITHUB_OUTPUT"):
        with open(gh_output, "a") as fh:
            for key, value in outputs.items():
                fh.write(f"{key}={value}\n")
    if gh_summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(gh_summary, "a") as fh:
            fh.write("\n".join(summary) + "\n")
    else:
        print("\n".join(summary), file=sys.stderr)


if __name__ == "__main__":
    main()
