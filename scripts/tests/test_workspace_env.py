"""Exercise the sourced helper independently of deployment CLI doubles."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class WorkspaceEnvironmentTests(unittest.TestCase):
    def test_devnet_setup_builds_the_resolved_standalone_sequencer(self):
        with tempfile.TemporaryDirectory(prefix="devnet tools ") as temporary:
            root = Path(temporary)
            upstream = root / "execution-zone"
            spel = root / "spel"
            for path in (
                upstream / "lee/state_machine/core/Cargo.toml",
                upstream / "lez/wallet/Cargo.toml",
                upstream / "lez/sequencer/service/Cargo.toml",
                spel / "spel-framework-core/Cargo.toml",
                spel / "spel-cli/Cargo.toml",
            ):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            metadata = {"packages": [
                {"name": "lee_core", "manifest_path": str(upstream / "lee/state_machine/core/Cargo.toml")},
                {"name": "spel-framework-core", "manifest_path": str(spel / "spel-framework-core/Cargo.toml")},
            ]}
            calls = root / "calls.json"
            cargo = root / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\n"
                "if sys.argv[1]=='metadata': print(os.environ['DEVNET_TEST_METADATA'])\n"
                "else:\n p=Path(os.environ['DEVNET_TEST_CALLS'])\n"
                " rows=json.loads(p.read_text()) if p.exists() else []\n"
                " rows.append(sys.argv[1:]);p.write_text(json.dumps(rows))\n"
            )
            cargo.chmod(0o755)
            subprocess.run(
                ["python3", str(ROOT / "scripts/setup-workspace-tools.py"), "--sequencer"],
                env=dict(os.environ, CARGO_BIN=str(cargo), DEVNET_TEST_METADATA=json.dumps(metadata), DEVNET_TEST_CALLS=str(calls)),
                check=True, capture_output=True, text=True,
            )
            builds = json.loads(calls.read_text())
            sequencer = next(args for args in builds if "sequencer_service" in args)
            self.assertEqual(sequencer[sequencer.index("--manifest-path") + 1], str(upstream / "lez/sequencer/service/Cargo.toml"))
            self.assertEqual(sequencer[sequencer.index("--features") + 1], "standalone")
            self.assertTrue(all("--locked" in args for args in builds))

    def test_switching_checkouts_prefers_local_tools_without_stale_overrides(self):
        with tempfile.TemporaryDirectory(prefix="workspace tools ") as temporary:
            root = Path(temporary)
            checkouts = [root / name for name in ("first checkout", "second checkout")]
            for checkout in checkouts:
                (checkout / "scripts").mkdir(parents=True)
                shutil.copy2(ROOT / "scripts/workspace-env.sh", checkout / "scripts")
                (checkout / "target/debug").mkdir(parents=True)
                for name in ("spel", "wallet"):
                    binary = checkout / "target/debug" / name
                    binary.write_text("#!/bin/sh\nexit 0\n")
                    binary.chmod(0o755)
            result = subprocess.run(
                ["bash", "-c", 'source "$1"; source "$2"; source "$2"; '
                 'command -v spel; command -v wallet; '
                 'printf "%s\\n" "${SPEL_BIN-unset}" "${WALLET_BIN-unset}" "$PATH"',
                 "test", *[str(p / "scripts/workspace-env.sh") for p in checkouts]],
                env={"HOME": str(root), "PATH": os.environ["PATH"]},
                check=True, capture_output=True, text=True,
            )
            spel, wallet, spel_override, wallet_override, path = result.stdout.splitlines()
            local = checkouts[1] / "target/debug"
            self.assertEqual(spel, str(local / "spel"))
            self.assertEqual(wallet, str(local / "wallet"))
            self.assertEqual((spel_override, wallet_override), ("unset", "unset"))
            self.assertEqual(path.split(os.pathsep).count(str(local)), 1)

    def test_existing_path_entry_moves_ahead_of_global_tools_and_preserves_overrides(self):
        local = ROOT / "target/debug"
        result = subprocess.run(
            ["bash", "-c", 'source "$1"; printf "%s\\n" "$PATH" "$SPEL_BIN" "$WALLET_BIN"',
             "test", str(ROOT / "scripts/workspace-env.sh")],
            env={"HOME": "/unused", "PATH": os.environ["PATH"] + os.pathsep + str(local),
                 "SPEL_BIN": "/custom/spel", "WALLET_BIN": "/custom/wallet"},
            check=True, capture_output=True, text=True,
        )
        path, spel, wallet = result.stdout.splitlines()
        self.assertEqual(path.split(os.pathsep)[0], str(local))
        self.assertEqual((spel, wallet), ("/custom/spel", "/custom/wallet"))


if __name__ == "__main__":
    unittest.main()
