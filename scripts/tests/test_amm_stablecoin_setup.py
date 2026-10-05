"""Exercise the AMM shell entry point without deploying or contacting a sequencer."""

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BASE_LABELS = [
    "token-a-def", "token-a-holding", "token-b-def", "token-b-holding", "lp-holding",
    "token-c-def", "token-c-holding", "token-d-def", "token-d-holding", "holder2",
    "holder2-a-holding", "amm-owner",
]

FAKE_TOOL = r'''#!/usr/bin/env python3
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path

def address(label):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    return "".join(alphabet[n % len(alphabet)] for n in hashlib.sha256(label.encode()).digest())

name = Path(sys.argv[0]).name
args = sys.argv[1:]
with Path(os.environ["CALL_LOG"]).open("a") as stream:
    stream.write(json.dumps([name, *args]) + "\n")
if name == "bootstrap":
    Path(os.environ["BOOTSTRAP_CAPTURE"]).write_text(json.dumps(dict(os.environ)))
    if os.environ.get("BOOTSTRAP_REBUILD") == "1":
        root = Path(os.environ["REPO_ROOT"])
        for target in root.glob("programs/*/methods/guest/target"):
            shutil.rmtree(target)
        shared = root / "target/guest"
        shutil.rmtree(shared)
        shared.mkdir()
        for program in ("token", "amm", "twap_oracle", "token_mint_authority"):
            (shared / f"{program}.bin").write_bytes(b"rebuilt " + program.encode())
    sys.exit(int(os.environ.get("BOOTSTRAP_FAILURE", "0")))
if name == "wallet":
    storage = Path(os.environ["LEE_WALLET_HOME_DIR"]) / "storage.json"
    data = json.loads(storage.read_text())
    if args[:2] == ["account", "id"]:
        label = args[args.index("--account-id") + 1]
        if label not in data["labels"]:
            sys.exit(1)
        print(data["labels"][label])
    elif args[:3] == ["account", "new", "public"]:
        label = args[args.index("--label") + 1]
        data["labels"][label] = address(label)
        storage.write_text(json.dumps(data))
    else:
        if args[:1] == ["deploy-program"]:
            recorded = Path(os.environ["DEPLOYED_BYTES"])
            binaries = json.loads(recorded.read_text()) if recorded.exists() else {}
            binaries[args[-1]] = Path(args[-1]).read_bytes().hex()
            recorded.write_text(json.dumps(binaries))
        print("Transaction confirmed — included in a block")
elif name == "spel":
    if "program-id" in args:
        if args[:4] != ["--format", "hex", "--", "program-id"]:
            sys.exit("program-id must select hex output")
        program = Path(args[-1]).stem
        index = {"token": 1, "amm": 2, "twap_oracle": 3, "token_mint_authority": 4}.get(program, 1)
        print(f"{index:02x}" * 32)
    elif "inspect" in args:
        print("{}")
    else:
        print("Transaction confirmed — included in a block")
elif name == "cargo":
    example = args[args.index("--example") + 1]
    if example in ("mint_authority", "faucet_allowance"):
        print("base58:", address(example))
    else:
        for key in ("config", "pool", "vault_a", "vault_b", "pool_definition_lp", "lp_lock_holding", "current_tick_account"):
            print(key, address(key))
'''


def address(label):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    return "".join(alphabet[n % len(alphabet)] for n in hashlib.sha256(label.encode()).digest())


class AmmStablecoinSetupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='amm setup "test" ')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.setup_dir = self.root / "apps/amm/tests/testnet"
        self.setup_dir.mkdir(parents=True)
        self.script = self.setup_dir / "setup-amm-testnet.sh"
        shutil.copy2(ROOT / "apps/amm/tests/testnet/setup-amm-testnet.sh", self.script)
        (self.root / "scripts").mkdir()
        shutil.copy2(ROOT / "scripts/workspace-env.sh", self.root / "scripts")
        artifacts = self.root / "artifacts"
        artifacts.mkdir()
        for name in ("token", "amm", "token_mint_authority"):
            (artifacts / f"{name}-idl.json").write_text("{}")
        self.binaries = self.root / "target/guest"
        self.binaries.mkdir(parents=True)
        for name in ("token", "amm", "twap_oracle", "token_mint_authority"):
            (self.binaries / f"{name}.bin").write_bytes(b"guest fixture")
        self.home = self.setup_dir / ".wallet"
        self.home.mkdir()
        (self.home / "storage.json").write_text(json.dumps({"labels": {}}))
        self.fake_bin = self.root / "bin"
        self.fake_bin.mkdir()
        for name in ("wallet", "spel", "cargo", "bootstrap"):
            executable = self.fake_bin / name
            executable.write_text(FAKE_TOOL)
            executable.chmod(0o755)
        bootstrap = self.root / "apps/stablecoin/tests/testnet/setup-stablecoin-testnet.sh"
        bootstrap.parent.mkdir(parents=True)
        bootstrap.write_text('#!/usr/bin/env bash\nexec bootstrap "$@"\n')
        bootstrap.chmod(0o755)
        self.log = self.root / "calls.jsonl"
        self.capture = self.root / "bootstrap.json"
        self.deployed = self.root / "deployed.json"

    def run_setup(self, success=True, **overrides):
        env = {
            "PATH": str(self.fake_bin) + os.pathsep + os.environ["PATH"],
            "HOME": str(self.root), "REPO_ROOT": str(self.root),
            "TEST_WALLET_HOME": str(self.home),
            "TEST_SEQUENCER_ADDR": "http://isolated-sequencer.invalid:3040",
            "CALL_LOG": str(self.log), "BOOTSTRAP_CAPTURE": str(self.capture),
            "DEPLOYED_BYTES": str(self.deployed),
            **overrides,
        }
        result = subprocess.run(
            ["bash", str(self.script)], env=env, cwd=self.root,
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_default_bootstrap_reuses_programs_wallet_and_faucet(self):
        self.run_setup(
            SEQUENCER_ADDR="http://other-network.invalid",
            DEPLOYMENT_DIR="unrelated standalone deployment",
            COLLATERAL_DEFINITION_ID="unrelated collateral",
            COLLATERAL_HOLDING_ID="unrelated holding",
            MARKET_PRICE_ORACLE_ID="unrelated oracle",
            TOKEN_PROGRAM_BIN="unrelated token binary",
            TWAP_ORACLE_PROGRAM_BIN="unrelated oracle binary",
        )
        env = json.loads(self.capture.read_text())
        self.assertEqual(env["LEE_WALLET_HOME_DIR"], str(self.home))
        config = json.loads((self.home / "wallet_config.json").read_text())
        self.assertEqual(config["sequencers"][0]["sequencer_addr"], "http://isolated-sequencer.invalid:3040")
        self.assertEqual(env["TOKEN_PROGRAM_ID"], "01" * 32)
        self.assertEqual(env["TWAP_ORACLE_PROGRAM_ID"], "03" * 32)
        self.assertEqual(env["ADMIN_ID"], address("stablecoin-admin"))
        self.assertEqual(env["ORACLE_SOURCE_ID"], address("stablecoin-oracle-source"))
        self.assertEqual(env["COLLATERAL_MINT_AUTHORITY_ID"], address("mint_authority"))
        for name in ("SEQUENCER_ADDR", "DEPLOYMENT_DIR", "COLLATERAL_DEFINITION_ID", "COLLATERAL_HOLDING_ID", "MARKET_PRICE_ORACLE_ID", "TOKEN_PROGRAM_BIN", "TWAP_ORACLE_PROGRAM_BIN"):
            self.assertNotIn(name, env)
        labels = json.loads((self.home / "storage.json").read_text())["labels"]
        self.assertEqual(list(labels), BASE_LABELS + ["stablecoin-admin", "stablecoin-oracle-source"])
        deployments = [call[-1] for call in self.calls() if call[:2] == ["wallet", "deploy-program"]]
        self.assertEqual([Path(path).name for path in deployments], [f"{name}.bin" for name in ("token", "amm", "twap_oracle", "token_mint_authority")])
        for path in deployments:
            self.assertEqual(Path(path).read_bytes(), b"guest fixture")

    def test_explicit_skip_leaves_amm_accounts_and_skips_bootstrap(self):
        self.run_setup(DEPLOY_STABLECOIN="0")
        self.assertFalse(self.capture.exists())
        labels = json.loads((self.home / "storage.json").read_text())["labels"]
        self.assertEqual(list(labels), BASE_LABELS)

    def test_explicit_paths_preserve_json_and_canonical_output_override(self):
        binary = self.binaries / 'token "quoted" \\ guest.bin'
        binary.write_bytes(b"guest fixture")
        directory = self.root / 'stablecoin "deployment"'
        self.run_setup(TOKEN_BIN=str(binary), STABLECOIN_DEPLOYMENT_DIR=str(directory))
        env = json.loads(self.capture.read_text())
        self.assertEqual(env["DEPLOYMENT_DIR"], str(directory))
        faucet = json.loads((self.setup_dir / "faucet.json").read_text())
        self.assertEqual(Path(faucet["tokenBin"]).read_bytes(), binary.read_bytes())
        self.assertIn('"', faucet["tokenBin"])
        self.assertEqual(faucet["mintAuthority"], env["COLLATERAL_MINT_AUTHORITY_ID"])

    def assert_deployed_binaries_survive(self, output):
        deployed = json.loads(self.deployed.read_text())
        self.assertEqual(len(deployed), 4)
        for path, original in deployed.items():
            self.assertEqual(Path(path).read_bytes().hex(), original)
            self.assertIn(["spel", "--format", "hex", "--", "program-id", path], self.calls())
        faucet = json.loads((self.setup_dir / "faucet.json").read_text())
        self.assertIn(faucet["tokenBin"], deployed)
        self.assertIn(faucet["faucetBin"], deployed)
        amm = next(path for path in deployed if Path(path).stem == "amm")
        self.assertIn("AMM_PROGRAM_BIN=" + amm, output)
        faucet_pdas = [call for call in self.calls() if call[0] == "cargo" and "mint_authority" in call]
        self.assertEqual(faucet_pdas[0][-1], faucet["faucetBin"])

    def test_shared_binaries_survive_delegated_rebuild(self):
        result = self.run_setup(BOOTSTRAP_REBUILD="1")
        self.assertEqual((self.binaries / "amm.bin").read_bytes(), b"rebuilt amm")
        self.assert_deployed_binaries_survive(result.stdout)

    def test_legacy_binaries_survive_default_enabled_bootstrap(self):
        for binary in self.binaries.glob("*.bin"):
            legacy = self.root / f"programs/{binary.stem}/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/{binary.name}"
            legacy.parent.mkdir(parents=True)
            binary.rename(legacy)
        result = self.run_setup(BOOTSTRAP_REBUILD="1")
        self.assertFalse(any(self.root.glob("programs/*/methods/guest/target")))
        self.assert_deployed_binaries_survive(result.stdout)

    def test_legacy_binary_layout_remains_supported(self):
        self.binaries.joinpath("token.bin").unlink()
        legacy = self.root / "programs/token/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/token.bin"
        legacy.parent.mkdir(parents=True)
        legacy.write_bytes(b"legacy guest fixture")
        self.run_setup(DEPLOY_STABLECOIN="0")
        self.assertIn(["wallet", "deploy-program", str(legacy.relative_to(self.root))], self.calls())

    def test_failed_bootstrap_fails_setup(self):
        result = self.run_setup(success=False, BOOTSTRAP_FAILURE="1")
        self.assertIn("stablecoin deployment failed", result.stderr)
        self.assertNotIn("Setup complete.", result.stdout)

    def test_invalid_enable_flag_stops_before_wallet_commands(self):
        result = self.run_setup(success=False, DEPLOY_STABLECOIN="false")
        self.assertIn("DEPLOY_STABLECOIN must be 0 or 1", result.stderr)
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
