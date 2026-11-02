#!/usr/bin/env python3
"""Check that the CI TOML command selects the tracked manifests."""

import subprocess
import tempfile
from pathlib import Path


root = Path(__file__).resolve().parents[2]
workflow = (root / ".github/workflows/ci.yml").read_text().splitlines()
step = next(i for i, line in enumerate(workflow) if line.strip() == "- name: Check TOML files are formatted")
run_line = workflow[step + 1].strip()
if not run_line.startswith("run: "):
    raise SystemExit("TOML format step must contain a run command")
command = run_line.removeprefix("run: ")

with tempfile.TemporaryDirectory(prefix="toml-check-test-") as temporary:
    fixture = Path(temporary)
    subprocess.run(["git", "init", "--quiet", str(fixture)], check=True)
    (fixture / "tracked manifest.toml").write_text("value = 1\n")
    (fixture / "other manifest.toml").write_text("value=1\n")
    subprocess.run(["git", "add", "--", "tracked manifest.toml"], cwd=fixture, check=True)

    def check():
        return subprocess.run(["bash", "-e", "-o", "pipefail", "-c", command], cwd=fixture, capture_output=True, text=True)

    result = check()
    if result.returncode:
        raise SystemExit(f"Formatted tracked manifest failed:\n{result.stdout}{result.stderr}")
    subprocess.run(["git", "add", "--", "other manifest.toml"], cwd=fixture, check=True)
    result = check()
    if result.returncode == 0:
        raise SystemExit("Incorrectly formatted tracked manifest passed")
print("TOML check file selection test passed")
