.PHONY: clippy clippy-guest clippy-all test fmt idl

clippy:
	RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

clippy-guest:
	for manifest in programs/*/methods/guest/Cargo.toml; do \
		cargo clippy --manifest-path "$$manifest" --all-targets -- -D warnings || exit 1; \
	done

clippy-all: clippy clippy-guest

test:
	RISC0_DEV_MODE=1 cargo test --workspace

fmt:
	cargo +nightly fmt --all

idl:
	for src in programs/*/methods/guest/src/bin/*.rs; do \
		program=$$(basename "$$src" .rs); \
		cargo run -p idl-gen -- "$$src" > "artifacts/$${program}-idl.json" || exit 1; \
	done
