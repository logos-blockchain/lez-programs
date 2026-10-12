#!/usr/bin/env python3
"""Exercise the deployment and Logos modules on an isolated standalone sequencer.

Build prerequisites with make setup-devnet-tools and the runbook's Nix commands.
This is an opt-in live test; it never connects to a non-loopback sequencer.
"""

import argparse
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import deploy_stablecoin as deployment

ROOT = Path(__file__).resolve().parent.parent
ONE = 10**27


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait_for(probe, message, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except (OSError, deployment.DeploymentError, subprocess.SubprocessError, ValueError):
            pass
        time.sleep(0.2)
    raise RuntimeError(message)


def stage_module(source, name, destination):
    """Use built libraries and emit the core module's discovery manifest."""
    source = Path(source).resolve()
    target = destination / name
    target.mkdir(parents=True)
    libraries = list((source / "lib").glob("*.so")) + list((source / "lib").glob("*.dylib"))
    require(libraries, f"No module libraries in {source}")
    for library in libraries:
        (target / library.name).symlink_to(library)
    plugins = [p for p in libraries if "plugin" in p.name]
    require(len(plugins) == 1, f"Expected one plugin in {source}")
    variant = "linux-amd64-dev" if sys.platform.startswith("linux") else "darwin-arm64-dev"
    manifest = {
        "name": name, "type": "core", "version": "0.1.0",
        "manifestVersion": "0.2.0", "main": {variant: plugins[0].name},
        "dependencies": [] if name == "lez_core" else ["lez_core"],
    }
    (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (target / "variant").write_text(variant + "\n")


class Journey:
    def __init__(self, args):
        self.args = args
        self.state = Path(args.state_dir).resolve()
        require(not self.state.exists() or not any(self.state.iterdir()), "Choose a fresh --state-dir; existing state is never deleted")
        self.state.mkdir(parents=True, exist_ok=True)
        self.cli_config = self.state / "logoscore"
        self.wallet_dir = self.state / "wallet"
        self.wallet_dir.mkdir()
        self.events = []
        self.processes = []
        self.logs = []
        self.rpc = deployment.Rpc({"sequencer_addr": f"http://127.0.0.1:{args.port}"})
        self.env = dict(
            os.environ, RISC0_DEV_MODE="1", RISC0_SKIP_BUILD="1",
            LEE_WALLET_HOME_DIR=str(self.wallet_dir),
            SEQUENCER_ADDR=self.rpc.connection["sequencer_addr"],
            DEPLOYMENT_DIR=str(self.state / "deployment"),
            WALLET_BIN=str(ROOT / "target/debug/wallet"),
            SPEL_BIN=str(ROOT / "target/debug/spel"),
        )
        for key in tuple(self.env):
            if key.startswith("LOGOSCORE_"):
                self.env.pop(key)
        self.env["LOGOSCORE_CONFIG_DIR"] = str(self.cli_config)
        # A disposable local test must not inherit a real network's addresses or
        # parameter overrides. The bootstrap chooses all bindings from this wallet.
        for key in (
            "ADMIN_ID", "FREEZE_AUTHORITY_ID", "COLLATERAL_DEFINITION_ID",
            "COLLATERAL_MINT_AUTHORITY_ID", "COLLATERAL_HOLDING_ID",
            "ORACLE_SOURCE_ID", "MARKET_PRICE_ORACLE_ID",
        ):
            self.env.pop(key, None)
        for key in deployment.NUMBERS | deployment.NAMES:
            self.env.pop(key.upper(), None)
        for program in deployment.PROGRAMS:
            self.env.pop(program.upper() + "_PROGRAM_ID", None)
            self.env.pop(program.upper() + "_PROGRAM_BIN", None)
        self.env["MINIMUM_MILLISECONDS_BETWEEN_RATE_UPDATES"] = "1"
        self.env["PATH"] = str(ROOT / "target/debug") + os.pathsep + self.env["PATH"]

    def cli(self, *arguments, allow_error=False):
        result = subprocess.run(
            [self.args.logoscore_bin, "--config-dir", str(self.cli_config), "--json", *arguments],
            env=self.env, cwd=ROOT, text=True, capture_output=True, timeout=120,
        )
        # Qt diagnostics belong to stderr. Machine-readable stdout is one JSON
        # envelope; a process-level failure is different from a module error.
        try:
            payload = json.loads(result.stdout)
        except ValueError as exc:
            raise RuntimeError(f"logoscore {arguments[:2]} did not return JSON (exit {result.returncode})") from exc
        if not allow_error:
            require(result.returncode == 0, f"logoscore {arguments[:2]} failed: {payload}")
        return payload

    def call(self, module, method, *arguments, expected_error=None):
        encoded = ["json:" + json.dumps(arg, separators=(",", ":")) for arg in arguments]
        envelope = self.cli("call", module, method, *encoded)
        require("result" in envelope, f"Missing call result for {method}: {envelope}")
        result = envelope["result"]
        if module != "lez_core":
            require(isinstance(result, dict), f"{method} returned a non-object")
            if expected_error:
                require(result.get("status") == "error" and result.get("error") == expected_error, f"{method}: expected {expected_error}, got {result}")
            else:
                require(result.get("status") == "ok", f"{method} failed: {result}")
        self.events.append({"module": module, "method": method, "arguments": arguments, "result": result})
        (self.state / "calls.json").write_text(json.dumps(self.events, indent=2) + "\n")
        return result

    def sync(self):
        height = self.rpc.call("getLastBlockId")
        result = self.call("lez_core", "sync_to_block", height)
        require(result == 0, f"Wallet sync failed: {result}")

    def tx(self, module, method, *arguments):
        self.sync()
        result = self.call(module, method, *arguments)
        require(result.get("transactionId"), f"{method} did not return a transaction ID")
        tx_hash = deployment.id_bytes(result["transactionId"]).hex()
        receipt = wait_for(lambda: self.rpc.call("getTransaction", [tx_hash]), f"{method} was not included")
        require(isinstance(receipt, list) and len(receipt) == 2, f"{method}: invalid receipt")
        self.events[-1]["includedInBlock"] = receipt[1]
        print(f"PASS {module}.{method} (block {receipt[1]})", flush=True)
        return result

    def core_tx(self, method, *arguments):
        self.sync()
        result = json.loads(self.call("lez_core", method, *arguments))
        require(result.get("success") and result.get("tx_hash"), f"{method} failed: {result}")
        receipt = wait_for(lambda: self.rpc.call("getTransaction", [result["tx_hash"]]), f"{method} was not included")
        self.events[-1]["includedInBlock"] = receipt[1]
        return result

    def spawn(self, command, name):
        log = (self.state / f"{name}.log").open("w")
        self.logs.append(log)
        process = subprocess.Popen(command, env=self.env, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        self.processes.append(process)
        return process

    def prepare(self):
        for binary in (self.args.sequencer_bin, ROOT / "target/debug/wallet", ROOT / "target/debug/spel"):
            require(Path(binary).is_file(), f"Missing {binary}; run make setup-devnet-tools")
        metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT))
        core = next(p for p in metadata["packages"] if p["name"] == "lee_core")
        upstream = next(p for p in Path(core["manifest_path"]).parents if (p / "lez/sequencer/service/configs/debug/sequencer_config.json").is_file())
        config = json.loads((upstream / "lez/sequencer/service/configs/debug/sequencer_config.json").read_text())
        config.update(home=str(self.state / "sequencer"), block_create_timeout="500ms", retry_pending_blocks_timeout="500ms", metrics_address=None, cross_zone=None, genesis=[])
        config["bedrock_config"]["node_url"] = "http://127.0.0.1:9"
        config["bedrock_config"]["channel_id"] = hashlib.sha256(str(self.state).encode()).hexdigest()
        config_path = self.state / "sequencer_config.json"
        config_path.write_text(json.dumps(config, indent=2) + "\n")
        # Refuse an occupied port instead of accidentally testing another chain.
        import socket
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", self.args.port))
        self.spawn([self.args.sequencer_bin, str(config_path), "--port", str(self.args.port), "--listen-address", "127.0.0.1"], "sequencer")
        wait_for(lambda: self.rpc.call("getLastBlockId") is not None, "Standalone sequencer did not start")
        wallet_config = {
            "sequencers": [self.rpc.connection], "seq_poll_timeout": "200ms",
            "seq_tx_poll_max_blocks": 120, "seq_poll_max_retries": 10,
            "seq_block_poll_max_amount": 100, "calibration_limit": 100,
        }
        (self.wallet_dir / "wallet_config.json").write_text(json.dumps(wallet_config, indent=2) + "\n")
        # New-wallet stdout contains a recovery phrase. Capture and discard it;
        # this disposable wallet is never an operator's existing wallet.
        created = subprocess.run([self.env["WALLET_BIN"], "account", "new", "public", "--label", "devnet-admin"], input="devnet-only\n", text=True, capture_output=True, env=self.env, cwd=ROOT, timeout=120)
        require(created.returncode == 0, "Disposable wallet creation failed")
        for label in ("bootstrap", "bootstrap-resume"):
            with (self.state / f"{label}.log").open("w") as log:
                subprocess.run([sys.executable, str(ROOT / "scripts/deploy_stablecoin.py")], env=self.env, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=600)
            manifest = json.loads((self.state / "deployment/deployment.json").read_text())
            require(manifest.get("complete"), f"{label} did not complete")
            if label == "bootstrap":
                previous = manifest["transactions"]
            else:
                require(manifest["transactions"] == previous, "Resume submitted new bootstrap transactions")
        self.manifest = manifest
        self.accounts = manifest["accounts"]
        for program in ("stablecoin", "token"):
            self.env[program.upper() + "_PROGRAM_ID"] = manifest["programs"][program]["id"]
        modules = self.state / "modules"
        for name, source in (("lez_core", self.args.wallet_module), ("token_module", self.args.token_module), ("stablecoin_module", self.args.stablecoin_module)):
            stage_module(source, name, modules)
        self.spawn([self.args.logoscore_bin, "--config-dir", str(self.cli_config), "-D", "-m", str(modules), "--persistence-path", str(self.state / "module-data")], "logoscore")
        wait_for(lambda: self.cli("status", allow_error=True).get("daemon", {}).get("status") == "running", "Isolated logoscore daemon did not start")
        for name in ("lez_core", "token_module", "stablecoin_module"):
            self.cli("load-module", name)
        require(self.call("lez_core", "open", str(self.wallet_dir / "wallet_config.json"), str(self.wallet_dir / "storage.json"), str(self.wallet_dir / "statistics.json")) == 0, "Module could not open disposable wallet")
        self.sync()
        # Repeated transactions require a claimed identity, not merely a key.
        # The deployment initializes its fresh admin before protocol use.
        admin_account = self.rpc.call("getAccount", [self.accounts["admin"]])
        owner = deployment.id_bytes(admin_account["program_owner"]).hex()
        require(owner != "0" * 64, "Bootstrap left the administrative identity unclaimed")
        exports = {
            "LOGOSCORE_CONFIG_DIR": str(self.cli_config), "LEE_WALLET_HOME_DIR": str(self.wallet_dir),
            "SEQUENCER_ADDR": self.rpc.connection["sequencer_addr"],
            "DEPLOYMENT_DIR": str(self.state / "deployment"),
            "STABLECOIN_PROGRAM_ID": self.env["STABLECOIN_PROGRAM_ID"],
            "TOKEN_PROGRAM_ID": self.env["TOKEN_PROGRAM_ID"],
        }
        exports.update({role.upper() + "_ID": value for role, value in self.accounts.items()})
        (self.state / "runtime.env").write_text("\n".join(f"export {key}={shlex.quote(value)}" for key, value in exports.items()) + "\n")
        (self.state / "pids.json").write_text(json.dumps([p.pid for p in self.processes]) + "\n")
        print("PASS deployment, read-only resume, isolated module loading", flush=True)

    def journey(self):
        owner = self.accounts["admin"]
        holding = self.accounts["collateral_holding"]
        request = {"ownerId": owner, "positionNonce": "0"}
        values = self.manifest["settings"]
        self.call("stablecoin_module", "initializeProgram", {
            "adminId": owner, "freezeAuthorityId": owner,
            "collateralDefinitionId": self.accounts["collateral_definition"],
            "marketPriceOracleId": self.accounts["market_price_oracle"],
            "initialStabilityFeePerMillisecond": values["initial_stability_fee_per_millisecond"],
            "initialControllerProportionalGain": "0", "initialControllerIntegralGain": "0",
            "initialMinimumCollateralizationRatio": values["initial_minimum_collateralization_ratio"],
            "minimumMillisecondsBetweenRateUpdates": values["minimum_milliseconds_between_rate_updates"],
            "maximumOraclePriceAgeMilliseconds": values["maximum_oracle_price_age_milliseconds"],
            "initialRedemptionPrice": values["initial_redemption_price"],
            "stablecoinName": values["stablecoin_name"],
        }, expected_error="already_initialized")
        for method in ("programInfo", "protocolParameters", "stabilityFeeAccumulator", "redemptionPriceState", "currentGlobalState", "redemptionRateUpdateQuote"):
            self.call("stablecoin_module", method)
        self.tx("stablecoin_module", "accrueStabilityFee", owner)
        self.tx("stablecoin_module", "updateRedemptionRate", owner)
        self.tx("stablecoin_module", "refreshGlobals", owner)
        # The destination is created through the shared wallet module, then
        # initialized through token_module; no raw transaction is assembled here.
        stable_holding = self.call("lez_core", "create_account_public")
        stable_holding = deployment.account_id(stable_holding)
        with (self.state / "runtime.env").open("a") as exports:
            exports.write(f"export STABLECOIN_HOLDING_ID={shlex.quote(stable_holding)}\n")
        self.tx("token_module", "initializeHolding", self.accounts["stablecoin_definition"], stable_holding)
        self.tx("stablecoin_module", "openPosition", dict(request, userCollateralHoldingId=holding, initialCollateralAmount="1000"))
        self.tx("stablecoin_module", "depositCollateral", dict(request, userCollateralHoldingId=holding, amount="500"))
        self.tx("stablecoin_module", "generateDebt", dict(request, userStablecoinHoldingId=stable_holding, amount="100"))
        health = self.call("stablecoin_module", "positionHealth", request)
        require(health["collateralAmount"] == "1500" and health["nominalDebt"] == "100" and health["isCollateralized"], "Unexpected post-mint health")
        minted = self.call("token_module", "inspectHolding", stable_holding)
        require(minted["holding"]["balanceRaw"] == "100", "Minted holding has the wrong balance")
        definition = self.call("token_module", "inspectDefinition", self.accounts["stablecoin_definition"])
        require(definition["definition"]["totalSupplyRaw"] == "100", "Token supply differs from minted debt")
        self.tx("stablecoin_module", "withdrawCollateral", dict(request, userCollateralHoldingId=holding, amount="100"))
        self.tx("stablecoin_module", "freeze", {"freezeAuthorityId": owner})
        self.call("stablecoin_module", "generateDebt", dict(request, userStablecoinHoldingId=stable_holding, amount="1"), expected_error="protocol_frozen")
        self.call("stablecoin_module", "positionHealth", request)
        self.tx("stablecoin_module", "depositCollateral", dict(request, userCollateralHoldingId=holding, amount="0"))
        self.tx("stablecoin_module", "repayDebt", dict(request, userStablecoinHoldingId=stable_holding, amount="100"))
        self.tx("stablecoin_module", "unfreeze", {"freezeAuthorityId": owner})
        self.tx("stablecoin_module", "withdrawCollateral", dict(request, userCollateralHoldingId=holding, amount="1400"))
        settled = self.call("stablecoin_module", "positionHealth", request)
        require(settled["collateralAmount"] == "0" and settled["nominalDebt"] == "0", "Position did not settle")
        self.tx("stablecoin_module", "closePosition", request)
        self.tx("stablecoin_module", "setStabilityFeePerMillisecond", {"adminId": owner, "newRate": str(ONE)})
        self.tx("stablecoin_module", "setMinimumCollateralizationRatio", {"adminId": owner, "newRatio": str(3 * ONE // 2)})
        self.tx("stablecoin_module", "setControllerGains", {"adminId": owner, "newProportionalGain": "0", "newIntegralGain": "0"})
        self.tx("stablecoin_module", "setMarketPriceOracle", {"adminId": owner, "newOracleId": self.accounts["market_price_oracle"]})
        self.tx("stablecoin_module", "setFreezeAuthority", {"adminId": owner, "newFreezeAuthorityId": owner})
        self.tx("stablecoin_module", "setAdmin", {"adminId": owner, "newAdminId": owner})
        # Force oracle expiry through an authorized timing change; no account
        # bytes, clock values or external feed are forged for the negative case.
        self.tx("stablecoin_module", "setTimingParameters", {"adminId": owner, "newMinimumMillisecondsBetweenRateUpdates": "1", "newMaximumOraclePriceAgeMilliseconds": "1"})
        quote = self.call("stablecoin_module", "redemptionRateUpdateQuote")
        require(not quote["canSubmit"], "Expired oracle was accepted")
        self.call("stablecoin_module", "updateRedemptionRate", owner, expected_error="oracle_stale")
        self.tx("stablecoin_module", "refreshGlobals", owner)
        self.tx("stablecoin_module", "setTimingParameters", {"adminId": owner, "newMinimumMillisecondsBetweenRateUpdates": "1", "newMaximumOraclePriceAgeMilliseconds": "900000"})
        self.call("stablecoin_module", "redemptionRateUpdateQuote")
        replacement = self.call("lez_core", "create_account_public")
        self.core_tx("register_public_account", replacement)
        self.tx("stablecoin_module", "setAdmin", {"adminId": owner, "newAdminId": replacement})
        self.call("stablecoin_module", "setMinimumCollateralizationRatio", {"adminId": owner, "newRatio": str(3 * ONE // 2)}, expected_error="admin_mismatch")
        self.tx("stablecoin_module", "setFreezeAuthority", {"adminId": replacement, "newFreezeAuthorityId": replacement})
        self.call("stablecoin_module", "freeze", {"freezeAuthorityId": owner}, expected_error="freeze_authority_mismatch")
        self.tx("stablecoin_module", "freeze", {"freezeAuthorityId": replacement})
        self.tx("stablecoin_module", "unfreeze", {"freezeAuthorityId": replacement})
        self.tx("stablecoin_module", "setFreezeAuthority", {"adminId": replacement, "newFreezeAuthorityId": owner})
        self.tx("stablecoin_module", "setAdmin", {"adminId": replacement, "newAdminId": owner})
        definition = self.call("token_module", "inspectDefinition", self.accounts["stablecoin_definition"])
        repaid = self.call("token_module", "inspectHolding", stable_holding)
        require(definition["definition"]["totalSupplyRaw"] == "0" and repaid["holding"]["balanceRaw"] == "0", "Repayment did not burn both holding balance and supply")
        self.call("lez_core", "save")
        public_header = (ROOT / "modules/stablecoin/src/stablecoin_module_impl.h").read_text().split("private:", 1)[0]
        public_methods = set(re.findall(r"^    LogosMap (\w+)\(", public_header, re.MULTILINE))
        covered = {event["method"] for event in self.events if event["module"] == "stablecoin_module"}
        require(public_methods <= covered, f"Uncovered public methods: {sorted(public_methods - covered)}")
        (self.state / "report.json").write_text(json.dumps({"status": "passed", "calls": len(self.events), "transactions": sum("includedInBlock" in event for event in self.events), "stablecoinMethods": sorted(covered), "standalone": True, "proofMode": "RISC0_DEV_MODE=1"}, indent=2) + "\n")
        print(f"PASS full position lifecycle, maintenance, admin, freeze and stale-oracle recovery; evidence: {self.state}", flush=True)

    def stop(self):
        try:
            self.cli("stop")
        except (RuntimeError, subprocess.SubprocessError):
            pass
        for process in reversed(self.processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        for log in self.logs:
            log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", required=True, help="Fresh disposable runtime/evidence directory")
    parser.add_argument("--port", type=int, default=3140)
    parser.add_argument("--sequencer-bin", default=str(ROOT / "target/debug/sequencer_service"))
    dev_cli = ROOT / "target/devnet-logoscore/bin/logoscore"
    parser.add_argument("--logoscore-bin", default=str(dev_cli) if dev_cli.is_file() else shutil.which("logoscore") or "logoscore")
    parser.add_argument("--wallet-module", required=True, help="Pinned lez_core Nix output")
    parser.add_argument("--token-module", required=True, help="Current token-module Nix output")
    parser.add_argument("--stablecoin-module", required=True, help="Current stablecoin-module Nix output")
    parser.add_argument("--keep-running", action="store_true", help="Leave this test's processes running after success for manual calls")
    args = parser.parse_args()
    journey = Journey(args)
    succeeded = False
    try:
        journey.prepare()
        journey.journey()
        succeeded = True
    finally:
        if not succeeded or not args.keep_running:
            journey.stop()


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError, deployment.DeploymentError, subprocess.SubprocessError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
