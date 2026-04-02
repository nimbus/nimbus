.PHONY: all build release check fmt fmt-check clippy test test-js build-js lint deny ci install clean changelog verify-harness-pr verify-harness-nightly verify-harness-repro

SINGLE_FLIGHT = bash scripts/single-flight.sh

# Default target
all: check

# Debug build
build:
	cargo build --workspace

# Release build (binary only)
release:
	cargo build --release -p neovex-bin

# Check compilation without producing artifacts
check:
	$(SINGLE_FLIGHT) --key cargo-check-workspace -- cargo check --workspace

# Format all Rust code
fmt:
	cargo fmt --all

# Check formatting (CI)
fmt-check:
	cargo fmt --all --check

# Run clippy lints
clippy:
	$(SINGLE_FLIGHT) --key cargo-clippy-workspace -- cargo clippy --workspace --all-targets -- -D warnings

# Run Rust tests
test:
	$(SINGLE_FLIGHT) --key cargo-test-workspace -- cargo test --workspace

# Build JS packages
build-js:
	npm run build --workspaces --if-present

# Run JS tests
test-js:
	npm run test --workspaces --if-present

# Full lint suite
lint: fmt-check clippy

# Dependency audit (licenses + vulnerabilities)
deny:
	$(SINGLE_FLIGHT) --key cargo-deny-check -- cargo deny check

# Focused verification harness slice for PRs
verify-harness-pr:
	bash scripts/verification-harness.sh pr

# Heavier adversarial verification harness slice for scheduled runs
verify-harness-nightly:
	bash scripts/verification-harness.sh nightly

# Re-run one exact verification harness case
verify-harness-repro:
	@test -n "$(SURFACE)" || (echo "set SURFACE=storage|engine|server" && exit 1)
	@test -n "$(MODE)" || (echo "set MODE=pr|nightly" && exit 1)
	@test -n "$(CASE)" || (echo "set CASE=<named-seed-case>" && exit 1)
	bash scripts/verification-harness.sh repro "$(SURFACE)" "$(MODE)" "$(CASE)"

# Full CI check (runs locally what CI runs remotely)
ci: lint deny test build-js test-js

# Install the CLI binary to ~/.cargo/bin
install:
	cargo install --path crates/neovex-bin

# Regenerate CHANGELOG.md from conventional commits
changelog:
	git-cliff --output CHANGELOG.md

# Remove build artifacts
clean:
	cargo clean
