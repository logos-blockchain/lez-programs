#!/usr/bin/env python3
"""Check which files Docker sends to the guest builder."""

import subprocess
import tempfile
from pathlib import Path


root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="guest-context-test-") as temporary:
    context = Path(temporary) / "context"
    output = Path(temporary) / "output"
    context.mkdir()
    files = {
        ".dockerignore": (root / ".dockerignore").read_text(),
        "Dockerfile": "FROM scratch\nCOPY . /context/\n",
        ".cargo/config.toml": "[build]\n",
        ".hidden/cache.toml": "temporary = true\n",
        "programs/amm/target/cache": "build artifact\n",
        "programs/amm/methods/guest/target/cache": "guest build artifact\n",
        "programs/amm/src/lib.rs": "// source\n",
    }
    for name, contents in files.items():
        path = context / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
    subprocess.run(
        ["docker", "buildx", "build", "--quiet", "--output", f"type=local,dest={output}", str(context)],
        check=True,
    )
    copied = output / "context"
    for name in (".cargo/config.toml", "programs/amm/src/lib.rs"):
        if not (copied / name).is_file():
            raise SystemExit(f"Required file is absent from the build context: {name}")
    for name in (".hidden/cache.toml", "programs/amm/target/cache", "programs/amm/methods/guest/target/cache"):
        if (copied / name).exists():
            raise SystemExit(f"Unexpected file in the build context: {name}")
print("Guest build context test passed")
