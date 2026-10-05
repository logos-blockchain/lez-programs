#!/usr/bin/env python3
"""Network-independent stablecoin bootstrap; invoked by setup-stablecoin-testnet.sh.

Only the Python standard library is required. SPEL owns instruction serialization
and account decoding; Rust examples own PDA derivation.
"""

import argparse
import base64
import fcntl
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
from contextlib import contextmanager
from pathlib import Path
from urllib import error, parse, request

ROOT = Path(__file__).resolve().parent.parent
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
ONE = 10**27
# Defaults, followed by inclusive on-chain bounds. Keep values as decimal strings
# at the CLI/manifest boundary; neither shell arithmetic nor floats are involved.
NUMBERS = {
    "initial_stability_fee_per_millisecond": (ONE, ONE, 2 * ONE),
    "initial_controller_proportional_gain": (0, -1000 * ONE, 1000 * ONE),
    "initial_controller_integral_gain": (0, -ONE, ONE),
    "initial_minimum_collateralization_ratio": (3 * ONE // 2, 11 * ONE // 10, 10 * ONE),
    "minimum_milliseconds_between_rate_updates": (300000, 1, 86400000),
    "maximum_oracle_price_age_milliseconds": (900000, 1, 86400000),
    "initial_redemption_price": (ONE, 1, 2**128 - 1),
    "collateral_supply": (10**21, 1, 2**128 - 1),
    "oracle_initial_price": (2**64, 1, 2**128 - 1),
    "oracle_window_milliseconds": (300000, 2048, 2**64 - 1),
}
NAMES = {
    "stablecoin_name": "Test Stablecoin",
    "collateral_name": "Stablecoin Collateral",
}
PROGRAMS = ("token", "twap_oracle", "stablecoin")
GLOBALS = {
    "protocol_parameters": "ProtocolParameters",
    "stability_fee_accumulator": "StabilityFeeAccumulator",
    "redemption_price_state": "RedemptionPriceState",
    "stablecoin_definition": "TokenDefinition",
    "stablecoin_master_holding": "TokenHolding",
}
TX_HASH = re.compile(r"(?:tx_hash:\s*|Transaction hash is\s*)([0-9a-fA-F]{64})\b")
ACCOUNT_FIELDS = {
    "admin_account_id",
    "freeze_authority_account_id",
    "stablecoin_definition_id",
    "collateral_definition_id",
    "market_price_oracle_id",
    "base_asset",
    "quote_asset",
    "source_id",
    "definition_id",
    "authority",
}


class DeploymentError(Exception):
    """An actionable bootstrap failure."""


class RpcError(DeploymentError):
    def __init__(self, method, code):
        super().__init__(f"{method}: JSON-RPC error {code}")
        self.code = code


def id_bytes(value):
    """Accept base58 IDs, ImageID hex, or RPC ProgramId u32 limbs."""
    if isinstance(value, str) and value.startswith("Public/"):
        value = value.removeprefix("Public/")
    if (
        isinstance(value, list)
        and len(value) == 8
        and all(type(n) is int and 0 <= n < 2**32 for n in value)
    ):
        return b"".join(n.to_bytes(4, "little") for n in value)
    if isinstance(value, str):
        if re.fullmatch(r"[0-9a-fA-F]{64}", value):
            return bytes.fromhex(value)
        if 32 <= len(value) <= 44 and all(c in ALPHABET for c in value):
            n = 0
            for c in value:
                n = n * 58 + ALPHABET.index(c)
            decoded = b"\0" * (len(value) - len(value.lstrip("1")))
            decoded += n.to_bytes((n.bit_length() + 7) // 8, "big")
            if len(decoded) == 32:
                return decoded
    raise DeploymentError(
        "Expected a 32-byte base58/hex ID or eight u32 ProgramId limbs"
    )


def account_id(value):
    raw = id_bytes(value)
    n = int.from_bytes(raw, "big")
    encoded = ""
    while n:
        n, digit = divmod(n, 58)
        encoded = ALPHABET[digit] + encoded
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + encoded


def settings(previous):
    values = {}
    for key, (default, minimum, maximum) in NUMBERS.items():
        value = str(os.environ.get(key.upper(), previous.get(key, default)))
        if not re.fullmatch(r"-?[0-9]+", value) or not minimum <= int(value) <= maximum:
            raise DeploymentError(
                f"{key.upper()} must be an integer in [{minimum}, {maximum}]"
            )
        values[key] = str(int(value))
    for key, default in NAMES.items():
        value = os.environ.get(key.upper(), previous.get(key, default))
        if not isinstance(value, str) or not value or "\0" in value:
            raise DeploymentError(
                f"{key.upper()} must be a nonempty string without NUL bytes"
            )
        values[key] = value
    if any(
        values[key] != "0"
        for key in (
            "initial_controller_proportional_gain", "initial_controller_integral_gain"
        )
    ):
        raise DeploymentError(
            "Controller gains must be zero: TWAP prices use Q64.64 but the protocol "
            "controller expects 10^27 fixed point; fix price conversion before enabling gains"
        )
    return values


def normalize_decoded_ids(value):
    """SPEL renders account_id fields as Public/<base58>; manifests use bare IDs."""
    if not isinstance(value, dict):
        return value
    return {
        key: account_id(item)
        if key in ACCOUNT_FIELDS and item is not None
        else normalize_decoded_ids(item)
        for key, item in value.items()
    }


def resolve_tool(name, root):
    """Resolve an explicit tool override, workspace build, or PATH command."""
    override = os.environ.get(f"{name.upper()}_BIN")
    if override:
        path = Path(override).expanduser()
        if not path.is_absolute():
            path = root / path
        # Rustup and other dispatchers use argv[0]; keep executable symlinks.
        path = path.absolute()
        if not path.is_file() or not os.access(path, os.X_OK):
            raise DeploymentError(
                f"{name.upper()}_BIN is not an executable file: {path}"
            )
        return str(path)
    if name in ("spel", "wallet"):
        workspace_binary = root / "target/debug" / name
        if workspace_binary.is_file() and os.access(workspace_binary, os.X_OK):
            return str(workspace_binary)
    path = shutil.which(name)
    if not path:
        raise DeploymentError(f"Required command not found: {name}")
    return path


def check_spel(root, spel):
    # An older CLI accepts --program-id but cannot decode the account_id type
    # in current IDLs. Exercise that capability offline before any transaction.
    address = "1" * 32
    data = (bytes(33) + (2**100).to_bytes(16, "little")).hex()
    result = subprocess.run(
        [
            spel,
            "--idl",
            str(root / "artifacts/token-idl.json"),
            "inspect",
            address,
            "--type",
            "TokenHolding",
            "--data",
            data,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    expected = {"Fungible": {"definition_id": address, "balance": str(2**100)}}
    try:
        valid = (
            result.returncode == 0
            and normalize_decoded_ids(json.loads(result.stdout)) == expected
        )
    except ValueError:
        valid = False
    if not valid:
        raise DeploymentError(
            "SPEL cannot decode current IDLs exactly. Build or install the SPEL revision in "
            "tools/idl-gen/Cargo.toml, then set SPEL_BIN or put its spel binary on PATH."
        )


@contextmanager
def instruction_transport(idl_path, name, arguments):
    """Generate an invocation-only schema for signed arguments unsupported by SPEL.

    RISC Zero serializes i128 with serialize_u128(v as u128). The pinned SPEL
    CLI only accepts u128, so transport the identical two's-complement bits.
    The canonical IDL and decoded account types retain their signed types.
    The native ABI equivalence is checked in stablecoin_core/tests/deploy_encoding.rs.
    """
    schema = json.loads(idl_path.read_text())
    instruction = next(
        item
        for item in schema["instructions"]
        if item["name"] == name.replace("-", "_")
    )
    signed = [item for item in instruction["args"] if item["type"] == "i128"]
    if not signed:
        yield idl_path, arguments
        return
    arguments = dict(arguments)
    for item in signed:
        value = int(arguments[item["name"]])
        if not -(2**127) <= value < 2**127:
            raise DeploymentError(f"{item['name']}: value outside i128 range")
        arguments[item["name"]] = str(value % 2**128)
        item["type"] = "u128"
    with tempfile.TemporaryDirectory(prefix="stablecoin-instruction-") as temporary:
        path = Path(temporary) / idl_path.name
        atomic_write(path, json.dumps(schema))
        yield path, arguments


def atomic_write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        try:
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)


def endpoint(connection):
    connection = dict(connection)
    url = parse.urlsplit(connection["sequencer_addr"])
    if url.scheme not in ("http", "https") or not url.hostname or url.fragment:
        raise DeploymentError("Sequencer must be an HTTP(S) URL without a fragment")
    if url.username is not None:
        connection["basic_auth"] = {
            "username": parse.unquote(url.username),
            "password": parse.unquote(url.password or ""),
        }
    connection["sequencer_addr"] = parse.urlunsplit(
        url._replace(netloc=url.netloc.rsplit("@", 1)[-1])
    )
    return connection


class NoRedirect(request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # A deployment must stay on the selected endpoint, including its auth.
        return None


class Rpc:
    def __init__(self, connection):
        self.connection = endpoint(connection)
        self.opener = request.build_opener(NoRedirect)

    def call(self, method, params=()):
        body = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": method, "params": list(params)}
        )
        headers = {"Content-Type": "application/json"}
        auth = self.connection.get("basic_auth")
        if auth:
            credentials = f"{auth['username']}:{auth.get('password') or ''}"
            headers["Authorization"] = (
                "Basic " + base64.b64encode(credentials.encode()).decode()
            )
        req = request.Request(self.connection["sequencer_addr"], body.encode(), headers)
        try:
            with self.opener.open(req, timeout=15) as response:
                result = json.load(response)
        except (error.URLError, OSError, ValueError) as exc:
            # Do not include URLs or credentials from the connection in errors.
            raise DeploymentError(
                f"{method}: sequencer request failed ({type(exc).__name__})"
            ) from exc
        if not isinstance(result, dict) or result.get("id") != 1:
            raise DeploymentError(f"{method}: malformed JSON-RPC response")
        if "error" in result:
            raise RpcError(method, result["error"].get("code"))
        if "result" not in result:
            raise DeploymentError(f"{method}: missing JSON-RPC result")
        return result["result"]


@contextmanager
def connected_wallet():
    home = Path(
        os.environ.get("LEE_WALLET_HOME_DIR", Path.home() / ".lee/wallet")
    ).resolve()
    config_path = home / "wallet_config.json"
    storage_path = home / "storage.json"
    if not config_path.is_file() or not storage_path.is_file():
        raise DeploymentError(
            "Current wallet needs wallet_config.json and storage.json; set LEE_WALLET_HOME_DIR"
        )
    config = json.loads(config_path.read_text())
    connections = config.get("sequencers", [])
    override = os.environ.get("SEQUENCER_ADDR")
    if override:
        connections = [
            next(
                (
                    c
                    for c in connections
                    if c["sequencer_addr"].rstrip("/") == override.rstrip("/")
                ),
                {"sequencer_addr": override},
            )
        ]
    if not connections:
        raise DeploymentError(
            "Wallet has no sequencers; configure one or set SEQUENCER_ADDR"
        )
    for connection in connections:
        rpc = Rpc(connection)
        try:
            channel = rpc.call("getChannelId")
        except DeploymentError:
            continue
        if not isinstance(channel, str) or not re.fullmatch(
            r"[0-9a-fA-F]{64}", channel
        ):
            raise DeploymentError("getChannelId returned an invalid network identity")
        break
    else:
        raise DeploymentError(
            "No configured sequencer is reachable (getChannelId failed)"
        )
    with (home / ".stablecoin-deploy.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise DeploymentError(
                "Another stablecoin deployment is using this wallet"
            ) from exc
        with tempfile.TemporaryDirectory(prefix="stablecoin-wallet-") as temporary:
            run_home = Path(temporary)
            # WalletCore writes storage.json in place, so account additions persist.
            # Connection statistics belong to this run, not the saved network.
            (run_home / "storage.json").symlink_to(storage_path)
            config["sequencers"] = [rpc.connection]
            atomic_write(run_home / "wallet_config.json", json.dumps(config))
            env = dict(os.environ, LEE_WALLET_HOME_DIR=str(run_home))
            yield rpc, channel.lower(), env


@contextmanager
def deployment_directory(root, channel):
    directory = Path(
        os.environ.get("DEPLOYMENT_DIR", root / "target/deployments/stablecoin" / channel)
    ).resolve()
    directory.mkdir(parents=True, exist_ok=True)
    # Different wallets can target the same deployment. Lock before loading it.
    with (directory / ".deployment.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise DeploymentError(
                "Another process is using this deployment directory"
            ) from exc
        yield directory


def deployment_hash(binary):
    # Pinned lee::ProgramDeploymentTransaction hashes its Borsh encoding:
    # one Vec<u8> (u32 little-endian length followed by the program bytes).
    bytecode = binary.read_bytes()
    return hashlib.sha256(len(bytecode).to_bytes(4, "little") + bytecode).hexdigest()


class Deployment:
    def __init__(self, root, rpc, channel, env, directory):
        self.root, self.rpc, self.env = root, rpc, env
        self.spel = resolve_tool("spel", root)
        self.wallet = resolve_tool("wallet", root)
        self.cargo = resolve_tool("cargo", root)
        self.make = resolve_tool("make", root)
        self.directory = directory
        self.manifest_path = self.directory / "deployment.json"
        self.manifest = (
            json.loads(self.manifest_path.read_text())
            if self.manifest_path.exists()
            else {
                "version": 1,
                "channelId": channel,
                "programs": {},
                "accounts": {},
                "transactions": {},
            }
        )
        if (
            self.manifest.get("version") != 1
            or self.manifest.get("channelId") != channel
        ):
            raise DeploymentError(
                "Deployment manifest belongs to another network or version; use a different DEPLOYMENT_DIR"
            )
        self.values = settings(self.manifest.get("settings", {}))
        # Once a transaction can have used these values, they describe bootstrap
        # history, not a request to reconfigure mutable on-chain state. Older
        # manifests have no explicit lock marker, but do record transactions.
        if self.manifest.get("settingsLocked") or (
            self.manifest.get("settings") and self.manifest["transactions"]
        ):
            for key, value in self.manifest.get("settings", {}).items():
                if self.values[key] != value:
                    raise DeploymentError(
                        f"{key.upper()}: bootstrap settings cannot change after submission; "
                        "saved settings were not changed"
                    )
            self.manifest["settingsLocked"] = True
        self.programs = self.manifest["programs"]
        self.accounts = self.manifest["accounts"]
        self.transactions = self.manifest["transactions"]
        self.idls = {name: root / f"artifacts/{name}-idl.json" for name in PROGRAMS}
        self.inspected_binaries = set()

    def checkpoint(self):
        atomic_write(self.manifest_path, json.dumps(self.manifest, indent=2) + "\n")

    def capture(self, command):
        result = subprocess.run(
            [str(x) for x in command],
            cwd=self.root,
            env=self.env,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode:
            raise DeploymentError(
                f"{command[0]} failed ({result.returncode}):\n{result.stderr}{result.stdout}"
            )
        return result.stdout

    def run(self, command, step=None):
        """Stream output and checkpoint a submitted hash even if confirmation fails."""
        command = [str(x) for x in command]
        print(f"==> {step or command[0]}", flush=True)
        if step:
            self.manifest["settings"] = dict(self.values)
            self.manifest["settingsLocked"] = True
            self.checkpoint()
        with subprocess.Popen(
            command,
            cwd=self.root,
            env=self.env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        ) as process:
            try:
                for line in process.stdout:
                    print(line, end="", flush=True)
                    match = TX_HASH.search(line)
                    if step and match:
                        self.transactions[step] = {
                            "hash": match[1].lower(),
                            "confirmed": False,
                        }
                        self.checkpoint()
                code = process.wait()
            except BaseException:
                process.terminate()
                process.wait()
                raise
        if code:
            raise DeploymentError(
                f"{step or command[0]} exited with status {code}; confirmed progress is saved"
            )
        if step:
            self.confirm(step)

    def confirm(self, step):
        tx = self.transactions.get(step)
        receipt = self.rpc.call("getTransaction", [tx["hash"]]) if tx else None
        if not isinstance(receipt, list) or len(receipt) != 2:
            raise DeploymentError(
                f"{step}: transaction not confirmed; inspect its status before retrying"
            )
        tx["confirmed"] = True
        self.checkpoint()

    def resolve_programs(self):
        try:
            discovered = self.rpc.call("getProgramIds")
        except RpcError as exc:
            if exc.code != -32601:
                raise
            discovered = {}
        available = {id_bytes(pid).hex() for pid in discovered.values()}
        selected = {}
        binaries = {}
        for name in PROGRAMS:
            saved = self.programs.get(name, {})
            explicit = os.environ.get(f"{name.upper()}_PROGRAM_ID")
            pid = id_bytes(explicit).hex() if explicit else saved.get("id")
            if saved and pid != saved["id"]:
                raise DeploymentError(
                    f"{name}: program differs from manifest; use a different DEPLOYMENT_DIR"
                )
            digest = hashlib.sha256(self.idls[name].read_bytes()).hexdigest()
            if saved and saved.get("idlSha256") != digest:
                raise DeploymentError(
                    f"{name}: IDL changed since deployment; select compatible IDs with a new DEPLOYMENT_DIR"
                )
            selected[name] = {**saved, "id": pid, "idlSha256": digest}
            configured_bin = os.environ.get(f"{name.upper()}_PROGRAM_BIN")
            if (
                configured_bin
                or not pid
                or (
                    saved
                    and not saved.get("ready")
                    and f"deploy-{name}" not in self.transactions
                )
            ):
                binaries[name] = (
                    Path(configured_bin).resolve()
                    if configured_bin
                    else self.root / f"target/guest/{name}.bin"
                )
        if binaries:
            self.run([self.make, "build-programs"])
            for name, binary in binaries.items():
                # Both older and pinned SPEL accept inspect <binary>. Parse its
                # ImageID, never the differently formatted u32 program-id line.
                output = self.capture([self.spel, "inspect", binary])
                matches = [
                    pid.lower() for pid in re.findall(r"\b[0-9a-fA-F]{64}\b", output)
                ]
                if len(set(matches)) != 1:
                    raise DeploymentError(
                        f"Could not extract one ImageID from {binary}"
                    )
                pid = matches[0].lower()
                if selected[name]["id"] and selected[name]["id"] != pid:
                    raise DeploymentError(
                        f"{name}: configured program ID does not match binary"
                    )
                selected[name].update(id=pid, binary=str(binary))
                self.inspected_binaries.add(name)
        for name in PROGRAMS:
            saved = self.programs.get(name, {})
            entry = selected[name]
            self.programs[name] = entry
            step = f"deploy-{name}"
            if saved and saved.get("ready"):
                if step in self.transactions:
                    self.confirm(step)  # Detect stale receipts after a chain reset.
                print(f"Reuse {name}: {entry['id']}")
            elif step in self.transactions:
                self.confirm(step)
            elif (
                os.environ.get(f"{name.upper()}_PROGRAM_ID") or entry["id"] in available
            ):
                print(f"Reuse {name}: {entry['id']}")
            else:
                self.checkpoint()
                tx_hash = deployment_hash(Path(entry["binary"]))
                receipt = self.rpc.call("getTransaction", [tx_hash])
                if receipt is not None:
                    # getProgramIds only lists builtins. A committed deployment
                    # of these exact bytes proves reuse without relying on CLI
                    # error text (the pinned wallet discards underlying errors).
                    self.transactions[step] = {"hash": tx_hash, "confirmed": False}
                    self.confirm(step)
                    print(f"Reuse {name}: {entry['id']}")
                else:
                    self.run([self.wallet, "deploy-program", entry["binary"]], step)
            entry["ready"] = True
            self.checkpoint()

    def example(self, package, name, *args):
        command = [
            self.cargo,
            "run",
            "-q",
            "-p",
            package,
            "--example",
            name,
            "--",
            *args,
        ]
        result = subprocess.run(
            command,
            cwd=self.root,
            env=dict(self.env, RISC0_SKIP_BUILD="1", RISC0_DEV_MODE="1"),
            text=True,
            stdout=subprocess.PIPE,
            check=True,
        )
        return {
            key: account_id(value)
            for key, value in (line.split() for line in result.stdout.splitlines())
        }

    def raw_account(self, address):
        raw = self.rpc.call("getAccount", [address])
        # Reading an absent account succeeds with Account::default(); RPC errors
        # must never be mistaken for an uninitialized target.
        owner = id_bytes(raw["program_owner"]).hex()
        data = bytes(raw["data"])
        empty = (
            owner == "0" * 64
            and not data
            and int(raw["balance"]) == 0
            and int(raw["nonce"]) == 0
        )
        return raw, owner, data, empty

    def decode(self, role, type_name, program, owner=None):
        address = self.accounts[role]
        _, actual_owner, data, empty = self.raw_account(address)
        if empty:
            return None
        if owner is not None and actual_owner != owner:
            raise DeploymentError(f"{role}: unexpected program owner")
        output = self.capture(
            [
                self.spel,
                "--idl",
                self.idls[program],
                "inspect",
                address,
                "--type",
                type_name,
                "--data",
                data.hex(),
            ]
        )
        return normalize_decoded_ids(json.loads(output))

    def bind_account(self, role, value):
        value = account_id(value)
        if self.accounts.get(role, value) != value:
            raise DeploymentError(
                f"{role}: account differs from manifest; use a different DEPLOYMENT_DIR"
            )
        self.accounts[role] = value
        self.checkpoint()
        return value

    def wallet_account(self, role):
        explicit = os.environ.get(f"{role.upper()}_ID")
        if explicit:
            return self.bind_account(role, explicit)
        if role in self.accounts:
            return self.accounts[role]
        label = f"stablecoin-{self.programs['stablecoin']['id'][:12]}-{role.replace('_', '-')}"
        storage = json.loads(
            (Path(self.env["LEE_WALLET_HOME_DIR"]) / "storage.json").read_text()
        )
        if label not in storage.get("labels", {}):
            self.run([self.wallet, "account", "new", "public", "--label", label])
        output = self.capture([self.wallet, "account", "id", "--account-id", label])
        candidates = [
            line.strip()
            for line in output.splitlines()
            if re.fullmatch(
                r"[1-9A-HJ-NP-Za-km-z]{32,44}|[0-9a-fA-F]{64}", line.strip()
            )
        ]
        if len(candidates) != 1:
            raise DeploymentError(
                f"wallet account id did not return one account for {label}"
            )
        return self.bind_account(role, candidates[0])

    def external_account(self, role):
        external = self.manifest.setdefault("externalAccounts", [])
        if os.environ.get(f"{role.upper()}_ID") and role not in external:
            external.append(role)
            self.checkpoint()
        return role in external

    def instruction(self, program, name, accounts, arguments):
        # All calls are public, so reused program IDs do not need local binaries.
        with instruction_transport(self.idls[program], name, arguments) as (
            idl,
            values,
        ):
            command = [
                self.spel,
                "--idl",
                idl,
                "--program-id",
                self.programs[program]["id"],
                name,
            ]
            for key, value in {**accounts, **values}.items():
                command.extend(["--" + key.replace("_", "-"), str(value)])
            self.run(command, name)

    @staticmethod
    def require_fields(actual, expected, role):
        if not isinstance(actual, dict) or any(
            str(actual.get(k)) != str(v) for k, v in expected.items()
        ):
            raise DeploymentError(
                f"{role}: on-chain state conflicts with requested configuration"
            )

    def bootstrap(self):
        for step in self.transactions:
            self.confirm(step)
        pids = {name: self.programs[name]["id"] for name in PROGRAMS}
        for key, value in self.example(
            "stablecoin_program", "stablecoin_pdas", pids["stablecoin"]
        ).items():
            self.bind_account(key, value)
        _, _, clock_data, empty = self.raw_account(self.accounts["clock"])
        if (
            empty
            or len(clock_data) != 16
            or int.from_bytes(clock_data[8:], "little") == 0
        ):
            raise DeploymentError(
                "Canonical CLOCK_01 account must contain an initialized nonzero timestamp"
            )
        states = self.global_states()
        if any(state is not None for state in states.values()):
            if any(state is None for state in states.values()):
                raise DeploymentError(
                    "Stablecoin global accounts are partially initialized"
                )
            if not self.manifest.get("settingsLocked"):
                self.manifest["settingsOrigin"] = "adopted"
            if self.manifest.get("settingsOrigin") == "adopted":
                for key in (
                    "COLLATERAL_SUPPLY", "ORACLE_INITIAL_PRICE", "ORACLE_WINDOW_MILLISECONDS"
                ):
                    if key in os.environ:
                        raise DeploymentError(
                            f"{key}: original bootstrap value is unknown for an adopted protocol; "
                            "omit this creation-only setting"
                        )
            # Existing protocol supplies defaults for its immutable bindings;
            # explicit overrides still have to match. No new accounts are needed.
            params = states["protocol_parameters"]
            bindings = {}
            for role, field in (
                ("admin", "admin_account_id"),
                ("freeze_authority", "freeze_authority_account_id"),
                ("collateral_definition", "collateral_definition_id"),
                ("market_price_oracle", "market_price_oracle_id"),
            ):
                value = os.environ.get(f"{role.upper()}_ID", params[field])
                if account_id(value) != account_id(params[field]):
                    raise DeploymentError(
                        f"{role}: on-chain state conflicts with requested configuration"
                    )
                bindings[role] = value
            for role, value in bindings.items():
                self.bind_account(role, value)
            self.verify_globals(states, fresh=False)
            self.verify_collateral()
            self.verify_oracle()
            return

        if self.manifest.get("settingsOrigin") == "adopted":
            raise DeploymentError(
                "Previously adopted protocol is missing from this sequencer"
            )
        admin = self.wallet_account("admin")
        self.bind_account(
            "freeze_authority",
            os.environ.get(
                "FREEZE_AUTHORITY_ID", self.accounts.get("freeze_authority", admin)
            ),
        )
        self.wallet_account("collateral_definition")
        external_collateral = self.external_account("collateral_definition")
        collateral = self.decode(
            "collateral_definition", "TokenDefinition", "token", pids["token"]
        )
        if collateral is None:
            if external_collateral:
                raise DeploymentError(
                    "Configured collateral definition is uninitialized"
                )
            holding = self.wallet_account("collateral_holding")
            if not self.raw_account(holding)[3]:
                raise DeploymentError(
                    "Collateral holding is occupied but definition is uninitialized"
                )
            self.instruction(
                "token",
                "new-fungible-definition",
                {
                    "definition_target_account": self.accounts["collateral_definition"],
                    "holding_target_account": holding,
                },
                {
                    "name": self.values["collateral_name"],
                    "total_supply": self.values["collateral_supply"],
                    "mint_authority": admin,
                },
            )
        self.verify_collateral()

        if os.environ.get("MARKET_PRICE_ORACLE_ID"):
            self.bind_account(
                "market_price_oracle", os.environ["MARKET_PRICE_ORACLE_ID"]
            )
        external_oracle = self.external_account("market_price_oracle")
        if not external_oracle:
            source = self.wallet_account("oracle_source")
            pdas = self.example(
                "twap_oracle_program",
                "twap_oracle_pdas",
                pids["twap_oracle"],
                source,
                self.values["oracle_window_milliseconds"],
            )
            self.bind_account("market_price_oracle", pdas["oracle_price_account"])
        oracle = self.decode(
            "market_price_oracle",
            "OraclePriceAccount",
            "twap_oracle",
            pids["twap_oracle"],
        )
        if oracle is None:
            if external_oracle:
                raise DeploymentError("Configured market price oracle is uninitialized")
            self.instruction(
                "twap_oracle",
                "create-oracle-price-account",
                {
                    "oracle_price_account": self.accounts["market_price_oracle"],
                    "price_source": self.accounts["oracle_source"],
                    "clock": self.accounts["clock"],
                },
                {
                    "base_asset": self.accounts["stablecoin_definition"],
                    "quote_asset": self.accounts["collateral_definition"],
                    "initial_price": self.values["oracle_initial_price"],
                    "window_duration": self.values["oracle_window_milliseconds"],
                },
            )
        self.verify_oracle()
        arguments = {
            key: value
            for key, value in self.values.items()
            if key.startswith("initial_")
            or key
            in (
                "minimum_milliseconds_between_rate_updates",
                "maximum_oracle_price_age_milliseconds",
                "stablecoin_name",
            )
        }
        arguments["freeze_authority_account_id"] = self.accounts["freeze_authority"]
        ordered_accounts = {
            role: self.accounts[role]
            for role in (
                "admin",
                *GLOBALS,
                "collateral_definition",
                "market_price_oracle",
                "clock",
            )
        }
        self.instruction(
            "stablecoin", "initialize-program", ordered_accounts, arguments
        )
        self.verify_globals(self.global_states(), fresh=True)

    def verify_collateral(self):
        token = self.decode(
            "collateral_definition",
            "TokenDefinition",
            "token",
            self.programs["token"]["id"],
        )
        if not isinstance(token, dict) or "Fungible" not in token:
            raise DeploymentError(
                "Collateral must be an initialized fungible token definition"
            )
        if "collateral_holding" in self.accounts:
            self.require_fields(
                token["Fungible"],
                {
                    "name": self.values["collateral_name"],
                    "authority": self.accounts["admin"],
                },
                "collateral_definition",
            )
            holding = self.decode(
                "collateral_holding",
                "TokenHolding",
                "token",
                self.programs["token"]["id"],
            )
            self.require_fields(
                (holding or {}).get("Fungible"),
                {"definition_id": self.accounts["collateral_definition"]},
                "collateral_holding",
            )

    def verify_oracle(self):
        oracle = self.decode(
            "market_price_oracle",
            "OraclePriceAccount",
            "twap_oracle",
            self.programs["twap_oracle"]["id"],
        )
        self.require_fields(
            oracle,
            {
                "base_asset": self.accounts["stablecoin_definition"],
                "quote_asset": self.accounts["collateral_definition"],
            },
            "market_price_oracle",
        )
        if int(oracle["price"]) <= 0 or int(oracle["timestamp"]) <= 0:
            raise DeploymentError(
                "Market price oracle must contain a nonzero price and timestamp"
            )

    def global_states(self):
        return {
            role: self.decode(
                role,
                type_name,
                "stablecoin",
                self.programs[
                    "token"
                    if role in ("stablecoin_definition", "stablecoin_master_holding")
                    else "stablecoin"
                ]["id"],
            )
            for role, type_name in GLOBALS.items()
        }

    def verify_globals(self, states, fresh):
        fields = {
            "admin_account_id": self.accounts["admin"],
            "freeze_authority_account_id": self.accounts["freeze_authority"],
            "stablecoin_definition_id": self.accounts["stablecoin_definition"],
            "collateral_definition_id": self.accounts["collateral_definition"],
            "market_price_oracle_id": self.accounts["market_price_oracle"],
        }
        for key, value in self.values.items():
            if key.startswith("initial_") and key != "initial_redemption_price":
                if fresh or key.upper() in os.environ:
                    fields[key.removeprefix("initial_")] = value
            elif key in (
                "minimum_milliseconds_between_rate_updates",
                "maximum_oracle_price_age_milliseconds",
            ) and (fresh or key.upper() in os.environ):
                fields[key] = value
        self.require_fields(
            states["protocol_parameters"], fields, "protocol_parameters"
        )
        definition = (states["stablecoin_definition"] or {}).get("Fungible")
        fields = {"authority": self.accounts["stablecoin_definition"]}
        if fresh or "STABLECOIN_NAME" in os.environ:
            fields["name"] = self.values["stablecoin_name"]
        if fresh:
            fields["total_supply"] = "0"
        self.require_fields(definition, fields, "stablecoin_definition")
        self.require_fields(
            (states["stablecoin_master_holding"] or {}).get("Fungible"),
            {"definition_id": self.accounts["stablecoin_definition"], "balance": "0"},
            "stablecoin_master_holding",
        )
        if fresh:
            self.require_fields(
                states["stability_fee_accumulator"],
                {"accumulated_rate_at_last_accrual": str(ONE)},
                "stability_fee_accumulator",
            )
            self.require_fields(
                states["redemption_price_state"],
                {
                    "redemption_price_at_last_update": self.values[
                        "initial_redemption_price"
                    ],
                    "redemption_rate_per_millisecond": str(ONE),
                    "controller_integral_term": "0",
                },
                "redemption_price_state",
            )
        elif "INITIAL_REDEMPTION_PRICE" in os.environ:
            self.require_fields(
                states["redemption_price_state"],
                {
                    "redemption_price_at_last_update": self.values[
                        "initial_redemption_price"
                    ]
                },
                "redemption_price_state",
            )
        self.manifest["verifiedState"] = states

    def finish(self):
        if self.manifest.get("settingsOrigin") == "adopted":
            # Current mutable state cannot establish original initialization
            # inputs. Expose verifiedState, not invented bootstrap defaults.
            self.manifest.pop("settings", None)
        else:
            self.manifest["settings"] = dict(self.values)
        self.manifest["settingsLocked"] = True
        self.manifest["complete"] = True
        self.checkpoint()
        exports = {
            f"{name.upper()}_PROGRAM_ID": entry["id"]
            for name, entry in self.programs.items()
        }
        exports.update(
            {f"{name.upper()}_ID": value for name, value in self.accounts.items()}
        )
        for name, entry in self.programs.items():
            if name in self.inspected_binaries:
                exports[f"{name.upper()}_PROGRAM_BIN"] = entry["binary"]
        lines = [
            f"unset {name.upper()}_PROGRAM_BIN"
            for name in PROGRAMS
            if name not in self.inspected_binaries
        ]
        lines.extend(
            f"export {key}={shlex.quote(value)}" for key, value in exports.items()
        )
        atomic_write(self.directory / "deployment.env", "\n".join(lines) + "\n")
        print(f"Setup complete. Manifest: {self.manifest_path}")
        print(f"Environment: {self.directory / 'deployment.env'}")
        print(
            "Oracle contains a seed price; ongoing oracle publication requires a separate keeper."
        )


def main():
    parser = argparse.ArgumentParser(
        prog="setup-stablecoin-testnet.sh",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""Environment (all optional):
  LEE_WALLET_HOME_DIR     Existing wallet (default: ~/.lee/wallet)
  SEQUENCER_ADDR          HTTP(S) endpoint for this run only
  DEPLOYMENT_DIR          Output/resume directory (default: target/deployments/stablecoin/<channel-id>)
  SPEL_BIN, WALLET_BIN, CARGO_BIN, MAKE_BIN  Optional explicit CLI paths
  {TOKEN,TWAP_ORACLE,STABLECOIN}_PROGRAM_ID   Reuse a compatible deployed program (base58 or hex)
  {TOKEN,TWAP_ORACLE,STABLECOIN}_PROGRAM_BIN  Binary to inspect after make build-programs
  ADMIN_ID, FREEZE_AUTHORITY_ID             Public account IDs (freeze authority defaults to admin)
  COLLATERAL_DEFINITION_ID                  Reuse initialized collateral
  COLLATERAL_HOLDING_ID, ORACLE_SOURCE_ID    Public accounts controlled by current wallet
  MARKET_PRICE_ORACLE_ID                    Reuse initialized TWAP price account

Numeric settings use exact decimal integers; protocol fixed point is 10^27,
oracle price is Q64.64 (2^64 = 1). Bootstrap disables interest/controller gains.
Nonzero controller gains are rejected until protocol price-unit conversion is fixed.
The seeded oracle does not provide a live market feed. Settings default to saved
manifest values on resume. Bootstrap settings are locked before the first
transaction; conflicting overrides are rejected without changing saved settings.
For adopted protocols, verifiedState records current state; historical bootstrap
settings are unknown and omitted from the manifest.
"""
        + "\n".join(
            f"  {key.upper()}={default}" for key, (default, _, _) in NUMBERS.items()
        )
        + "\n"
        + "\n".join(f"  {key.upper()}={default}" for key, default in NAMES.items())
        + "\n\nPrerequisites: python3, wallet, spel, cargo; Docker/BuildKit for fresh builds.\n"
        + "Build compatible wallet/SPEL binaries with: make setup-workspace-tools\n"
        + "Explicit IDs must implement the repository IDLs. Wallet unlock prompts remain interactive.\n"
        + "Example: SEQUENCER_ADDR=http://127.0.0.1:3040 make deploy-stablecoin",
    )
    parser.parse_args()
    settings({})  # Reject bad arguments before wallet or network work.
    for variable in (
        "ADMIN_ID",
        "FREEZE_AUTHORITY_ID",
        "COLLATERAL_DEFINITION_ID",
        "COLLATERAL_HOLDING_ID",
        "ORACLE_SOURCE_ID",
        "MARKET_PRICE_ORACLE_ID",
        *(name.upper() + "_PROGRAM_ID" for name in PROGRAMS),
    ):
        if os.environ.get(variable):
            try:
                id_bytes(os.environ[variable])
            except DeploymentError as exc:
                raise DeploymentError(f"{variable}: {exc}") from exc
    tools = {
        name: resolve_tool(name, ROOT) for name in ("wallet", "spel", "cargo", "make")
    }
    check_spel(ROOT, tools["spel"])
    with connected_wallet() as (rpc, channel, env):
        with deployment_directory(ROOT, channel) as directory:
            deployment = Deployment(ROOT, rpc, channel, env, directory)
            deployment.manifest["complete"] = False
            deployment.checkpoint()
            deployment.resolve_programs()
            deployment.bootstrap()
            deployment.finish()


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    try:
        main()
    except (
        DeploymentError,
        OSError,
        ValueError,
        KeyError,
        TypeError,
        subprocess.CalledProcessError,
    ) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        print(
            "Interrupted; confirmed progress is saved in deployment.json.",
            file=sys.stderr,
        )
        sys.exit(130)
