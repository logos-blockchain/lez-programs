"""Exercise the sourced helper independently of deployment CLI doubles."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class WorkspaceEnvironmentTests(unittest.TestCase):
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
