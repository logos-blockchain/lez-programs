#!/usr/bin/env python3
"""CLI doubles for the deployment subprocess tests. Never contacts a sequencer."""

import hashlib
import json
import os
import sys
from pathlib import Path

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
PROGRAMS = {"token": "11" * 32, "twap_oracle": "22" * 32, "stablecoin": "33" * 32}
ROLES = (
    "protocol_parameters",
    "stability_fee_accumulator",
    "redemption_price_state",
    "stablecoin_definition",
    "stablecoin_master_holding",
    "clock",
)


def address(label):
    raw = hashlib.sha256(label.encode()).digest()
    n, encoded = int.from_bytes(raw, "big"), ""
    while n:
        n, remainder = divmod(n, 58)
        encoded = ALPHABET[remainder] + encoded
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + encoded


def program_transaction(name):
    bytecode = name.encode()
    return hashlib.sha256(len(bytecode).to_bytes(4, "little") + bytecode).hexdigest()


def main():
    tool = Path(sys.argv[0]).name
    args = sys.argv[1:]
    state_path = Path(os.environ["FAKE_STATE"])
    state = json.loads(state_path.read_text())

    def save():
        state_path.write_text(json.dumps(state))

    def put(account, program, value):
        raw = bytes.fromhex(PROGRAMS[program])
        state["accounts"][account] = {
            "program_owner": [
                int.from_bytes(raw[i : i + 4], "little") for i in range(0, 32, 4)
            ],
            "balance": 0,
            "nonce": "0",
            "data": list(json.dumps(value).encode()),
        }

    def transaction(name):
        if state.get("fail_before") == name:
            print("Rejected before submission")
            save()
            sys.exit(1)

    def confirmed(name):
        tx_hash = (
            program_transaction(name.removeprefix("deploy-"))
            if name.startswith("deploy-")
            else hashlib.sha256(f"{name}-{len(state['calls'])}".encode()).hexdigest()
        )
        state["transactions"][tx_hash] = ["confirmed transaction", 1]
        save()
        if tool == "wallet":
            print(f"Transaction hash is {tx_hash}", flush=True)
            print("Transaction is included in block 1")
        else:
            print(f"tx_hash: {tx_hash}", flush=True)
            print("Transaction confirmed — included in a block.")
        if state.get("fail_after") == name:
            sys.exit(1)

    state["calls"].append([tool, *args])
    save()
    if tool == "make":
        assert args == ["build-programs"]
        for program in PROGRAMS:
            path = Path("target/guest") / f"{program}.bin"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(program)
    elif tool == "cargo":
        example = args[args.index("--example") + 1]
        if example == "stablecoin_pdas":
            for role in ROLES:
                print(role, address(role))
        else:
            assert example == "twap_oracle_pdas"
            for role in (
                "current_tick_account",
                "price_observations",
                "oracle_price_account",
            ):
                print(role, address(role))
    elif tool == "wallet":
        home = Path(os.environ["LEE_WALLET_HOME_DIR"])
        config = json.loads((home / "wallet_config.json").read_text())
        state["used_configs"].append(config)
        storage_path = home / "storage.json"
        storage = json.loads(storage_path.read_text())
        if args[:3] == ["account", "new", "public"]:
            label = args[args.index("--label") + 1]
            storage["labels"][label] = address(label)
            storage_path.write_text(json.dumps(storage))
        elif args[:2] == ["account", "id"]:
            print(storage["labels"][args[-1]])
            print(f"Stored statistics at {home / 'statistics.json'}")
        else:
            assert args[0] == "deploy-program"
            name = Path(args[1]).read_text()
            transaction(f"deploy-{name}")
            if name in state["deployed"]:
                # Pinned wallet discards individual RPC failures.
                print("Error: Sending transaction failed for each client")
                save()
                sys.exit(1)
            state["deployed"].append(name)
            confirmed(f"deploy-{name}")
            return
        save()
    elif tool == "spel":
        if args[0] == "inspect":
            print("ImageID:", PROGRAMS[Path(args[1]).read_text()])
            return
        if "inspect" in args:
            data = bytes.fromhex(args[args.index("--data") + 1])
            if data == bytes(33) + (2**100).to_bytes(16, "little"):
                if state.get("unsupported_decoder"):
                    print("Unknown primitive type: account_id", file=sys.stderr)
                    sys.exit(1)
                print(
                    json.dumps(
                        {
                            "Fungible": {
                                "definition_id": "Public/" + "1" * 32,
                                "balance": str(2**100),
                            }
                        }
                    )
                )
            else:
                decoded = json.loads(data)
                fields = decoded.get("Fungible", decoded)
                for key in list(fields):
                    if (
                        key
                        in (
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
                        )
                        and fields[key] is not None
                    ):
                        fields[key] = "Public/" + fields[key]
                print(json.dumps(decoded))
            return
        assert args[:1] == ["--idl"] and args[2] == "--program-id"
        program = Path(args[1]).stem.removesuffix("-idl")
        assert args[3] == PROGRAMS[program]
        name = args[4]
        keys = args[5::2]
        values = args[6::2]
        assert len(keys) == len(values)
        options = {
            key.removeprefix("--").replace("-", "_"): value
            for key, value in zip(keys, values)
        }
        transaction(name)
        if name == "new-fungible-definition":
            definition = options["definition_target_account"]
            assert definition not in state["accounts"], "duplicate mint"
            put(
                definition,
                "token",
                {
                    "Fungible": {
                        "name": options["name"],
                        "total_supply": options["total_supply"],
                        "metadata_id": None,
                        "authority": options["mint_authority"],
                    }
                },
            )
            put(
                options["holding_target_account"],
                "token",
                {
                    "Fungible": {
                        "definition_id": definition,
                        "balance": options["total_supply"],
                    }
                },
            )
        elif name == "create-oracle-price-account":
            assert options["oracle_price_account"] not in state["accounts"], (
                "duplicate oracle"
            )
            put(
                options["oracle_price_account"],
                "twap_oracle",
                {
                    "base_asset": options["base_asset"],
                    "quote_asset": options["quote_asset"],
                    "price": options["initial_price"],
                    "timestamp": "1700000000000",
                    "source_id": options["price_source"],
                    "confidence_interval": "0",
                },
            )
        elif name == "initialize-program":
            schema = json.loads(Path(args[1]).read_text())
            for argument in schema["instructions"][0]["args"]:
                key = argument["name"]
                if key.startswith("initial_controller_"):
                    assert argument["type"] == "u128", (
                        "pinned SPEL cannot serialize i128"
                    )
                    value = int(options[key])
                    assert 0 <= value < 2**128
                    options[key] = str(value if value < 2**127 else value - 2**128)
            expected = [
                "admin",
                *ROLES[:-1],
                "collateral_definition",
                "market_price_oracle",
                "clock",
            ]
            assert list(options)[:9] == expected, "wrong instruction account order"
            assert options["protocol_parameters"] not in state["accounts"], (
                "duplicate initialization"
            )
            definition = options["stablecoin_definition"]
            fields = {
                "admin_account_id": options["admin"],
                "freeze_authority_account_id": options["freeze_authority_account_id"],
                "stablecoin_definition_id": definition,
                "collateral_definition_id": options["collateral_definition"],
                "market_price_oracle_id": options["market_price_oracle"],
                "is_frozen": False,
            }
            for key, value in options.items():
                if key.startswith("initial_") and key != "initial_redemption_price":
                    fields[key.removeprefix("initial_")] = value
                if key in (
                    "minimum_milliseconds_between_rate_updates",
                    "maximum_oracle_price_age_milliseconds",
                ):
                    fields[key] = value
            put(options["protocol_parameters"], "stablecoin", fields)
            put(
                options["stability_fee_accumulator"],
                "stablecoin",
                {
                    "accumulated_rate_at_last_accrual": str(10**27),
                    "last_accrued_at": "1700000000000",
                },
            )
            put(
                options["redemption_price_state"],
                "stablecoin",
                {
                    "redemption_price_at_last_update": options[
                        "initial_redemption_price"
                    ],
                    "redemption_rate_per_millisecond": str(10**27),
                    "controller_integral_term": "0",
                    "last_updated_at": "1700000000000",
                },
            )
            put(
                definition,
                "token",
                {
                    "Fungible": {
                        "name": options["stablecoin_name"],
                        "total_supply": "0",
                        "authority": definition,
                    }
                },
            )
            put(
                options["stablecoin_master_holding"],
                "token",
                {"Fungible": {"definition_id": definition, "balance": "0"}},
            )
        else:
            raise AssertionError(f"Unexpected instruction: {name}")
        confirmed(name)
    else:
        raise AssertionError(f"Unexpected executable: {tool}")


if __name__ == "__main__":
    main()
