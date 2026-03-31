.PHONY: all build release check fmt fmt-check clippy test test-js build-js lint deny ci install clean changelog

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
	cargo check --workspace

# Format all Rust code
fmt:
	cargo fmt --all

# Check formatting (CI)
fmt-check:
	cargo fmt --all --check

# Run clippy lints
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# Run Rust tests
test:
	cargo test --workspace

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
	cargo deny check

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
