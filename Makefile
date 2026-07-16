.PHONY: build-programs clippy clippy-guest clippy-all test integration-test fmt idl changelog release

build-programs:
	./scripts/build-guests.sh

clippy:
	RISC0_SKIP_BUILD=1 cargo clippy --workspace --all-targets -- -D warnings

clippy-guest:
	for manifest in programs/*/methods/guest/Cargo.toml; do \
		cargo clippy --manifest-path "$$manifest" --all-targets -- -D warnings || exit 1; \
	done

clippy-all: clippy clippy-guest

test:
	RISC0_DEV_MODE=1 cargo test --workspace

integration-test:
	RISC0_DEV_MODE=1 cargo test -p integration_tests

fmt:
	cargo +nightly fmt --all

idl:
	for src in programs/*/methods/guest/src/bin/*.rs; do \
		program=$$(basename "$$src" .rs); \
		cargo run -p idl-gen -- "$$src" > "artifacts/$${program}-idl.json" || exit 1; \
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
