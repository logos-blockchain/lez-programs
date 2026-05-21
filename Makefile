.PHONY: clippy clippy-guest clippy-all test fmt

clippy:
	RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

clippy-guest:
	for manifest in token/methods/guest/Cargo.toml amm/methods/guest/Cargo.toml ata/methods/guest/Cargo.toml; do \
		cargo clippy --manifest-path "$$manifest" --all-targets -- -D warnings || exit 1; \
	done

clippy-all: clippy clippy-guest

test:
	RISC0_DEV_MODE=1 cargo test --workspace

fmt:
	cargo +nightly fmt --all
