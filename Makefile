-include .env
export

.PHONY: all build release check fmt fmt-check clippy test test-js build-js lint deny ci install clean changelog verify-harness verify-harness-nightly verify-harness-repro verify-harness-storage verify-harness-engine verify-harness-server verify-harness-runtime verify-harness-nightly-storage verify-harness-nightly-engine verify-harness-nightly-server verify-harness-nightly-runtime bench-embedded-providers bench-postgres-provider bench-mysql-provider bench-libsql-replica-provider convex-demo convex-demo-node convex-demo-html convex-demo-http convex-demo-stop

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

# Benchmark retained embedded providers on the storage migration workloads
bench-embedded-providers:
	cargo bench -p neovex-engine --bench embedded-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),)

# Benchmark the Postgres provider against embedded SQLite plus injected RTT sensitivity
bench-postgres-provider:
	cargo bench -p neovex-engine --bench postgres-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the MySQL provider against embedded SQLite plus injected RTT sensitivity
bench-mysql-provider:
	cargo bench -p neovex-engine --bench mysql-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the libsql replica provider against embedded SQLite plus replica-specific catch-up drills
bench-libsql-replica-provider:
	cargo bench -p neovex-engine --bench libsql-replica-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Dependency audit (licenses + vulnerabilities)
deny:
	$(SINGLE_FLIGHT) --key cargo-deny-check -- cargo deny check

# Focused verification harness slice
verify-harness:
	bash scripts/verification-harness.sh pr $(if $(SURFACE),$(SURFACE),all)

verify-harness-storage:
	$(MAKE) verify-harness SURFACE=storage

verify-harness-engine:
	$(MAKE) verify-harness SURFACE=engine

verify-harness-server:
	$(MAKE) verify-harness SURFACE=server

verify-harness-runtime:
	$(MAKE) verify-harness SURFACE=runtime

# Heavier adversarial verification harness slice for scheduled runs
verify-harness-nightly:
	bash scripts/verification-harness.sh nightly $(if $(SURFACE),$(SURFACE),all)

verify-harness-nightly-storage:
	$(MAKE) verify-harness-nightly SURFACE=storage

verify-harness-nightly-engine:
	$(MAKE) verify-harness-nightly SURFACE=engine

verify-harness-nightly-server:
	$(MAKE) verify-harness-nightly SURFACE=server

verify-harness-nightly-runtime:
	$(MAKE) verify-harness-nightly SURFACE=runtime

# Re-run one exact verification harness case
verify-harness-repro:
	@test -n "$(SURFACE)" || (echo "set SURFACE=storage|engine|server|runtime" && exit 1)
	@test -n "$(MODE)" || (echo "set MODE=pr|nightly" && exit 1)
	@test -n "$(CASE)" || (echo "set CASE=<named-seed-case>" && exit 1)
	bash scripts/verification-harness.sh repro "$(SURFACE)" "$(MODE)" "$(CASE)"

# Prepare an upstream convex-demos overlay, then run codegen + Neovex against it
convex-demo: convex-demo-stop
	@test -n "$(CONVEX_DEMOS_DIR)" || (echo "Set CONVEX_DEMOS_DIR in .env first" && exit 1)
	@test -n "$(DEMO)" || (echo "Usage: make convex-demo DEMO=node|html|http" && exit 1)
	@overlay_dir="$$(node ./scripts/convex-demo-overlay.mjs "$(CONVEX_DEMOS_DIR)" "$(DEMO)")"; \
	echo "Prepared overlay at $$overlay_dir"; \
	npx convex codegen --app "$$overlay_dir"; \
	cargo run -p neovex-bin -- --port 8080 --convex-app-dir "$$overlay_dir"

convex-demo-node: DEMO=node
convex-demo-node: convex-demo

convex-demo-html: DEMO=html
convex-demo-html: convex-demo

convex-demo-http: DEMO=http
convex-demo-http: convex-demo

convex-demo-stop:
	bash scripts/stop-demo-processes.sh

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
