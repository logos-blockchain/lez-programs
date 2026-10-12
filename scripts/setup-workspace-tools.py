#!/usr/bin/env python3
"""Build wallet, SPEL, and optionally a standalone sequencer from workspace pins."""

import argparse
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--sequencer", action="store_true", help="Also build the standalone sequencer"
    )
    args = parser.parse_args()
    cargo = os.environ.get("CARGO_BIN", "cargo")
    metadata = json.loads(
        subprocess.check_output(
            [cargo, "metadata", "--locked", "--format-version", "1"], cwd=ROOT
        )
    )
    packages = metadata["packages"]
    # Resolve through Cargo, not checkout directory names or copied revision IDs.
    core = next(p for p in packages if p["name"] == "lee_core")
    framework = next(p for p in packages if p["name"] == "spel-framework-core")

    def sibling_manifest(package, relative):
        return next(
            parent / relative
            for parent in Path(package["manifest_path"]).parents
            if (parent / relative).is_file()
        )

    builds = [
        ("wallet", sibling_manifest(core, "lez/wallet/Cargo.toml"), []),
        ("spel", sibling_manifest(framework, "spel-cli/Cargo.toml"), []),
    ]
    if args.sequencer:
        builds.append((
            "sequencer_service",
            sibling_manifest(core, "lez/sequencer/service/Cargo.toml"),
            ["--features", "standalone"],
        ))
    env = dict(os.environ, RISC0_SKIP_BUILD="1", RISC0_DEV_MODE="1")
    for binary, manifest, features in builds:
        subprocess.run(
            [
                cargo, "build", "--locked", "--manifest-path", str(manifest),
                "--bin", binary, "--target-dir", str(ROOT / "target"), *features,
            ],
            cwd=ROOT,
            env=env,
            check=True,
        )
    print("Workspace tools ready. Source scripts/workspace-env.sh to use them.")


if __name__ == "__main__":
    main()
