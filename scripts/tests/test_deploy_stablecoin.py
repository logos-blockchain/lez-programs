"""Run with python3 -m unittest discover -s scripts/tests -p 'test_*.py'."""

import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from stablecoin_fake_tool import PROGRAMS, ROLES, address, program_transaction

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "deploy_stablecoin", ROOT / "scripts/deploy_stablecoin.py"
)
deploy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(deploy)
CHANNEL = "aa" * 32
DEFAULT_ACCOUNT = {"program_owner": [0] * 8, "balance": 0, "nonce": "0", "data": []}


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="stablecoin test ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        wrapper = self.root / "apps/stablecoin/tests/testnet"
        wrapper.mkdir(parents=True)
        shutil.copy2(
            ROOT / "apps/stablecoin/tests/testnet/setup-stablecoin-testnet.sh",
            wrapper / "setup-stablecoin-testnet.sh",
        )
        for name in ("deploy_stablecoin.py", "workspace-env.sh"):
            shutil.copy2(ROOT / "scripts" / name, self.root / "scripts" / name)
        shutil.copytree(ROOT / "artifacts", self.root / "artifacts")
        self.home = self.root / "wallet home"
        self.home.mkdir()
        (self.home / "storage.json").write_text(
            json.dumps({"labels": {}, "key_chain": "untouched"})
        )
        self.state_path = self.root / "state.json"
        self.state_path.write_text(
            json.dumps(
                {
                    "channel": CHANNEL,
                    "calls": [],
                    "used_configs": [],
                    "deployed": [],
                    "transactions": {},
                    "discovered": {},
                    "accounts": {
                        address("clock"): {
                            **DEFAULT_ACCOUNT,
                            "data": list(
                                (1).to_bytes(8, "little")
                                + (1700000000000).to_bytes(8, "little")
                            ),
                        }
                    },
                }
            )
        )
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                query = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                state = owner.state()
                method, params = query["method"], query["params"]
                owner.auth_headers.append(self.headers.get("Authorization"))
                result = {"jsonrpc": "2.0", "id": query["id"]}
                if method in state.get("rpc_errors", {}):
                    result["error"] = {
                        "code": state["rpc_errors"][method],
                        "message": "test failure",
                    }
                else:
                    if method == "getChannelId":
                        value = state["channel"]
                    elif method == "getProgramIds":
                        value = state["discovered"]
                    elif method == "getAccount":
                        value = state["accounts"].get(params[0], DEFAULT_ACCOUNT)
                    elif method == "getTransaction":
                        value = (
                            None
                            if state.get("hide_receipts")
                            else state["transactions"].get(params[0])
                        )
                    else:
                        raise AssertionError(method)
                    result["result"] = value
                response = json.dumps(result).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)

        self.auth_headers = []
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.url = f"http://127.0.0.1:{self.server.server_port}"
        self.config = {
            "sequencers": [
                {
                    "sequencer_addr": self.url,
                    "basic_auth": {"username": "user", "password": "secret"},
                }
            ],
            "seq_poll_timeout": "3s",
            "seq_tx_poll_max_blocks": 40,
            "seq_poll_max_retries": 5,
            "seq_block_poll_max_amount": 100,
            "custom_setting": "preserved",
        }
        self.write_config()
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        fake = fake_bin / "tool.py"
        shutil.copy2(Path(__file__).with_name("stablecoin_fake_tool.py"), fake)
        fake.chmod(0o755)
        for name in ("wallet", "spel", "cargo", "make"):
            (fake_bin / name).symlink_to(fake)
        self.env = {
            k: v
            for k, v in os.environ.items()
            if k in ("HOME", "PATH", "LANG", "PYTHONPATH")
        }
        self.env.update(
            PATH=str(fake_bin) + os.pathsep + self.env["PATH"],
            FAKE_STATE=str(self.state_path),
            LEE_WALLET_HOME_DIR=str(self.home),
        )

    def write_config(self):
        self.saved_config = json.dumps(self.config, indent=2)
        (self.home / "wallet_config.json").write_text(self.saved_config)

    def state(self):
        return json.loads(self.state_path.read_text())

    def change(self, **values):
        state = self.state()
        state.update(values)
        self.state_path.write_text(json.dumps(state))

    def run_script(self, success=True, **env):
        result = subprocess.run(
            [str(self.root / "apps/stablecoin/tests/testnet/setup-stablecoin-testnet.sh")],
            cwd=self.root.parent,
            env={**self.env, **env},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=30,
            check=False,
        )
        if success:
            self.assertEqual(result.returncode, 0, result.stdout)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(
            (self.home / "wallet_config.json").read_text(), self.saved_config
        )
        return result

    def manifest(self):
        return json.loads(
            (
                self.root
                / "target/deployments/stablecoin"
                / CHANNEL
                / "deployment.json"
            ).read_text()
        )

    def tx_calls(self):
        return [
            c
            for c in self.state()["calls"]
            if c[:2] == ["wallet", "deploy-program"]
            or (c[0] == "spel" and "--program-id" in c)
        ]

    def test_fresh_bootstrap_and_no_transaction_rerun(self):
        self.run_script()
        manifest = self.manifest()
        self.assertTrue(manifest["complete"])
        self.assertEqual(len(self.tx_calls()), 6)
        self.assertEqual(
            manifest["settings"]["collateral_supply"], "1000000000000000000000"
        )
        self.assertEqual(
            manifest["settings"]["oracle_initial_price"], "18446744073709551616"
        )
        for role in ROLES:
            self.assertEqual(manifest["accounts"][role], address(role))
        storage = json.loads((self.home / "storage.json").read_text())
        self.assertEqual(len(storage["labels"]), 4)
        self.assertEqual(storage["key_chain"], "untouched")
        self.assertTrue(all(c == self.config for c in self.state()["used_configs"]))
        self.assertTrue(all(h == "Basic dXNlcjpzZWNyZXQ=" for h in self.auth_headers))
        self.run_script()
        self.assertEqual(len(self.tx_calls()), 6)
        self.assertEqual(sum(c[0] == "make" for c in self.state()["calls"]), 1)

    def test_explicit_programs_need_no_build_or_deployment(self):
        self.run_script(
            **{
                name.upper() + "_PROGRAM_ID": deploy.account_id(pid)
                for name, pid in PROGRAMS.items()
            }
        )
        self.assertFalse(
            any(
                c[0] == "make" or c[:2] == ["wallet", "deploy-program"]
                for c in self.state()["calls"]
            )
        )

    def test_discovery_reuses_only_matching_images(self):
        self.change(
            discovered={
                "token": [7] * 8,
                "stablecoin": [int.from_bytes(bytes.fromhex("33" * 4), "little")] * 8,
            }
        )
        self.run_script()
        self.assertEqual(self.state()["deployed"], ["token", "twap_oracle"])

    def test_program_missing_from_discovery_can_already_exist(self):
        self.change(
            deployed=["token"],
            transactions={program_transaction("token"): ["confirmed deployment", 1]},
        )
        self.run_script()
        self.assertTrue(self.manifest()["complete"])
        self.assertFalse(any(
            c[:2] == ["wallet", "deploy-program"] and c[-1].endswith("token.bin")
            for c in self.tx_calls()
        ))

    def test_generic_deployment_error_is_not_proof_of_reuse(self):
        self.change(deployed=["token"])
        result = self.run_script(success=False)
        self.assertIn("exited with status", result.stdout)
        self.assertFalse(self.manifest()["complete"])

    def test_overridden_tools_preserve_dispatcher_symlinks(self):
        self.run_script(**{
            name.upper() + "_BIN": str(self.root / "bin" / name)
            for name in ("wallet", "spel", "cargo", "make")
        })

    def test_local_wallet_precedes_incompatible_global_wallet(self):
        local = self.root / "target/debug"
        local.mkdir(parents=True)
        (self.root / "bin/wallet").rename(local / "wallet")
        bad_wallet = self.root / "bin/wallet"
        bad_wallet.write_text("#!/bin/sh\necho incompatible wallet >&2\nexit 1\n")
        bad_wallet.chmod(0o755)
        self.run_script()

    def test_resume_after_deploy_failure_before_submission(self):
        self.change(fail_before="deploy-twap_oracle")
        self.run_script(success=False)
        self.change(fail_before=None)
        self.run_script()
        self.assertEqual(self.state()["deployed"], list(PROGRAMS))
        self.assertEqual(
            sum(
                c[:2] == ["wallet", "deploy-program"] and c[-1].endswith("token.bin")
                for c in self.state()["calls"]
            ),
            1,
        )

    def test_resume_after_committed_mint_without_cli_success(self):
        self.change(fail_after="new-fungible-definition")
        self.run_script(success=False)
        self.assertFalse(self.manifest()["complete"])
        self.change(fail_after=None)
        self.run_script()
        self.assertEqual(
            sum("new-fungible-definition" in c for c in self.tx_calls()), 1
        )

    def test_failed_confirmation_never_reports_complete_or_retries_transaction(self):
        self.change(hide_receipts=True)
        result = self.run_script(success=False)
        self.assertIn("not confirmed", result.stdout)
        self.assertFalse(self.manifest()["complete"])
        self.run_script(success=False)
        self.assertEqual(len(self.tx_calls()), 1)
        self.change(hide_receipts=False)
        self.run_script()

    def test_network_identity_mismatch_prevents_transactions(self):
        self.run_script()
        directory = self.root / "target/deployments/stablecoin" / CHANNEL
        self.change(channel="bb" * 32)
        result = self.run_script(success=False, DEPLOYMENT_DIR=str(directory))
        self.assertIn("another network", result.stdout)
        self.assertEqual(len(self.tx_calls()), 6)

    def test_endpoint_override_is_temporary_and_does_not_forward_other_auth(self):
        self.config["sequencers"] = [
            {
                "sequencer_addr": "http://127.0.0.1:1",
                "basic_auth": {"username": "private", "password": "other"},
            }
        ]
        self.write_config()
        self.run_script(SEQUENCER_ADDR=self.url)
        self.assertTrue(all(h is None for h in self.auth_headers))
        self.assertTrue(
            all(
                c["sequencers"] == [{"sequencer_addr": self.url}]
                for c in self.state()["used_configs"]
            )
        )
        self.assertEqual(
            len(json.loads((self.home / "storage.json").read_text())["labels"]), 4
        )

    def test_endpoint_failure_and_rpc_failure_do_not_create_accounts(self):
        self.run_script(success=False, SEQUENCER_ADDR="http://127.0.0.1:1")
        self.assertEqual(self.tx_calls(), [])
        self.change(rpc_errors={"getAccount": -32001})
        self.run_script(
            success=False,
            **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
        )
        self.assertEqual(self.tx_calls(), [])
        self.assertEqual(
            json.loads((self.home / "storage.json").read_text())["labels"], {}
        )

    def test_discovery_method_missing_is_allowed_but_server_errors_are_not(self):
        self.change(rpc_errors={"getProgramIds": -32001})
        self.run_script(success=False)
        self.assertEqual(self.tx_calls(), [])
        self.change(rpc_errors={"getProgramIds": -32601})
        self.run_script()

    def test_binary_id_mismatch_fails_before_transactions(self):
        result = self.run_script(
            success=False,
            STABLECOIN_PROGRAM_ID="44" * 32,
            STABLECOIN_PROGRAM_BIN=str(self.root / "target/guest/stablecoin.bin"),
        )
        self.assertIn("does not match binary", result.stdout)
        self.assertEqual(self.tx_calls(), [])

    def test_large_integers_and_quoted_names(self):
        name = "Stable 'coin' $(touch never-created)"
        self.run_script(
            COLLATERAL_SUPPLY=str(2**128 - 1),
            STABLECOIN_NAME=name,
        )
        state = self.manifest()["verifiedState"]
        self.assertEqual(
            state["protocol_parameters"]["controller_proportional_gain"],
            "0",
        )
        self.assertEqual(state["stablecoin_definition"]["Fungible"]["name"], name)
        self.assertFalse((self.root / "never-created").exists())

    def test_nonzero_controller_gains_fail_before_any_command(self):
        for variable, value in (
            ("INITIAL_CONTROLLER_PROPORTIONAL_GAIN", "-123456789012345678901234567"),
            ("INITIAL_CONTROLLER_INTEGRAL_GAIN", "1"),
        ):
            with self.subTest(variable=variable):
                result = self.run_script(success=False, **{variable: value})
                self.assertIn("Controller gains must be zero", result.stdout)
                self.assertEqual(self.state()["calls"], [])

    def test_bad_integer_is_rejected_before_commands(self):
        self.run_script(success=False, COLLATERAL_SUPPLY="1e21")
        self.assertEqual(self.state()["calls"], [])

    def test_old_spel_is_rejected_before_transactions_and_account_creation(self):
        self.change(unsupported_decoder=True)
        result = self.run_script(success=False)
        self.assertIn("SPEL cannot decode current IDLs", result.stdout)
        self.assertEqual(self.tx_calls(), [])
        self.assertEqual(
            json.loads((self.home / "storage.json").read_text())["labels"], {}
        )

    def test_invalid_account_is_rejected_before_commands(self):
        self.assertIn(
            "ADMIN_ID", self.run_script(success=False, ADMIN_ID="not-an-id").stdout
        )
        self.assertEqual(self.state()["calls"], [])

    def test_existing_collateral_and_oracle_are_reused(self):
        self.change(fail_before="initialize-program")
        self.run_script(success=False)
        accounts = self.manifest()["accounts"]
        self.change(fail_before=None)
        self.run_script(
            DEPLOYMENT_DIR=str(self.root / "external assets"),
            COLLATERAL_DEFINITION_ID=accounts["collateral_definition"],
            MARKET_PRICE_ORACLE_ID=accounts["market_price_oracle"],
            **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
        )
        self.assertEqual(
            sum("new-fungible-definition" in c for c in self.tx_calls()), 1
        )
        self.assertEqual(
            sum("create-oracle-price-account" in c for c in self.tx_calls()), 1
        )

    def test_changed_redemption_price_is_not_silently_ignored(self):
        self.run_script()
        result = self.run_script(
            success=False, INITIAL_REDEMPTION_PRICE=str(2 * 10**27)
        )
        self.assertIn("INITIAL_REDEMPTION_PRICE", result.stdout)
        self.assertEqual(len(self.tx_calls()), 6)

    def test_resume_rejects_changed_seed_settings_without_overwriting_manifest(self):
        self.change(fail_before="initialize-program")
        self.run_script(success=False)
        before = self.manifest()
        self.change(fail_before=None)
        for overrides in (
            {"COLLATERAL_SUPPLY": "2"},
            {"ORACLE_INITIAL_PRICE": str(2**65)},
            {"ORACLE_WINDOW_MILLISECONDS": "400000"},
        ):
            with self.subTest(overrides=overrides):
                result = self.run_script(success=False, **overrides)
                self.assertIn("bootstrap settings cannot change", result.stdout)
                self.assertEqual(self.manifest(), before)
        self.run_script()
        self.assertTrue(self.manifest()["complete"])

    def test_rejected_protocol_override_does_not_poison_next_resume(self):
        self.run_script()
        before = self.manifest()
        self.run_script(
            success=False,
            INITIAL_MINIMUM_COLLATERALIZATION_RATIO=str(2 * 10**27),
        )
        self.assertEqual(self.manifest(), before)
        self.run_script()
        self.assertEqual(self.manifest()["settings"], before["settings"])
        self.assertEqual(len(self.tx_calls()), 6)

    def test_legacy_manifest_settings_are_locked_on_resume(self):
        self.run_script()
        path = self.root / "target/deployments/stablecoin" / CHANNEL / "deployment.json"
        legacy = self.manifest()
        legacy.pop("settingsLocked")
        path.write_text(json.dumps(legacy))
        self.run_script(success=False, COLLATERAL_SUPPLY="2")
        self.assertEqual(self.manifest(), legacy)
        self.run_script()

    def test_different_wallet_cannot_write_locked_deployment(self):
        other_home = self.root / "other wallet"
        shutil.copytree(self.home, other_home)
        with patch.dict(os.environ, {}, clear=True):
            with deploy.deployment_directory(self.root, CHANNEL):
                result = self.run_script(success=False, LEE_WALLET_HOME_DIR=str(other_home))
        self.assertIn("Another process is using this deployment directory", result.stdout)
        self.assertEqual(self.tx_calls(), [])

    def test_rejected_adoption_override_is_not_saved(self):
        self.run_script()
        directory = self.root / "adopted"
        env = {
            "DEPLOYMENT_DIR": str(directory),
            **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
        }
        self.run_script(
            success=False,
            INITIAL_MINIMUM_COLLATERALIZATION_RATIO=str(2 * 10**27),
            **env,
        )
        self.run_script(**env)
        adopted = json.loads((directory / "deployment.json").read_text())
        self.assertEqual(
            adopted["verifiedState"]["protocol_parameters"]["minimum_collateralization_ratio"],
            str(15 * 10**26),
        )
        self.assertNotIn("settings", adopted)

    def test_adoption_does_not_invent_original_bootstrap_settings(self):
        self.run_script(
            INITIAL_MINIMUM_COLLATERALIZATION_RATIO=str(2 * 10**27),
            INITIAL_REDEMPTION_PRICE=str(3 * 10**27),
            STABLECOIN_NAME="Existing custom stablecoin",
        )
        directory = self.root / "adopted"
        env = {
            "DEPLOYMENT_DIR": str(directory),
            **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
        }
        self.run_script(**env)
        adopted = json.loads((directory / "deployment.json").read_text())
        self.assertEqual(adopted["settingsOrigin"], "adopted")
        self.assertNotIn("settings", adopted)
        self.assertEqual(
            adopted["verifiedState"]["protocol_parameters"]["minimum_collateralization_ratio"],
            str(2 * 10**27),
        )
        self.run_script(**env)
        self.run_script(success=False, ORACLE_INITIAL_PRICE=str(2**65), **env)
        self.assertEqual(len(self.tx_calls()), 6)

    def test_rejected_adoption_bindings_do_not_poison_manifest(self):
        self.run_script()
        for variable in ("ADMIN_ID", "FREEZE_AUTHORITY_ID", "COLLATERAL_DEFINITION_ID", "MARKET_PRICE_ORACLE_ID"):
            with self.subTest(variable=variable):
                directory = self.root / variable
                env = {
                    "DEPLOYMENT_DIR": str(directory),
                    **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
                }
                self.run_script(success=False, **{variable: "ab" * 32}, **env)
                self.run_script(**env)
                adopted = json.loads((directory / "deployment.json").read_text())
                self.assertTrue(adopted["complete"])
        self.assertEqual(len(self.tx_calls()), 6)

    def test_existing_protocol_conflict_and_partial_state_fail_without_transactions(
        self,
    ):
        self.run_script()
        self.run_script(success=False, ADMIN_ID="ab" * 32)
        self.assertEqual(len(self.tx_calls()), 6)
        state = self.state()
        del state["accounts"][address("redemption_price_state")]
        self.change(accounts=state["accounts"])
        result = self.run_script(success=False)
        self.assertIn("partially initialized", result.stdout)
        self.assertEqual(len(self.tx_calls()), 6)

    def test_wrong_oracle_pair_and_account_owner_fail(self):
        self.run_script()
        state = self.state()
        oracle = state["accounts"][address("oracle_price_account")]
        data = json.loads(bytes(oracle["data"]))
        data["quote_asset"] = address("wrong collateral")
        oracle["data"] = list(json.dumps(data).encode())
        self.change(accounts=state["accounts"])
        self.assertIn("market_price_oracle", self.run_script(success=False).stdout)
        state["accounts"][address("protocol_parameters")]["program_owner"] = [1] * 8
        self.change(accounts=state["accounts"])
        self.assertIn("unexpected program owner", self.run_script(success=False).stdout)
        self.assertEqual(len(self.tx_calls()), 6)

    def test_existing_initialized_protocol_reuses_accounts_without_manifest(self):
        self.run_script()
        before_labels = json.loads((self.home / "storage.json").read_text())["labels"]
        self.run_script(
            DEPLOYMENT_DIR=str(self.root / "second manifest"),
            **{name.upper() + "_PROGRAM_ID": pid for name, pid in PROGRAMS.items()},
        )
        self.assertEqual(len(self.tx_calls()), 6)
        self.assertEqual(
            before_labels,
            json.loads((self.home / "storage.json").read_text())["labels"],
        )


class EncodingTests(unittest.TestCase):
    def test_signed_transport_preserves_twos_complement_without_editing_idl(self):
        path = ROOT / "artifacts/stablecoin-idl.json"
        original = path.read_bytes()
        args = {
            "initial_controller_proportional_gain": "-123456789012345678901234567",
            "initial_controller_integral_gain": "-1",
        }
        with deploy.instruction_transport(path, "initialize-program", args) as (transport, values):
            self.assertNotEqual(transport, path)
            for key, value in args.items():
                self.assertEqual(int(values[key]), int(value) + 2**128)
        self.assertEqual(path.read_bytes(), original)

    def test_spel_account_prefix_does_not_change_token_names(self):
        raw = {
            "Fungible": {
                "definition_id": "Public/" + "1" * 32,
                "authority": None,
                "name": "Public/" + "1" * 32,
            }
        }
        normalized = deploy.normalize_decoded_ids(raw)
        self.assertEqual(normalized["Fungible"]["definition_id"], "1" * 32)
        self.assertEqual(normalized["Fungible"]["name"], raw["Fungible"]["name"])
        self.assertIsNone(normalized["Fungible"]["authority"])

    def test_program_limb_byte_order_and_base58_leading_zeros(self):
        limbs = [1, 2, 3, 4, 5, 6, 7, 8]
        expected = "0100000002000000030000000400000005000000060000000700000008000000"
        self.assertEqual(deploy.id_bytes(limbs).hex(), expected)
        self.assertEqual(deploy.id_bytes(deploy.account_id(expected)).hex(), expected)
        self.assertEqual(deploy.account_id("0" * 64), "1" * 32)
        self.assertEqual(deploy.id_bytes("1" * 32), bytes(32))
        for value in ("1", "z" * 44, [True] * 8, [2**32] * 8):
            with self.assertRaises(deploy.DeploymentError):
                deploy.id_bytes(value)


if __name__ == "__main__":
    unittest.main()
