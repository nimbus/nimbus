-include .env
export

.PHONY: test-node-workload-executor-live all build build-ui build-packages release check fmt fmt-check clippy test test-js typecheck-js build-js lint deny ci install clean changelog verify-release-version-contract verify-release-archive-layout-helper verify-release-oci-image-helper verify-release-oci-image-build-helper verify-release-oci-image-live verify-release-oci-image-live-helper verify-desktop-ui verify-tenant-isolation-conformance verify-enterprise-policy-egress verify-artifact-provenance verify-bun-jsc-runtime-contract verify-harness verify-harness-nightly verify-harness-repro verify-harness-storage verify-harness-engine verify-harness-server verify-harness-runtime verify-harness-nightly-storage verify-harness-nightly-engine verify-harness-nightly-server verify-harness-nightly-runtime node-compat-report node-compat-dashboard node-compat-status node-compat-inventory node-compat-classifications node-compat-sync node-compat-refresh node-compat-validate-fixtures node-compat-verify-fixture-upstream node-compat-publish-evidence node-compat-publish-docs node-compat-release-train node-compat-trends node-compat-required-surface-blockers node-compat-sync-watchpoints node-compat-validate-watchpoints node-compat-oracle node-compat-canaries-bootstrap node-compat-canaries node-compat-validate-claims check-vmm-host collect-vmm-package-versions collect-podman-machine-diagnostics collect-nimbus-machine-diagnostics collect-nimbus-machine-cli-proof collect-nimbus-machine-guest-proof collect-nimbus-machine-service-proof collect-nimbus-homebrew-cask-proof collect-sqlcipher-proof-bundles collect-encryption-benchmark-evidence build-nimbus-machine-guest-binary build-linux-release-packages build-apt-repository build-fedora-release-srpms check-podman-machine-socket-paths validate-podman-machine-readiness recreate-podman-machine recreate-nimbus-machine prepare-linux-vmm-validation-bundle verify-build-nimbus-machine-guest-binary-helper verify-build-linux-release-packages-helper verify-build-apt-repository-helper verify-build-fedora-release-srpms-helper verify-podman-machine-socket-paths-helper verify-podman-machine-readiness-helper verify-podman-machine-recreate-helper verify-nimbus-machine-diagnostics-helper verify-nimbus-machine-recreate-helper verify-nimbus-machine-cli-proof-helper verify-nimbus-machine-guest-proof-helper verify-nimbus-homebrew-cask-proof-helper verify-collect-sqlcipher-proof-bundles-helper verify-install-helper verify-linux-vmm-validation-bundle-helper prepare-krun-bundle verify-krun-bundle-helper prepare-direct-krun-drill verify-direct-krun-drill-helper verify-runtime-separation verify-runtime-separation-helper verify-podman-machine-diagnostics-helper prepare-conmon-krun-drill verify-conmon-krun-drill-helper bench-embedded-providers bench-postgres-provider bench-mysql-provider bench-libsql-replica-provider convex-demo convex-demo-node convex-demo-html convex-demo-http convex-demo-stop
.PHONY: test-rust-runtime test-rust-workspace test-rust-docs test-external-provider test-external-providers provider-fixture-up provider-fixture-down verify-external-provider-fixture-helper verify-tenant-lifecycle-callers verify-ppsc-seed-farm verify-ppsc-seed-farm-helper verify-elle-serializability verify-elle-serializability-helper proof-helpers ci-required prove-linux-cgroup-memory-limit verify-bun-jsc-linked-adapter verify-bun-jsc-adapter-package verify-bun-jsc-release-assets verify-bun-jsc-installed-package-proof verify-profile-aware-runtime-crossover verify-runtime-tenant-isolation examples-verify
.PHONY: verify-loom-handoff

SINGLE_FLIGHT = bash scripts/single-flight.sh

# === UI build dependency graph =============================================
# nimbus-assets embeds artifacts produced by the nimbus-ui JS
# toolchain (`npm run codegen` + `npm run build`). These variables and
# recipes make the dependency explicit so a fresh clone running
# `make check` / `make test` / `make verify-desktop-ui` / `make ci-required`
# walks the graph and builds UI prerequisites on demand.
# See docs/private/plans/archive/local-dev-canonicalization-plan.md for the design.

UI_PKG := packages/nimbus-ui

# Files whose change should trigger the UI codegen re-run.
UI_CODEGEN_SOURCES := \
  $(shell find $(UI_PKG)/convex -type f -name '*.ts' -not -path '*/_generated/*' 2>/dev/null) \
  $(UI_PKG)/scripts/generate-routes.mjs \
  $(UI_PKG)/scripts/route-ignore-pattern.mjs \
  $(UI_PKG)/package.json

# Files produced by `npm run codegen`. Documented for clarity; the Make target
# is the stamp below.
UI_CODEGEN_OUTPUTS := \
  $(UI_PKG)/.nimbus/convex/auth.config.json \
  $(UI_PKG)/.nimbus/convex/bundle.mjs \
  $(UI_PKG)/.nimbus/convex/bundle.sha256 \
  $(UI_PKG)/.nimbus/convex/functions.json \
  $(UI_PKG)/.nimbus/convex/http_routes.json \
  $(UI_PKG)/.nimbus/convex/node_external_packages.json \
  $(UI_PKG)/.nimbus/convex/schema.json

# Track codegen freshness via bundle.sha256 — `npm run codegen` writes it
# at the end of its bundle step, so its mtime is a faithful sentinel for
# the whole UI_CODEGEN_OUTPUTS set. (Edge case: if you manually delete
# one of the other six outputs but leave bundle.sha256, Make won't notice
# — `rm $(UI_CODEGEN_SENTINEL) && make` recovers.)
UI_CODEGEN_SENTINEL := $(UI_PKG)/.nimbus/convex/bundle.sha256

$(UI_CODEGEN_SENTINEL): $(UI_CODEGEN_SOURCES)
	npm run codegen -w $(UI_PKG)

# Files whose change should trigger SPA rebuild.
UI_SPA_SOURCES := \
  $(shell find $(UI_PKG)/src -type f 2>/dev/null) \
  $(UI_PKG)/index.html \
  $(UI_PKG)/vite.config.ts \
  $(UI_PKG)/tsconfig.json \
  $(UI_PKG)/package.json

# Sentinel for the whole dist tree — consumed by nimbus-assets' rust-embed at
# compile time via `#[folder = "$CARGO_MANIFEST_DIR/../../packages/nimbus-ui/dist/"]`.
UI_DIST_INDEX := $(UI_PKG)/dist/index.html

$(UI_DIST_INDEX): $(UI_CODEGEN_SENTINEL) $(UI_SPA_SOURCES)
	npm run build -w $(UI_PKG)
# ===========================================================================

# === Embedded JS package payload dependency graph =========================
# nimbus-assets embeds the dependency-closed package payloads via rust-embed
# (#[folder = "$CARGO_MANIFEST_DIR/embedded/packages/"]). These recipes build
# each provisioned package's dist and stage them + a checksummed manifest, so a
# fresh clone running `make build` / `check` / `test` walks the graph and
# produces the payload before cargo compiles nimbus-bin.
# See docs/private/plans/archive/binary-embedded-package-distribution-plan.md (BPD1).

# Provisioned (app-facing) package dirs. `codegen` is build-time tooling
# (prebundled + embedded as a tooling run target), tracked here for sources.
EMBEDDED_PKG_DIRS := convex nimbus firebase mongodb dynamodb codegen
EMBEDDED_PKG_BUILD_SCRIPTS := $(shell find $(addprefix packages/,$(EMBEDDED_PKG_DIRS)) -maxdepth 1 -name build.mjs -type f 2>/dev/null)
EMBEDDED_PKG_SOURCES := \
  $(shell find $(addprefix packages/,$(EMBEDDED_PKG_DIRS)) -path '*/src/*' -type f 2>/dev/null) \
  $(addsuffix /package.json,$(addprefix packages/,$(EMBEDDED_PKG_DIRS))) \
  $(EMBEDDED_PKG_BUILD_SCRIPTS) \
  package.json \
  package-lock.json \
  scripts/build-js-package.mjs \
  scripts/check-package-closure.mjs \
  scripts/stage-embedded-packages.mjs

# Sentinel for the staged payload — stage-embedded-packages.mjs writes
# manifest.json last, so its mtime is a faithful stamp for the whole tree.
EMBEDDED_PKG_MANIFEST := crates/nimbus-assets/embedded/packages/manifest.json

$(EMBEDDED_PKG_MANIFEST): $(EMBEDDED_PKG_SOURCES)
	npm run build:embedded-packages

build-packages: $(EMBEDDED_PKG_MANIFEST)
# ===========================================================================

# Default target
all: check

# Build the embedded operator UI bundle that nimbus-server serves at /ui/*.
# Now a thin alias for the file-target above so `make build-ui` becomes
# a no-op when dist/ is already fresh relative to UI sources.
build-ui: $(UI_DIST_INDEX)

# Debug build
build: build-ui build-packages
	cargo build --workspace

# Release build (binary only)
release: build-ui build-packages
	cargo_features="$$(bash scripts/nimbus-release-rust-features.sh --format cargo-args)"; \
	cargo build --release -p nimbus-bin $$cargo_features

# Check compilation without producing artifacts
check: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	$(SINGLE_FLIGHT) --key cargo-check-workspace -- cargo check --workspace

# Format all Rust code
fmt:
	cargo fmt --all

# Check formatting (CI)
fmt-check:
	cargo fmt --all --check

# Minimal PPSC4 committer handoff state-space model. `--cfg loom` must be
# scoped to the test crate: setting it globally disables tokio::net in the
# ordinary engine dependency graph and makes hyper-util fail to compile.
verify-loom-handoff:
	rm -f target/release/deps/loom_handoff-*
	cargo rustc --locked -p nimbus-engine --test loom_handoff --release -- --cfg loom
	loom_test="$$(find target/release/deps -maxdepth 1 -type f -name 'loom_handoff-*' -perm -111 -print -quit)"; \
	test -n "$$loom_test"; \
	"$$loom_test"

# Run clippy lints
clippy: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	$(SINGLE_FLIGHT) --key cargo-clippy-workspace -- cargo clippy --workspace --all-targets -- -D warnings

# Run Rust tests
test: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	$(SINGLE_FLIGHT) --key cargo-test-workspace -- cargo test --workspace

# Run the CI runtime Rust test bucket. No UI prereq: nimbus-runtime has
# zero workspace deps (per CLAUDE.md), so cargo test -p nimbus-runtime
# doesn't compile nimbus-server and doesn't need the UI artifacts. The
# build-node22-anchor-snapshot-off prerequisite (also nimbus-runtime-only)
# generates the feature-off blob the tests/embedded_anchor integration test
# installs; it is idempotent so an up-to-date tree pays only a --check.
#
# Keep this bucket serial: many runtime tests are subprocess-isolated, but
# the parent test binary still contains unignored V8 constructors. Letting
# libtest overlap those with isolated parents can poison process-global V8
# cage / RO-heap state and abort the whole binary instead of producing a
# normal Rust test failure.
# Keep this feature-off lane out of the pool_reuse `isol_` parent tests:
# those subprocess crash-oracle parents are covered by test-rust-runtime-cage
# under the pointer-compression configuration they are designed to prove.
test-rust-runtime: build-node22-anchor-snapshot-off
	$(SINGLE_FLIGHT) --key cargo-test-runtime-ci -- cargo test -p nimbus-runtime -- --skip runtime::tests::node_compat:: --skip runtime::tests::pool_reuse::isol_ --test-threads=1

# Run the cage (pointer-compression) crash-oracle lane. The cross-profile shared-RO-heap
# crash only reproduces under --features v8-pointer-compression (the single shared cage),
# which is exactly how release binaries ship; the default test-rust-runtime lane is feature-off
# and therefore CANNOT exercise it. This lane runs the subprocess-isolated `isol_*` parents
# (filter `isol_`): each spawns a fresh-cage child, so the crash-by-design controls assert the
# bug still aborts by signal (non-vacuous) and the fix tests assert success, without aborting
# or poisoning the shared test binary.
test-rust-runtime-cage:
	$(SINGLE_FLIGHT) --key cargo-test-runtime-cage-ci -- cargo test -p nimbus-runtime --features v8-pointer-compression --lib isol_

.PHONY: build-node22-anchor-snapshot build-node22-anchor-snapshot-off build-node22-anchor-snapshot-on
# The embedded NodeFull(Node22) anchor snapshots are GENERATED per build target + pointer-compression
# config (a V8 startup snapshot is platform-specific) and are NOT committed (gitignored). The serving
# path DESERIALIZES the generated blob (~19ms) instead of building it lazily (~4.18s, which blows
# per-request timeouts). Each CI lane / a release build regenerates the blob for ITS target+config
# BEFORE building the consuming binary; the runtime provenance guard (arch+OS+V8+pc+extensions+ops+JS)
# falls back to a runtime build if a blob is missing or built for the wrong target/config, so a fresh
# `cargo build` without this step is slow-but-correct, never wrong. Each target also `--check`s that
# the just-generated blob is live (a self-test of the generator). `-off`/`-on` do ONE config (the
# single-config CI lanes); the bare target does both for local dev (the `-on` half in a separate
# CARGO_TARGET_DIR so its pointer-compression V8 prebuilt does not collide with the shared
# gn_out/obj/librusty_v8.a a preceding feature-off build wrote — the build.rs guard otherwise fails).
# `-off`/`-on` are IDEMPOTENT (`--check` first; only regenerate if the blob is absent or stale for
# this target+config), so they are cheap to use as a build prerequisite — a fresh checkout / changed
# bootstrap pays the ~4.18s once, an up-to-date tree pays only a `--check`. The bare target FORCE
# regenerates both (the `-on` half in a separate CARGO_TARGET_DIR to dodge the shared-.a guard).
build-node22-anchor-snapshot-off:
	cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot -- --check \
		|| cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot
build-node22-anchor-snapshot-on:
	cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot --features v8-pointer-compression -- --check \
		|| cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot --features v8-pointer-compression
build-node22-anchor-snapshot:
	cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot
	cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot -- --check
	CARGO_TARGET_DIR=target/ptrcomp cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot --features v8-pointer-compression
	CARGO_TARGET_DIR=target/ptrcomp cargo run -p nimbus-runtime --bin build_node22_anchor_snapshot --features v8-pointer-compression -- --check

# Run the CI workspace Rust test bucket. CW2: when NIMBUS_NEXTEST_PARTITION is
# set to `N/M`, the partition is forwarded as `--partition hash:N/M` so the
# job can be sharded across the CI matrix. The single-flight key includes the
# partition suffix so concurrent shards in the same workspace do not collide.
NIMBUS_NEXTEST_PARTITION ?=
ifeq ($(strip $(NIMBUS_NEXTEST_PARTITION)),)
NEXTEST_PARTITION_ARGS :=
NEXTEST_SINGLE_FLIGHT_SUFFIX :=
else
NEXTEST_PARTITION_ARGS := --partition hash:$(NIMBUS_NEXTEST_PARTITION)
NEXTEST_SINGLE_FLIGHT_SUFFIX := -$(subst /,-of-,$(NIMBUS_NEXTEST_PARTITION))
endif

# The convex seeded HTTP test (in nimbus-server) deserializes the embedded NodeFull anchor snapshot
# (~19ms) on its first request; without it the anchor builds lazily (~4.18s) and blows the test's 3s
# per-request timeout. The blob is generated per target (not committed), so generate it (idempotently)
# before the workspace build — locally and in CI.
test-rust-workspace: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST) build-node22-anchor-snapshot-off
	NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 $(SINGLE_FLIGHT) --key cargo-nextest-workspace-ci$(NEXTEST_SINGLE_FLIGHT_SUFFIX) -- cargo nextest run --workspace --exclude nimbus-runtime $(NEXTEST_PARTITION_ARGS)

# LR12: live hidden node-workload-executor test against session systemd
# (Linux CI lane node-dbus-integration). Builds nimbus-bin, so it rides the
# same UI + embedded-package prerequisite graph as the workspace lanes.
test-node-workload-executor-live: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	cargo test -p nimbus-bin --features node-workload-executor-integration-tests node_workload_executor_converges_transient_unit -- --nocapture

# Run the CI workspace doctest bucket
test-rust-docs: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	$(SINGLE_FLIGHT) --key cargo-doc-tests-workspace-ci -- cargo test --workspace --exclude nimbus-runtime --doc

# Deterministic service-backed storage/engine/system provider integration tests.
# The fixture lifecycle owns provisioning, URLs, required-fixture mode, logs,
# and cleanup. KEEP=1 retains a fixture started by a test run; REUSE=1 opts in
# to consuming an already-running healthy matching fixture.
PROVIDER ?= all
KEEP ?= 0
REUSE ?= 0
TEST_FILTER ?=

# The provider runner compiles nimbus-bin transitively. Keep its generated
# rust-embed inputs on the same fresh-checkout dependency graph as `make test`
# so the advertised one-command fixture interface does not require manual JS
# preparation.
test-external-provider: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	NIMBUS_PROVIDER_FIXTURE_KEEP=$(KEEP) NIMBUS_PROVIDER_FIXTURE_REUSE=$(REUSE) \
		NIMBUS_EXTERNAL_PROVIDER_TEST_FILTER="$(TEST_FILTER)" \
		$(SINGLE_FLIGHT) --key test-external-provider-$(PROVIDER) -- \
		bash scripts/external-provider-fixture.sh run "$(PROVIDER)"

test-external-providers: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	NIMBUS_PROVIDER_FIXTURE_KEEP=$(KEEP) NIMBUS_PROVIDER_FIXTURE_REUSE=$(REUSE) \
		NIMBUS_EXTERNAL_PROVIDER_TEST_FILTER="$(TEST_FILTER)" \
		$(SINGLE_FLIGHT) --key test-external-providers -- \
		bash scripts/external-provider-fixture.sh run all

provider-fixture-up:
	bash scripts/external-provider-fixture.sh up "$(PROVIDER)"

provider-fixture-down:
	bash scripts/external-provider-fixture.sh down "$(PROVIDER)"

verify-external-provider-fixture-helper:
	bash scripts/verify-external-provider-fixture-helper.sh

verify-tenant-lifecycle-callers:
	bash scripts/verify-tenant-lifecycle-callers.sh

# Deterministic PPSC redb seed farm. A single-seed replay is selected with
# SEED=<u64> (or NIMBUS_PPSC_SEED); otherwise the explicit global range is
# partitioned into non-overlapping contiguous shards. The ignored Engine driver
# writes one interruption/failure bundle per current seed and a count-bearing
# summary, while the script rejects zero-test filters before execution.
BACKEND ?= redb
SEED ?=
SEED_START ?= 0
SEED_COUNT ?= 1000
SHARD_INDEX ?= 0
SHARD_COUNT ?= 1
STEP_COUNT ?= 32
FAILURE_DIR ?= target/ppsc-seed-farm/shard-$(SHARD_INDEX)-of-$(SHARD_COUNT)

verify-ppsc-seed-farm:
	@set -eu; \
	export NIMBUS_PPSC_BACKEND="$${NIMBUS_PPSC_BACKEND:-$(BACKEND)}"; \
	export NIMBUS_PPSC_STEP_COUNT="$${NIMBUS_PPSC_STEP_COUNT:-$(STEP_COUNT)}"; \
	export NIMBUS_PPSC_FAILURE_DIR="$${NIMBUS_PPSC_FAILURE_DIR:-$(FAILURE_DIR)}"; \
	export NIMBUS_PPSC_REVISION="$${NIMBUS_PPSC_REVISION:-$$(git rev-parse HEAD)}"; \
	selected_seed="$${NIMBUS_PPSC_SEED:-$(SEED)}"; \
	if [ -n "$$selected_seed" ]; then \
		export NIMBUS_PPSC_SEED="$$selected_seed"; \
	else \
		export NIMBUS_PPSC_SEED_START="$${NIMBUS_PPSC_SEED_START:-$(SEED_START)}"; \
		export NIMBUS_PPSC_SEED_COUNT="$${NIMBUS_PPSC_SEED_COUNT:-$(SEED_COUNT)}"; \
		export NIMBUS_PPSC_SHARD_INDEX="$${NIMBUS_PPSC_SHARD_INDEX:-$(SHARD_INDEX)}"; \
		export NIMBUS_PPSC_SHARD_COUNT="$${NIMBUS_PPSC_SHARD_COUNT:-$(SHARD_COUNT)}"; \
	fi; \
	$(SINGLE_FLIGHT) --key "verify-ppsc-seed-farm-$${NIMBUS_PPSC_BACKEND}-$${NIMBUS_PPSC_SHARD_INDEX:-single}-$${NIMBUS_PPSC_SHARD_COUNT:-seed}" -- \
		bash scripts/ppsc-seed-farm.sh

verify-ppsc-seed-farm-helper:
	bash scripts/verify-ppsc-seed-farm-helper.sh

# Pinned external Elle 0.1.9 serializability proof. The script owns download,
# release + standalone-JAR checksum verification, Java readiness, exact ignored
# test selection, and preservation of the checker/test exit status.
verify-elle-serializability: $(UI_DIST_INDEX) $(EMBEDDED_PKG_MANIFEST)
	$(SINGLE_FLIGHT) --key verify-elle-serializability -- \
		bash scripts/verify-elle-serializability.sh

verify-elle-serializability-helper:
	bash scripts/verify-elle-serializability-helper.sh

# Build JS packages
build-js:
	npm run build --workspaces --if-present

# Run JS tests
test-js:
	npm run test --workspaces --if-present

# Typecheck every JS/TS workspace that declares a typecheck script.
typecheck-js:
	npm run typecheck --workspaces --if-present

# Boot every examples/ app against a fresh local server and run its headless
# smoke (EX5.2). Deliberately NOT folded into ci-required/ci: it boots eight
# Nimbus servers sequentially (each paying its own health-check wait plus,
# for four apps, a codegen preflight), so its wall-clock cost and
# port/data-dir contention risk don't belong on ci's fast-feedback critical
# path. It runs as its own dedicated CI lane instead (EX5.3) — see
# .github/workflows/ci.yml.
examples-verify:
	$(SINGLE_FLIGHT) --key examples-verify -- bash scripts/examples-verify.sh

# Full lint suite
lint: fmt-check clippy

# Verify deterministic proof-helper scripts used by hosted CI
proof-helpers:
	bash -n scripts/verify-tenant-lifecycle-callers.sh
	bash scripts/verify-tenant-lifecycle-callers.sh
	bash -n scripts/ppsc-seed-farm.sh
	bash -n scripts/verify-ppsc-seed-farm-helper.sh
	bash scripts/verify-ppsc-seed-farm-helper.sh
	bash -n scripts/verify-elle-serializability.sh
	bash -n scripts/verify-elle-serializability-helper.sh
	bash scripts/verify-elle-serializability-helper.sh
	bash -n scripts/external-provider-fixture.sh
	bash -n scripts/test-external-providers.sh
	bash -n scripts/verify-external-provider-fixture-helper.sh
	bash scripts/verify-external-provider-fixture-helper.sh
	bash -n scripts/verify-mutation-committer-arm.sh
	bash scripts/verify-mutation-committer-arm.sh
	bash -n scripts/collect-sqlcipher-proof-bundles.sh
	bash -n scripts/collect-nimbus-machine-guest-proof.sh
	bash -n scripts/collect-nimbus-machine-service-proof.sh
	bash -n scripts/collect-nimbus-homebrew-cask-proof.sh
	bash -n scripts/prove-linux-cgroup-memory-limit.sh
	bash -n scripts/verify-tenant-isolation-conformance.sh
	bash -n scripts/verify-runtime-tenant-isolation.sh
	bash scripts/verify-runtime-tenant-isolation.sh
	bash -n scripts/verify-artifact-provenance.sh
	bash -n scripts/verify-bun-jsc-runtime-contract.sh
	bash -n scripts/verify-bun-jsc-in-process-lockdown.sh
	bash -n scripts/bun-jsc-adapter-contract.sh
	bash -n scripts/build-bun-jsc-adapter-artifacts.sh
	bash -n scripts/package-bun-jsc-adapter.sh
	bash -n scripts/verify-bun-jsc-adapter-package.sh
	bash -n scripts/verify-bun-jsc-adapter-package-helper.sh
	bash -n scripts/verify-bun-jsc-release-assets.sh
	bash -n scripts/verify-bun-jsc-release-assets-helper.sh
	bash -n scripts/render-release-oci-image-report.sh
	bash -n scripts/verify-release-oci-image-report.sh
	bash -n scripts/nimbus-release-rust-features.sh
	bash -n scripts/verify-node-full-substrate-realm.sh
	bash -n scripts/verify-runtime-execution-classification.sh
	bash -n scripts/verify-profile-aware-isolate-runtime-crossover.sh
	bash -n scripts/verify-release-oci-image-assets.sh
	bash -n scripts/smoke-release-oci-image.sh
	bash -n scripts/verify-release-oci-image-helper.sh
	bash -n scripts/verify-release-oci-image-build-helper.sh
	bash -n scripts/verify-release-oci-image-live.sh
	bash -n scripts/verify-release-oci-image-live-helper.sh
	bash -n scripts/verify-bun-jsc-installed-package-proof.sh
	bash -n scripts/build-linux-release-packages.sh
	bash -n scripts/verify-build-linux-release-packages-helper.sh
	bash -n scripts/verify-collect-sqlcipher-proof-bundles-helper.sh
	bash -n scripts/verify-nimbus-machine-guest-proof-helper.sh
	bash -n scripts/verify-nimbus-machine-service-proof-helper.sh
	bash -n scripts/verify-nimbus-homebrew-cask-proof-helper.sh
	bash -n scripts/install.sh
	bash -n scripts/verify-install.sh
	bash -n scripts/verify-install-helper.sh
	bash scripts/verify-collect-sqlcipher-proof-bundles-helper.sh
	bash scripts/verify-nimbus-machine-guest-proof-helper.sh
	bash scripts/verify-nimbus-machine-service-proof-helper.sh
	bash scripts/verify-nimbus-homebrew-cask-proof-helper.sh
	bash scripts/verify-bun-jsc-adapter-package-helper.sh
	bash scripts/verify-bun-jsc-release-assets-helper.sh
	bash scripts/verify-release-oci-image-helper.sh
	bash scripts/verify-release-oci-image-live-helper.sh
	bash scripts/verify-build-linux-release-packages-helper.sh
	bash scripts/verify-install-helper.sh

# Benchmark retained embedded providers on the storage migration workloads
bench-embedded-providers:
	cargo bench -p nimbus-engine --bench embedded-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),) $(if $(ENCRYPTION),--local-encryption $(ENCRYPTION),)

# Benchmark the Postgres provider against embedded SQLite plus injected RTT sensitivity
bench-postgres-provider:
	cargo bench -p nimbus-engine --bench postgres-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the MySQL provider against embedded SQLite plus injected RTT sensitivity
bench-mysql-provider:
	cargo bench -p nimbus-engine --bench mysql-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the libsql replica provider against embedded SQLite plus replica-specific catch-up drills
bench-libsql-replica-provider:
	cargo bench -p nimbus-engine --bench libsql-replica-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(foreach workload,$(WORKLOADS),--workload $(workload)) $(if $(WORKLOAD),--workload $(WORKLOAD),) $(if $(ENCRYPTION),--local-cache-encryption $(ENCRYPTION),)

collect-encryption-benchmark-evidence:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/path/to/output-dir" && exit 1)
	bash scripts/collect-encryption-benchmark-evidence.sh --output-dir "$(OUTPUT_DIR)"

# Dependency audit (licenses + vulnerabilities)
deny:
	$(SINGLE_FLIGHT) --key cargo-deny-check -- cargo deny check

# Third-party attribution gate (G4 of nimbus-sandbox-plan.md Fork-Health Guardrails)
verify-third-party-attribution:
	bash scripts/verify-third-party-attribution.sh

verify-third-party-attribution-helper:
	bash scripts/verify-third-party-attribution-helper.sh

# Verify that release tags, crate/package versions, and changelog entry agree
verify-release-version-contract:
	@test -n "$(VERSION)" || (echo "set VERSION=vX.Y.Z or VERSION=X.Y.Z" && exit 1)
	bash scripts/verify-release-version-contract.sh "$(VERSION)"

# Verify the published release archive layout contract, including the macOS helper bundle
verify-release-archive-layout-helper:
	bash scripts/verify-release-archive-layout-helper.sh

# Verify the release-owned Nimbus application OCI image contract
verify-release-oci-image-helper:
	bash scripts/verify-release-oci-image-helper.sh

verify-release-oci-image-build-helper:
	bash scripts/verify-release-oci-image-build-helper.sh

verify-release-oci-image-live:
	@test -n "$(TAG)" || (echo "set TAG=vX.Y.Z" && exit 1)
	bash scripts/verify-release-oci-image-live.sh \
		--tag "$(TAG)" \
		$(if $(REPO),--repo "$(REPO)",) \
		$(if $(IMAGE),--image "$(IMAGE)",) \
		$(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) \
		$(if $(RUNTIME),--runtime "$(RUNTIME)",) \
		$(if $(SKIP_SMOKE),--skip-smoke,)

verify-release-oci-image-live-helper:
	bash scripts/verify-release-oci-image-live-helper.sh

# Desktop UI browser-smoke harness. Builds the nimbus binary the
# disposable-server fixture spawns, then runs the deterministic walk.
# `$(UI_DIST_INDEX)` brings in both the convex registry outputs consumed by
# nimbus-convex and the SPA dist consumed by nimbus-assets; see the UI build
# dependency graph at the top of this file.
verify-desktop-ui: $(UI_DIST_INDEX)
	cargo build -p nimbus-bin
	npm run test:e2e:smoke -w packages/nimbus-ui

verify-tenant-isolation-conformance:
	bash scripts/verify-tenant-isolation-conformance.sh

verify-runtime-tenant-isolation:
	bash scripts/verify-runtime-tenant-isolation.sh

verify-enterprise-policy-egress:
	bash scripts/verify-enterprise-policy-egress.sh

verify-artifact-provenance:
	bash scripts/verify-artifact-provenance.sh

verify-bun-jsc-runtime-contract:
	bash scripts/verify-bun-jsc-runtime-contract.sh

verify-bun-jsc-linked-adapter:
	bash scripts/verify-bun-jsc-linked-adapter.sh

verify-bun-jsc-adapter-package:
	bash scripts/verify-bun-jsc-adapter-package-helper.sh

verify-bun-jsc-release-assets:
	bash scripts/verify-bun-jsc-release-assets-helper.sh

verify-bun-jsc-installed-package-proof:
	@test -n "$(ARCHIVE)" || (echo "set ARCHIVE=/path/to/nimbus-bun-jsc-adapter-*.tar.gz" && exit 1)
	bash scripts/verify-bun-jsc-installed-package-proof.sh --archive "$(ARCHIVE)"

verify-profile-aware-runtime-crossover:
	bash scripts/verify-profile-aware-isolate-runtime-crossover.sh

prove-linux-cgroup-memory-limit:
	bash scripts/prove-linux-cgroup-memory-limit.sh

# Focused verification harness slice
verify-harness:
	bash scripts/verification-harness.sh required $(if $(SURFACE),$(SURFACE),all)

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
	@test -n "$(MODE)" || (echo "set MODE=required|nightly" && exit 1)
	@test -n "$(CASE)" || (echo "set CASE=<named-seed-case>" && exit 1)
	bash scripts/verification-harness.sh repro "$(SURFACE)" "$(MODE)" "$(CASE)"

# Emit manifest-driven node-compat report artifacts for one seeded family/slice
node-compat-report:
	@test -n "$(FAMILY)" || (echo "set FAMILY=<family-id>" && exit 1)
	@test -n "$(SLICE)" || (echo "set SLICE=<slice-id>" && exit 1)
	bash scripts/runtime/node/report.sh --family "$(FAMILY)" --slice "$(SLICE)" $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(OBSERVED_RESULTS),--observed-results "$(OBSERVED_RESULTS)",) $(if $(CAPTURE_LIVE),--capture-live,)

node-compat-dashboard:
	python3 scripts/runtime/node/dashboard.py $(if $(ARTIFACTS_ROOT),--artifacts-root "$(ARTIFACTS_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-status:
	python3 scripts/runtime/node/status.py $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(EXPECTATION_CATALOG),--expectation-catalog "$(EXPECTATION_CATALOG)",) $(if $(OBSERVED_RESULTS),--observed-results "$(OBSERVED_RESULTS)",)

node-compat-inventory:
	python3 scripts/runtime/node/inventory.py $(if $(LANE),--lane "$(LANE)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-classifications:
	python3 scripts/runtime/node/classifications.py sync --lane "$(if $(LANE),$(LANE),all)" $(if $(PRESERVE_EXISTING),--preserve-existing,) $(if $(CHECK),--check,)

node-compat-sync:
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24|node26, or another checked-in nodeNN lane" && exit 1)
	python3 scripts/runtime/node/sync.py --lane "$(LANE)" $(if $(TAG),--upstream-tag "$(TAG)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(SOURCE_ROOT),--source-root "$(SOURCE_ROOT)",) $(if $(DRY_RUN),--dry-run,) $(if $(COMPARE_UPSTREAM),--compare-upstream,) $(if $(APPLY),--apply,) $(if $(FORCE),--force,)

node-compat-refresh:
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24|node26, or another checked-in nodeNN lane" && exit 1)
	python3 scripts/runtime/node/refresh.py --lane "$(LANE)" $(if $(TAG),--tag "$(TAG)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(SOURCE_ROOT),--source-root "$(SOURCE_ROOT)",) $(if $(DRY_RUN),--dry-run,) $(if $(COMPARE_UPSTREAM),--compare-upstream,) $(if $(APPLY),--apply,) $(if $(FORCE),--force,) $(if $(RUN_SLICES),--run-representative-slices,)

node-compat-validate-fixtures:
	python3 scripts/runtime/node/fixture_provenance.py validate $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-verify-fixture-upstream:
	python3 scripts/runtime/node/fixture_provenance.py verify-upstream $(if $(SOURCE_ROOT),--source-root "$(SOURCE_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(ALLOW_FETCH),--allow-fetch,)

node-compat-publish-evidence:
	python3 scripts/runtime/node/publish_evidence.py $(if $(ARTIFACTS_ROOT),--artifacts-root "$(ARTIFACTS_ROOT)",) $(if $(PUBLISH_ROOT),--publish-root "$(PUBLISH_ROOT)",)

node-compat-publish-docs:
	python3 scripts/runtime/node/publish_docs.py $(if $(EVIDENCE_ROOT),--evidence-root "$(EVIDENCE_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(CHECK),--check,)

node-compat-release-train:
	python3 scripts/runtime/node/release_train.py $(if $(CHECK),check,publish)

node-compat-trends:
	python3 scripts/runtime/node/trends.py $(if $(ARTIFACTS_ROOT),--artifacts-root "$(ARTIFACTS_ROOT)",) $(if $(BASELINE_ROOT),--baseline-root "$(BASELINE_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-required-surface-blockers:
	python3 scripts/runtime/node/required_surface_blockers.py $(if $(CHECK),--check,)

node-compat-sync-watchpoints:
	python3 scripts/runtime/node/watchpoints.py sync $(if $(EXPECTATION_CATALOG),--catalog "$(EXPECTATION_CATALOG)",)

node-compat-validate-watchpoints:
	python3 scripts/runtime/node/watchpoints.py validate $(if $(EXPECTATION_CATALOG),--catalog "$(EXPECTATION_CATALOG)",) $(if $(OBSERVED_RESULTS),--observed-results "$(OBSERVED_RESULTS)",)

node-compat-oracle:
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24, or another checked-in nodeNN lane" && exit 1)
	@test -n "$(SAMPLE)" || (echo "set SAMPLE=test/parallel/test-buffer-alloc.js" && exit 1)
	bash scripts/runtime/node/oracle-run.sh --lane "$(LANE)" --fixture "$(SAMPLE)" $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(NODE_BIN),--node-bin "$(NODE_BIN)",)

node-compat-canaries-bootstrap:
	bash scripts/runtime/node/canaries-bootstrap.sh $(if $(PRESET),--preset "$(PRESET)",)

node-compat-canaries: $(UI_DIST_INDEX)
	@test -n "$(PRESET)" || (echo "set PRESET=application|tooling" && exit 1)
	bash scripts/runtime/node/canaries-run.sh --preset "$(PRESET)" $(if $(LANE),--lane "$(LANE)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-validate-claims:
	bash scripts/runtime/node/validate-claims.sh

# crun patch/build/verify targets moved to nimbus/nimbus-crun

# Check whether the current host is ready for Linux krun/conmon validation work
check-vmm-host:
	bash scripts/check-vmm-host.sh

# Collect package-manager and command-level version evidence for the Linux VMM stack
collect-vmm-package-versions:
	bash scripts/collect-vmm-package-versions.sh

# Collect best-effort Podman machine diagnostics for the macOS research lane
collect-podman-machine-diagnostics:
	@test -n "$(MACHINE)" || (echo "set MACHINE=<podman-machine-name>" && exit 1)
	bash scripts/collect-podman-machine-diagnostics.sh --machine "$(MACHINE)" $(if $(PROVIDER),--provider "$(PROVIDER)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(CONFIG_ROOT),--config-root "$(CONFIG_ROOT)",) $(if $(DATA_ROOT),--data-root "$(DATA_ROOT)",) $(if $(TMP_ROOT),--tmp-root "$(TMP_ROOT)",) $(if $(PODMAN),--podman "$(PODMAN)",) $(if $(PS),--ps "$(PS)",) $(if $(SYSTEM_PROFILER),--system-profiler "$(SYSTEM_PROFILER)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Collect best-effort Nimbus machine diagnostics for the macOS manager lane
collect-nimbus-machine-diagnostics:
	bash scripts/collect-nimbus-machine-diagnostics.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(CONFIG_ROOT),--config-root "$(CONFIG_ROOT)",) $(if $(STATE_ROOT),--state-root "$(STATE_ROOT)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NIMBUS),--nimbus "$(NIMBUS)",) $(if $(PS),--ps "$(PS)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Collect isolated-root local-binary proof for `nimbus machine ...` without touching default roots
collect-nimbus-machine-cli-proof:
	bash scripts/collect-nimbus-machine-cli-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(ROOT),--root "$(ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NIMBUS),--nimbus "$(NIMBUS)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(GUEST_BINARY),--guest-binary "$(GUEST_BINARY)",) $(if $(SCRIPT),--script "$(SCRIPT)",) $(if $(KEEP_MACHINE),--keep-machine,)

# Collect guest-image contract proof from a booted Nimbus machine via `machine ssh`
collect-nimbus-machine-guest-proof:
	bash scripts/collect-nimbus-machine-guest-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NIMBUS),--nimbus "$(NIMBUS)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(GUEST_VOLUME_PATH),--guest-volume-path "$(GUEST_VOLUME_PATH)",) $(if $(GUEST_SOCKET_PATH),--guest-socket-path "$(GUEST_SOCKET_PATH)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Collect forwarded machine-API and host `nimbus service ...` proof from a booted Nimbus machine
collect-nimbus-machine-service-proof:
	@test -n "$(COMPOSE_FILE)" || (echo "set COMPOSE_FILE=/absolute/path/to/compose.yaml" && exit 1)
	@test -n "$(SERVICE)" || (echo "set SERVICE=<service-name>" && exit 1)
	bash scripts/collect-nimbus-machine-service-proof.sh --compose-file "$(COMPOSE_FILE)" --service "$(SERVICE)" $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NIMBUS),--nimbus "$(NIMBUS)",) $(if $(CURL),--curl "$(CURL)",) $(if $(PUBLISHED_URL),--published-url "$(PUBLISHED_URL)",)

# Collect real-host proof for the supported macOS Homebrew/cask install surface on isolated roots
collect-nimbus-homebrew-cask-proof:
	bash scripts/collect-nimbus-homebrew-cask-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(HOST_BINARY),--host-binary "$(HOST_BINARY)",) $(if $(GUEST_BINARY),--guest-binary "$(GUEST_BINARY)",) $(if $(BREW),--brew "$(BREW)",) $(if $(BREW_PREFIX),--brew-prefix "$(BREW_PREFIX)",) $(if $(GVPROXY),--gvproxy "$(GVPROXY)",) $(if $(READLINK),--readlink "$(READLINK)",) $(if $(SSH_KEYGEN),--ssh-keygen "$(SSH_KEYGEN)",) $(if $(TAP),--tap "$(TAP)",) $(if $(CASK),--cask "$(CASK)",) $(if $(KEEP_INSTALLED),--keep-installed,)

# Download SQLCipher proof bundles from a hosted GitHub Actions workflow run
collect-sqlcipher-proof-bundles:
	@test -n "$(RUN_ID)" || (echo "set RUN_ID=<github-actions-run-id>" && exit 1)
	bash scripts/collect-sqlcipher-proof-bundles.sh --run-id "$(RUN_ID)" $(if $(REPO),--repo "$(REPO)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(ARTIFACT_PREFIX),--artifact-prefix "$(ARTIFACT_PREFIX)",)

# Verify the SQLCipher proof bundle collector against a deterministic fake GitHub CLI
verify-collect-sqlcipher-proof-bundles-helper:
	bash scripts/verify-collect-sqlcipher-proof-bundles-helper.sh

# Build the Linux guest nimbus binary that macOS machine-start prefers before release downloads
build-nimbus-machine-guest-binary:
	bash scripts/build-nimbus-machine-guest-binary.sh $(if $(TARGET),--target "$(TARGET)",) $(if $(PROFILE),--profile "$(PROFILE)",) $(if $(COPY_TO),--copy-to "$(COPY_TO)",) $(if $(CACHE_ROOT),--cache-root "$(CACHE_ROOT)",) $(if $(CARGO_BIN),--cargo "$(CARGO_BIN)",) $(if $(RUSTUP_BIN),--rustup "$(RUSTUP_BIN)",) $(if $(ZIG_BIN),--zig "$(ZIG_BIN)",)

# Stage Linux package payloads, render nFPM manifests, and optionally build deb/rpm artifacts
build-linux-release-packages:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(NIMBUS_BINARY)" || (echo "set NIMBUS_BINARY=/absolute/path/to/nimbus" && exit 1)
	@test -n "$(NIMBUS_LIBKRUN_ARCHIVE)" || (echo "set NIMBUS_LIBKRUN_ARCHIVE=/absolute/path/to/nimbus-libkrun-linux-<arch>.tar.gz" && exit 1)
	@test -n "$(NIMBUS_CRUN_BINARY)" || (echo "set NIMBUS_CRUN_BINARY=/absolute/path/to/nimbus-crun" && exit 1)
	@test -n "$(VERSION)" || (echo "set VERSION=X.Y.Z or VERSION=vX.Y.Z" && exit 1)
	@test -n "$(LIBKRUN_VERSION)" || (echo "set LIBKRUN_VERSION=X.Y.Z or LIBKRUN_VERSION=vX.Y.Z" && exit 1)
	bash scripts/build-linux-release-packages.sh --output-dir "$(OUTPUT_DIR)" --nimbus-binary "$(NIMBUS_BINARY)" --nimbus-libkrun-archive "$(NIMBUS_LIBKRUN_ARCHIVE)" --nimbus-crun-binary "$(NIMBUS_CRUN_BINARY)" --version "$(VERSION)" --libkrun-version "$(LIBKRUN_VERSION)" $(if $(CRUN_VERSION),--crun-version "$(CRUN_VERSION)",) $(if $(ARCH),--arch "$(ARCH)",) $(foreach format,$(FORMAT),--format "$(format)") $(if $(NFPM),--nfpm "$(NFPM)",) $(if $(RENDER_ONLY),--render-only,)

# Build a static Debian/Ubuntu apt repository tree from prebuilt .deb packages
build-apt-repository:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(PACKAGES_DIR)" || (echo "set PACKAGES_DIR=/absolute/path/to/packages-dir" && exit 1)
	bash scripts/build-apt-repository.sh --output-dir "$(OUTPUT_DIR)" --packages-dir "$(PACKAGES_DIR)" $(if $(DISTRIBUTION),--distribution "$(DISTRIBUTION)",) $(if $(SUITE),--suite "$(SUITE)",) $(if $(COMPONENT),--component "$(COMPONENT)",) $(if $(ORIGIN),--origin "$(ORIGIN)",) $(if $(LABEL),--label "$(LABEL)",) $(if $(DESCRIPTION),--description "$(DESCRIPTION)",) $(foreach arch,$(ARCH),--arch "$(arch)") $(if $(APT_FTPARCHIVE),--apt-ftparchive "$(APT_FTPARCHIVE)",) $(if $(GPG_BIN),--gpg "$(GPG_BIN)",) $(if $(GPG_PRIVATE_KEY),--gpg-private-key "$(GPG_PRIVATE_KEY)",) $(if $(GPG_KEY_ID),--gpg-key-id "$(GPG_KEY_ID)",) $(if $(GPG_PASSPHRASE_FILE),--gpg-passphrase-file "$(GPG_PASSPHRASE_FILE)",) $(if $(KEYRING_NAME),--keyring-name "$(KEYRING_NAME)",)

# Build Fedora/COPR SRPMs from published Nimbus release artifacts
build-fedora-release-srpms:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(NIMBUS_VERSION)" || (echo "set NIMBUS_VERSION=X.Y.Z or NIMBUS_VERSION=vX.Y.Z" && exit 1)
	@test -n "$(NIMBUS_LINUX_AMD64_TARBALL)" || (echo "set NIMBUS_LINUX_AMD64_TARBALL=/absolute/path/to/nimbus_linux_x86_64.tar.gz" && exit 1)
	@test -n "$(NIMBUS_LINUX_ARM64_TARBALL)" || (echo "set NIMBUS_LINUX_ARM64_TARBALL=/absolute/path/to/nimbus_linux_arm64.tar.gz" && exit 1)
	@test -n "$(NIMBUS_LIBKRUN_VERSION)" || (echo "set NIMBUS_LIBKRUN_VERSION=X.Y.Z or NIMBUS_LIBKRUN_VERSION=vX.Y.Z" && exit 1)
	@test -n "$(NIMBUS_LIBKRUN_LINUX_AMD64_ARCHIVE)" || (echo "set NIMBUS_LIBKRUN_LINUX_AMD64_ARCHIVE=/absolute/path/to/nimbus-libkrun-linux-amd64.tar.gz" && exit 1)
	@test -n "$(NIMBUS_LIBKRUN_LINUX_ARM64_ARCHIVE)" || (echo "set NIMBUS_LIBKRUN_LINUX_ARM64_ARCHIVE=/absolute/path/to/nimbus-libkrun-linux-arm64.tar.gz" && exit 1)
	@test -n "$(NIMBUS_CRUN_VERSION)" || (echo "set NIMBUS_CRUN_VERSION=X.Y.Z or NIMBUS_CRUN_VERSION=vX.Y.Z" && exit 1)
	@test -n "$(NIMBUS_CRUN_LINUX_AMD64)" || (echo "set NIMBUS_CRUN_LINUX_AMD64=/absolute/path/to/nimbus-crun-linux-amd64" && exit 1)
	@test -n "$(NIMBUS_CRUN_LINUX_ARM64)" || (echo "set NIMBUS_CRUN_LINUX_ARM64=/absolute/path/to/nimbus-crun-linux-arm64" && exit 1)
	bash scripts/build-fedora-release-srpms.sh --output-dir "$(OUTPUT_DIR)" --nimbus-version "$(NIMBUS_VERSION)" --nimbus-linux-amd64-tarball "$(NIMBUS_LINUX_AMD64_TARBALL)" --nimbus-linux-arm64-tarball "$(NIMBUS_LINUX_ARM64_TARBALL)" --nimbus-libkrun-version "$(NIMBUS_LIBKRUN_VERSION)" --nimbus-libkrun-linux-amd64-archive "$(NIMBUS_LIBKRUN_LINUX_AMD64_ARCHIVE)" --nimbus-libkrun-linux-arm64-archive "$(NIMBUS_LIBKRUN_LINUX_ARM64_ARCHIVE)" --nimbus-crun-version "$(NIMBUS_CRUN_VERSION)" --nimbus-crun-linux-amd64 "$(NIMBUS_CRUN_LINUX_AMD64)" --nimbus-crun-linux-arm64 "$(NIMBUS_CRUN_LINUX_ARM64)" $(if $(RELEASE),--release "$(RELEASE)",) $(if $(RPMBUILD),--rpmbuild "$(RPMBUILD)",) $(if $(RENDER_ONLY),--render-only,)

# Check whether a Podman/libkrun machine tmp root will overflow Darwin's unix-socket path budget
check-podman-machine-socket-paths:
	@test -n "$(MACHINE)" || (echo "set MACHINE=<podman-machine-name>" && exit 1)
	bash scripts/check-podman-machine-socket-paths.sh --machine "$(MACHINE)" $(if $(TMP_ROOT),--tmp-root "$(TMP_ROOT)",) $(if $(SOCKET_BYTE_LIMIT),--socket-byte-limit "$(SOCKET_BYTE_LIMIT)",)

# Validate that a running Podman machine stays reachable via its named connection and machine ssh
validate-podman-machine-readiness:
	@test -n "$(MACHINE)" || (echo "set MACHINE=<podman-machine-name>" && exit 1)
	bash scripts/validate-podman-machine-readiness.sh --machine "$(MACHINE)" $(if $(CONNECTION),--connection "$(CONNECTION)",) $(if $(PROVIDER),--provider "$(PROVIDER)",) $(if $(TMP_ROOT),--tmp-root "$(TMP_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(PODMAN),--podman "$(PODMAN)",) $(if $(PS),--ps "$(PS)",) $(if $(SYSTEM_PROFILER),--system-profiler "$(SYSTEM_PROFILER)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",) $(if $(SSH_COMMAND),--ssh-command "$(SSH_COMMAND)",)

# Recreate a Podman machine with the short-runtime-dir recipe and capture readiness artifacts
recreate-podman-machine:
	@test -n "$(MACHINE)" || (echo "set MACHINE=<podman-machine-name>" && exit 1)
	bash scripts/recreate-podman-machine.sh --machine "$(MACHINE)" $(if $(CONNECTION),--connection "$(CONNECTION)",) $(if $(PROVIDER),--provider "$(PROVIDER)",) $(if $(TMP_ROOT),--tmp-root "$(TMP_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(CPUS),--cpus "$(CPUS)",) $(if $(MEMORY),--memory "$(MEMORY)",) $(if $(DISK_SIZE),--disk-size "$(DISK_SIZE)",) $(if $(VOLUME),--volume "$(VOLUME)",) $(if $(SKIP_PRE_DIAGNOSTICS),--skip-pre-diagnostics,) $(if $(PODMAN),--podman "$(PODMAN)",) $(if $(PS),--ps "$(PS)",) $(if $(SYSTEM_PROFILER),--system-profiler "$(SYSTEM_PROFILER)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",) $(if $(SSH_COMMAND),--ssh-command "$(SSH_COMMAND)",)

# Recreate a Nimbus machine with the shipped machine CLI and capture diagnostics artifacts
recreate-nimbus-machine:
	bash scripts/recreate-nimbus-machine.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NIMBUS),--nimbus "$(NIMBUS)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(BOOTC_NATIVE),--bootc-native,) $(if $(MACHINE_OS_REPO),--machine-os-repo "$(MACHINE_OS_REPO)",) $(if $(MACHINE_OS_BUILDER),--machine-os-builder "$(MACHINE_OS_BUILDER)",) $(if $(MACHINE_OS_BUILDER_REPO),--machine-os-builder-repo "$(MACHINE_OS_BUILDER_REPO)",) $(if $(MACHINE_OS_BUILDER_WORK_DIR),--machine-os-builder-work-dir "$(MACHINE_OS_BUILDER_WORK_DIR)",) $(if $(SSH_IDENTITY),--identity "$(SSH_IDENTITY)",) $(if $(IGNITION_FILE),--ignition-path "$(IGNITION_FILE)",) $(if $(EFI_STORE),--firmware "$(EFI_STORE)",) $(if $(CPUS),--cpus "$(CPUS)",) $(if $(MEMORY_MIB),--memory "$(MEMORY_MIB)",) $(if $(DISK_GIB),--disk-size "$(DISK_GIB)",) $(if $(VOLUME),--volume "$(VOLUME)",) $(if $(SKIP_PRE_DIAGNOSTICS),--skip-pre-diagnostics,) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Prepare a deterministic Linux-host LH1-LH6 execution bundle
prepare-linux-vmm-validation-bundle:
	@if [ -z "$(CRUN_SRC)" ] && { [ -z "$(NIMBUS_CRUN_ASSET)" ] || [ -z "$(NIMBUS_LIBKRUN_ARCHIVE)" ]; }; then echo "set NIMBUS_CRUN_ASSET and NIMBUS_LIBKRUN_ARCHIVE, or set CRUN_SRC for source diagnostics"; exit 1; fi
	bash scripts/prepare-linux-vmm-validation-bundle.sh $(if $(CRUN_SRC),--crun-source "$(CRUN_SRC)",) $(if $(NIMBUS_CRUN_ASSET),--nimbus-crun-asset "$(NIMBUS_CRUN_ASSET)",) $(if $(NIMBUS_LIBKRUN_ARCHIVE),--nimbus-libkrun-archive "$(NIMBUS_LIBKRUN_ARCHIVE)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(STAGE_DIR),--stage-dir "$(STAGE_DIR)",) $(if $(STAGE_BINARY),--stage-binary "$(STAGE_BINARY)",) $(if $(INSTALL_PATH),--install-path "$(INSTALL_PATH)",) $(if $(SYSTEM_RUNTIME),--system-runtime "$(SYSTEM_RUNTIME)",) $(if $(BUNDLE_DIR),--bundle-dir "$(BUNDLE_DIR)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(BUILDAH_NAME),--buildah-name "$(BUILDAH_NAME)",) $(if $(HOST_PORT),--host-port "$(HOST_PORT)",) $(if $(GUEST_PORT),--guest-port "$(GUEST_PORT)",) $(if $(DIRECT_STATE_ROOT),--direct-state-root "$(DIRECT_STATE_ROOT)",) $(if $(DIRECT_CONTAINER_ID),--direct-container-id "$(DIRECT_CONTAINER_ID)",) $(if $(CONMON_STATE_ROOT),--conmon-state-root "$(CONMON_STATE_ROOT)",) $(if $(CONMON),--conmon "$(CONMON)",) $(if $(CONMON_NAME),--conmon-name "$(CONMON_NAME)",) $(if $(PROBE_HOST),--probe-host "$(PROBE_HOST)",) $(if $(PROBE_PATH),--probe-path "$(PROBE_PATH)",)

# Prepare a krun OCI bundle config with the correct annotations and port mapping shape
prepare-krun-bundle:
	@test -n "$(BUNDLE_DIR)" || (echo "set BUNDLE_DIR=/absolute/path/to/bundle-dir" && exit 1)
	@test -n "$(ROOTFS)" || (echo "set ROOTFS=/absolute/path/to/rootfs" && exit 1)
	@test -n "$(HOST_PORT)" || (echo "set HOST_PORT=<host-port>" && exit 1)
	@test -n "$(GUEST_PORT)" || (echo "set GUEST_PORT=<guest-port>" && exit 1)
	bash scripts/prepare-krun-bundle.sh --bundle-dir "$(BUNDLE_DIR)" --rootfs "$(ROOTFS)" --host-port "$(HOST_PORT)" --guest-port "$(GUEST_PORT)" $(if $(RUNTIME),--runtime "$(RUNTIME)",)

# Verify the krun bundle helper against a checked-in config fixture
verify-krun-bundle-helper:
	bash scripts/verify-krun-bundle-helper.sh

# Prepare a deterministic direct private-runtime krun drill layout for Linux host execution
prepare-direct-krun-drill:
	@test -n "$(BUNDLE_DIR)" || (echo "set BUNDLE_DIR=/absolute/path/to/bundle-dir" && exit 1)
	bash scripts/prepare-direct-krun-drill.sh --bundle-dir "$(BUNDLE_DIR)" $(if $(STATE_ROOT),--state-root "$(STATE_ROOT)",) $(if $(CONTAINER_ID),--container-id "$(CONTAINER_ID)",) $(if $(RUNTIME),--runtime "$(RUNTIME)",) $(if $(HOST_PORT),--host-port "$(HOST_PORT)",) $(if $(PROBE_HOST),--probe-host "$(PROBE_HOST)",) $(if $(PROBE_PATH),--probe-path "$(PROBE_PATH)",) $(if $(COMMAND_FILE),--command-file "$(COMMAND_FILE)",)

# Verify the direct private-runtime krun drill helper against a temporary bundle
verify-direct-krun-drill-helper:
	bash scripts/verify-direct-krun-drill-helper.sh

# Verify that the system runtime remains separate from the private nimbus runtime path
verify-runtime-separation:
	bash scripts/verify-runtime-separation.sh $(if $(SYSTEM_RUNTIME),--system-runtime "$(SYSTEM_RUNTIME)",) $(if $(PRIVATE_RUNTIME),--private-runtime "$(PRIVATE_RUNTIME)",) $(if $(PODMAN),--podman "$(PODMAN)",)

# Verify the runtime-separation helper against temporary fake runtimes
verify-runtime-separation-helper:
	bash scripts/verify-runtime-separation-helper.sh

# Verify the Podman machine diagnostics helper against deterministic fake host artifacts
verify-podman-machine-diagnostics-helper:
	bash scripts/verify-podman-machine-diagnostics-helper.sh

# Verify the Podman/libkrun socket-path helper against deterministic long-root and /tmp cases
verify-podman-machine-socket-paths-helper:
	bash scripts/verify-podman-machine-socket-paths-helper.sh

# Verify the Podman machine readiness helper against deterministic fake host artifacts
verify-podman-machine-readiness-helper:
	bash scripts/verify-podman-machine-readiness-helper.sh

# Verify the Podman machine recreate helper against deterministic fake host artifacts
verify-podman-machine-recreate-helper:
	bash scripts/verify-podman-machine-recreate-helper.sh

# Verify the Nimbus machine diagnostics helper against deterministic fake host artifacts
verify-nimbus-machine-diagnostics-helper:
	bash scripts/verify-nimbus-machine-diagnostics-helper.sh

# Verify the Nimbus machine recreate helper against deterministic fake host artifacts
verify-nimbus-machine-recreate-helper:
	bash scripts/verify-nimbus-machine-recreate-helper.sh

# Verify the isolated local-binary machine CLI proof helper against deterministic fake host artifacts
verify-nimbus-machine-cli-proof-helper:
	bash scripts/verify-nimbus-machine-cli-proof-helper.sh

# Verify the Nimbus machine guest-proof helper against deterministic fake guest artifacts
verify-nimbus-machine-guest-proof-helper:
	bash scripts/verify-nimbus-machine-guest-proof-helper.sh

# Verify the machine guest-binary build helper against deterministic fake cargo/rustup/zig shims
verify-build-nimbus-machine-guest-binary-helper:
	bash scripts/verify-build-nimbus-machine-guest-binary-helper.sh

# Verify the Linux package builder helper against deterministic staged binaries and manifests
verify-build-linux-release-packages-helper:
	bash scripts/verify-build-linux-release-packages-helper.sh

# Verify the apt repository builder helper against deterministic stub packages and signed metadata
verify-build-apt-repository-helper:
	bash scripts/verify-build-apt-repository-helper.sh

# Verify the Fedora/COPR SRPM builder against deterministic release-asset stubs and Fedora userspace
verify-build-fedora-release-srpms-helper:
	bash scripts/verify-build-fedora-release-srpms-helper.sh

# Verify the Nimbus machine service-proof helper against deterministic fake host artifacts
verify-nimbus-machine-service-proof-helper:
	bash scripts/verify-nimbus-machine-service-proof-helper.sh

# Verify the Nimbus Homebrew/cask proof helper against deterministic fake brew and guest artifacts
verify-nimbus-homebrew-cask-proof-helper:
	bash scripts/verify-nimbus-homebrew-cask-proof-helper.sh

# Verify the install script helper against deterministic inputs
verify-install-helper:
	bash scripts/verify-install-helper.sh

# machine-os build/package/publish targets moved to nimbus/machine-os

# Verify the Linux-host LH1-LH6 command-bundle generator against deterministic fake inputs
verify-linux-vmm-validation-bundle-helper:
	bash scripts/verify-linux-vmm-validation-bundle-helper.sh

# Prepare a deterministic conmon -> patched-crun drill layout for Linux host execution
prepare-conmon-krun-drill:
	@test -n "$(BUNDLE_DIR)" || (echo "set BUNDLE_DIR=/absolute/path/to/bundle-dir" && exit 1)
	bash scripts/prepare-conmon-krun-drill.sh --bundle-dir "$(BUNDLE_DIR)" $(if $(STATE_ROOT),--state-root "$(STATE_ROOT)",) $(if $(CONTAINER_ID),--container-id "$(CONTAINER_ID)",) $(if $(NAME),--name "$(NAME)",) $(if $(CONMON),--conmon "$(CONMON)",) $(if $(RUNTIME),--runtime "$(RUNTIME)",) $(if $(COMMAND_FILE),--command-file "$(COMMAND_FILE)",) $(if $(TERMINAL),--terminal,)

# Verify the conmon -> patched-crun drill helper against a temporary bundle
verify-conmon-krun-drill-helper:
	bash scripts/verify-conmon-krun-drill-helper.sh

# Prepare an upstream convex-demos overlay, then run codegen + Nimbus against it
convex-demo: convex-demo-stop
	@test -n "$(CONVEX_DEMOS_DIR)" || (echo "Set CONVEX_DEMOS_DIR in .env first" && exit 1)
	@test -n "$(DEMO)" || (echo "Usage: make convex-demo DEMO=node|html|http" && exit 1)
	@overlay_dir="$$(node ./scripts/convex-demo-overlay.mjs "$(CONVEX_DEMOS_DIR)" "$(DEMO)")"; \
	echo "Prepared overlay at $$overlay_dir"; \
	cargo run -p nimbus-bin -- codegen --app "$$overlay_dir"; \
	cargo run -p nimbus-bin -- start --port 8080 --app-dir "$$overlay_dir"

convex-demo-node: DEMO=node
convex-demo-node: convex-demo

convex-demo-html: DEMO=html
convex-demo-html: convex-demo

convex-demo-http: DEMO=http
convex-demo-http: convex-demo

convex-demo-stop:
	bash scripts/stop-demo-processes.sh

# Required local CI-shaped check. Hosted CI still owns coverage upload and the
# scheduled/manual Node compatibility evidence workflow.
ci-required: $(UI_DIST_INDEX) fmt-check clippy deny test-rust-runtime test-rust-workspace test-rust-docs verify-harness build-js typecheck-js test-js proof-helpers

ci: ci-required

# Install the CLI binary to ~/.cargo/bin
install:
	cargo install --path crates/nimbus-bin

# Regenerate CHANGELOG.md from conventional commits
changelog:
	git-cliff --output CHANGELOG.md

# Remove build artifacts
clean:
	cargo clean
