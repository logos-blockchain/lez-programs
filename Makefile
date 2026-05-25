.PHONY: clippy clippy-guest clippy-all test fmt idl

clippy:
	RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

clippy-guest:
	for manifest in token/methods/guest/Cargo.toml amm/methods/guest/Cargo.toml ata/methods/guest/Cargo.toml stablecoin/methods/guest/Cargo.toml twap_oracle/methods/guest/Cargo.toml; do \
		cargo clippy --manifest-path "$$manifest" --all-targets -- -D warnings || exit 1; \
	done

clippy-all: clippy clippy-guest

test:
	RISC0_DEV_MODE=1 cargo test --workspace

fmt:
	cargo +nightly fmt --all

idl:
	for program in token amm ata stablecoin twap_oracle; do \
		cargo run -p idl-gen -- "$$program/methods/guest/src/bin/$$program.rs" > "artifacts/$$program-idl.json" || exit 1; \
	done
