.PHONY: build-programs clippy clippy-guest clippy-all check-benchmark test integration-test test-apps check-lez-core-version fmt idl changelog release

# Builds every guest, or a subset: make build-programs PROGRAMS="amm token"
build-programs:
	./scripts/build-guests.sh $(PROGRAMS)

clippy:
	RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

clippy-guest:
	for manifest in programs/*/methods/guest/Cargo.toml; do \
		cargo clippy --manifest-path "$$manifest" --all-targets -- -D warnings || exit 1; \
	done

clippy-all: clippy clippy-guest

# The benchmark crate is excluded from the workspace, so workspace builds never compile it.
check-benchmark:
	RISC0_SKIP_BUILD=1 cargo clippy --manifest-path programs/benchmark/Cargo.toml --all-targets -- -D warnings

test:
	RISC0_DEV_MODE=1 cargo test --workspace

integration-test:
	RISC0_DEV_MODE=1 cargo test -p integration_tests

# Wallet, AMM and token app tests (C++ and QML), with Qt from the flake's nixpkgs.
test-apps:
	./scripts/test-apps.sh

# Every flake pins one lez_core rev, and each metadata.json declares that rev's version.
check-lez-core-version:
	./scripts/check-lez-core-version.sh

fmt:
	cargo +nightly fmt --all

# IDLs of the programs ported to the actor model, generated from their core crates, which mark
# their message enum and state types. The others keep their v1 IDLs in artifacts/ until their
# ports. `make idl PROGRAM=<name>` generates only that one.
IDL_PROGRAMS := token nft twap_oracle

idl:
	@for program in $(IDL_PROGRAMS); do \
		[ -z "$(PROGRAM)" ] || [ "$$program" = "$(PROGRAM)" ] || continue; \
		cargo run -q -p idl-gen -- programs/$$program/core --events event \
			> "artifacts/$$program-idl.json.tmp" || exit 1; \
		mv "artifacts/$$program-idl.json.tmp" "artifacts/$$program-idl.json"; \
	done

changelog:
	git cliff -o CHANGELOG.md

# Cut a release: regenerate the changelog for VERSION, commit it, and tag that
# commit so the tag contains the changelog. Usage: make release VERSION=v0.3.0
# (omit VERSION to let git-cliff pick the next semver from the commit history).
release:
	@VERSION="$(VERSION)"; \
	if [ -z "$$VERSION" ]; then VERSION=$$(git cliff --bumped-version); fi; \
	echo "Releasing $$VERSION"; \
	git cliff --tag "$$VERSION" -o CHANGELOG.md && \
	git add CHANGELOG.md && \
	git commit -m "chore(release): $$VERSION" && \
	git tag -a "$$VERSION" -m "$$VERSION" && \
	echo "Tagged $$VERSION. Push with: git push origin $$(git rev-parse --abbrev-ref HEAD) $$VERSION"

# Build and run the Logos module test suites (amm, token, stablecoin, oracle).
# Building each *-module-tests package runs its tests; -L prints their output.
.PHONY: test-modules
test-modules:
	nix build --no-link -L \
		.#amm-module-tests .#token-module-tests \
		.#stablecoin-module-tests .#oracle-module-tests
