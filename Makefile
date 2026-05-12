-include .env
export

.PHONY: all build release check fmt fmt-check clippy test test-js build-js lint deny ci install clean changelog verify-release-version-contract verify-release-archive-layout-helper verify-harness verify-harness-nightly verify-harness-repro verify-harness-storage verify-harness-engine verify-harness-server verify-harness-runtime verify-harness-nightly-storage verify-harness-nightly-engine verify-harness-nightly-server verify-harness-nightly-runtime node-compat-report node-compat-dashboard node-compat-status node-compat-inventory node-compat-classifications node-compat-sync node-compat-refresh node-compat-publish-evidence node-compat-publish-docs node-compat-trends node-compat-expectations-sync node-compat-expectations-validate node-compat-oracle node-compat-canaries-bootstrap node-compat-canaries node-compat-validate-claims check-vmm-host collect-vmm-package-versions collect-podman-machine-diagnostics collect-neovex-machine-diagnostics collect-neovex-machine-cli-proof collect-neovex-machine-guest-proof collect-neovex-machine-service-proof collect-neovex-homebrew-cask-proof collect-sqlcipher-proof-bundles collect-encryption-benchmark-evidence build-neovex-machine-guest-binary build-linux-release-packages build-apt-repository build-fedora-release-srpms check-podman-machine-socket-paths validate-podman-machine-readiness recreate-podman-machine recreate-neovex-machine prepare-linux-vmm-validation-bundle verify-build-neovex-machine-guest-binary-helper verify-build-linux-release-packages-helper verify-build-apt-repository-helper verify-build-fedora-release-srpms-helper verify-podman-machine-socket-paths-helper verify-podman-machine-readiness-helper verify-podman-machine-recreate-helper verify-neovex-machine-diagnostics-helper verify-neovex-machine-recreate-helper verify-neovex-machine-cli-proof-helper verify-neovex-machine-guest-proof-helper verify-neovex-machine-service-proof-helper verify-neovex-homebrew-cask-proof-helper verify-collect-sqlcipher-proof-bundles-helper verify-install-helper verify-linux-vmm-validation-bundle-helper prepare-krun-bundle verify-krun-bundle-helper prepare-direct-krun-drill verify-direct-krun-drill-helper verify-runtime-separation verify-runtime-separation-helper verify-podman-machine-diagnostics-helper prepare-conmon-krun-drill verify-conmon-krun-drill-helper bench-embedded-providers bench-postgres-provider bench-mysql-provider bench-libsql-replica-provider convex-demo convex-demo-node convex-demo-html convex-demo-http convex-demo-stop

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
	cargo bench -p neovex-engine --bench embedded-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),) $(if $(ENCRYPTION),--local-encryption $(ENCRYPTION),)

# Benchmark the Postgres provider against embedded SQLite plus injected RTT sensitivity
bench-postgres-provider:
	cargo bench -p neovex-engine --bench postgres-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the MySQL provider against embedded SQLite plus injected RTT sensitivity
bench-mysql-provider:
	cargo bench -p neovex-engine --bench mysql-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(if $(WORKLOAD),--workload $(WORKLOAD),)

# Benchmark the libsql replica provider against embedded SQLite plus replica-specific catch-up drills
bench-libsql-replica-provider:
	cargo bench -p neovex-engine --bench libsql-replica-provider-benchmarks -- $(if $(REPORT),--markdown $(REPORT),) $(foreach workload,$(WORKLOADS),--workload $(workload)) $(if $(WORKLOAD),--workload $(WORKLOAD),) $(if $(ENCRYPTION),--local-cache-encryption $(ENCRYPTION),)

collect-encryption-benchmark-evidence:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/path/to/output-dir" && exit 1)
	bash scripts/collect-encryption-benchmark-evidence.sh --output-dir "$(OUTPUT_DIR)"

# Dependency audit (licenses + vulnerabilities)
deny:
	$(SINGLE_FLIGHT) --key cargo-deny-check -- cargo deny check

# Verify that release tags, crate/package versions, and changelog entry agree
verify-release-version-contract:
	@test -n "$(VERSION)" || (echo "set VERSION=vX.Y.Z or VERSION=X.Y.Z" && exit 1)
	bash scripts/verify-release-version-contract.sh "$(VERSION)"

# Verify the published release archive layout contract, including the macOS helper bundle
verify-release-archive-layout-helper:
	bash scripts/verify-release-archive-layout-helper.sh

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
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24, or another checked-in nodeNN lane" && exit 1)
	python3 scripts/runtime/node/sync.py --lane "$(LANE)" $(if $(TAG),--upstream-tag "$(TAG)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(DRY_RUN),--dry-run,) $(if $(COMPARE_UPSTREAM),--compare-upstream,) $(if $(APPLY),--apply,) $(if $(FORCE),--force,)

node-compat-refresh:
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24, or another checked-in nodeNN lane" && exit 1)
	python3 scripts/runtime/node/refresh.py --lane "$(LANE)" $(if $(TAG),--tag "$(TAG)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(DRY_RUN),--dry-run,) $(if $(COMPARE_UPSTREAM),--compare-upstream,) $(if $(APPLY),--apply,) $(if $(FORCE),--force,) $(if $(RUN_SLICES),--run-representative-slices,)

node-compat-publish-evidence:
	python3 scripts/runtime/node/publish_evidence.py $(if $(ARTIFACTS_ROOT),--artifacts-root "$(ARTIFACTS_ROOT)",) $(if $(PUBLISH_ROOT),--publish-root "$(PUBLISH_ROOT)",)

node-compat-publish-docs:
	python3 scripts/runtime/node/publish_docs.py $(if $(EVIDENCE_ROOT),--evidence-root "$(EVIDENCE_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-trends:
	python3 scripts/runtime/node/trends.py $(if $(ARTIFACTS_ROOT),--artifacts-root "$(ARTIFACTS_ROOT)",) $(if $(BASELINE_ROOT),--baseline-root "$(BASELINE_ROOT)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-expectations-sync:
	python3 scripts/runtime/node/expectations.py sync $(if $(EXPECTATION_CATALOG),--catalog "$(EXPECTATION_CATALOG)",)

node-compat-expectations-validate:
	python3 scripts/runtime/node/expectations.py validate $(if $(EXPECTATION_CATALOG),--catalog "$(EXPECTATION_CATALOG)",) $(if $(OBSERVED_RESULTS),--observed-results "$(OBSERVED_RESULTS)",)

node-compat-oracle:
	@test -n "$(LANE)" || (echo "set LANE=node20|node22|node24, or another checked-in nodeNN lane" && exit 1)
	@test -n "$(SAMPLE)" || (echo "set SAMPLE=test/parallel/test-buffer-alloc.js" && exit 1)
	bash scripts/runtime/node/oracle-run.sh --lane "$(LANE)" --fixture "$(SAMPLE)" $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(NODE_BIN),--node-bin "$(NODE_BIN)",)

node-compat-canaries-bootstrap:
	bash scripts/runtime/node/canaries-bootstrap.sh $(if $(PRESET),--preset "$(PRESET)",)

node-compat-canaries:
	@test -n "$(PRESET)" || (echo "set PRESET=application|tooling" && exit 1)
	bash scripts/runtime/node/canaries-run.sh --preset "$(PRESET)" $(if $(LANE),--lane "$(LANE)",) $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",)

node-compat-validate-claims:
	bash scripts/runtime/node/validate-claims.sh

# crun patch/build/verify targets moved to agentstation/neovex-crun

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

# Collect best-effort Neovex machine diagnostics for the macOS manager lane
collect-neovex-machine-diagnostics:
	bash scripts/collect-neovex-machine-diagnostics.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(CONFIG_ROOT),--config-root "$(CONFIG_ROOT)",) $(if $(STATE_ROOT),--state-root "$(STATE_ROOT)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NEOVEX),--neovex "$(NEOVEX)",) $(if $(PS),--ps "$(PS)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Collect isolated-root local-binary proof for `neovex machine ...` without touching default roots
collect-neovex-machine-cli-proof:
	bash scripts/collect-neovex-machine-cli-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(ROOT),--root "$(ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NEOVEX),--neovex "$(NEOVEX)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(GUEST_BINARY),--guest-binary "$(GUEST_BINARY)",) $(if $(SCRIPT),--script "$(SCRIPT)",) $(if $(KEEP_MACHINE),--keep-machine,)

# Collect guest-image contract proof from a booted Neovex machine via `machine ssh`
collect-neovex-machine-guest-proof:
	bash scripts/collect-neovex-machine-guest-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NEOVEX),--neovex "$(NEOVEX)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(GUEST_VOLUME_PATH),--guest-volume-path "$(GUEST_VOLUME_PATH)",) $(if $(GUEST_SOCKET_PATH),--guest-socket-path "$(GUEST_SOCKET_PATH)",) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Collect forwarded machine-API and host `neovex service ...` proof from a booted Neovex machine
collect-neovex-machine-service-proof:
	@test -n "$(COMPOSE_FILE)" || (echo "set COMPOSE_FILE=/absolute/path/to/compose.yaml" && exit 1)
	@test -n "$(SERVICE)" || (echo "set SERVICE=<service-name>" && exit 1)
	bash scripts/collect-neovex-machine-service-proof.sh --compose-file "$(COMPOSE_FILE)" --service "$(SERVICE)" $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NEOVEX),--neovex "$(NEOVEX)",) $(if $(CURL),--curl "$(CURL)",) $(if $(PUBLISHED_URL),--published-url "$(PUBLISHED_URL)",)

# Collect real-host proof for the supported macOS Homebrew/cask install surface on isolated roots
collect-neovex-homebrew-cask-proof:
	bash scripts/collect-neovex-homebrew-cask-proof.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(HOST_BINARY),--host-binary "$(HOST_BINARY)",) $(if $(GUEST_BINARY),--guest-binary "$(GUEST_BINARY)",) $(if $(GVPROXY),--gvproxy "$(GVPROXY)",) $(if $(BREW),--brew "$(BREW)",) $(if $(READLINK),--readlink "$(READLINK)",) $(if $(SSH_KEYGEN),--ssh-keygen "$(SSH_KEYGEN)",) $(if $(TAP),--tap "$(TAP)",) $(if $(CASK),--cask "$(CASK)",) $(if $(KEEP_INSTALLED),--keep-installed,)

# Download SQLCipher proof bundles from a hosted GitHub Actions workflow run
collect-sqlcipher-proof-bundles:
	@test -n "$(RUN_ID)" || (echo "set RUN_ID=<github-actions-run-id>" && exit 1)
	bash scripts/collect-sqlcipher-proof-bundles.sh --run-id "$(RUN_ID)" $(if $(REPO),--repo "$(REPO)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(ARTIFACT_PREFIX),--artifact-prefix "$(ARTIFACT_PREFIX)",)

# Verify the SQLCipher proof bundle collector against a deterministic fake GitHub CLI
verify-collect-sqlcipher-proof-bundles-helper:
	bash scripts/verify-collect-sqlcipher-proof-bundles-helper.sh

# Build the Linux guest neovex binary that macOS machine-start prefers before release downloads
build-neovex-machine-guest-binary:
	bash scripts/build-neovex-machine-guest-binary.sh $(if $(TARGET),--target "$(TARGET)",) $(if $(PROFILE),--profile "$(PROFILE)",) $(if $(COPY_TO),--copy-to "$(COPY_TO)",) $(if $(CACHE_ROOT),--cache-root "$(CACHE_ROOT)",) $(if $(CARGO_BIN),--cargo "$(CARGO_BIN)",) $(if $(RUSTUP_BIN),--rustup "$(RUSTUP_BIN)",) $(if $(ZIG_BIN),--zig "$(ZIG_BIN)",)

# Stage Linux package payloads, render nFPM manifests, and optionally build deb/rpm artifacts
build-linux-release-packages:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(NEOVEX_BINARY)" || (echo "set NEOVEX_BINARY=/absolute/path/to/neovex" && exit 1)
	@test -n "$(NEOVEX_CRUN_BINARY)" || (echo "set NEOVEX_CRUN_BINARY=/absolute/path/to/neovex-crun" && exit 1)
	@test -n "$(VERSION)" || (echo "set VERSION=X.Y.Z or VERSION=vX.Y.Z" && exit 1)
	bash scripts/build-linux-release-packages.sh --output-dir "$(OUTPUT_DIR)" --neovex-binary "$(NEOVEX_BINARY)" --neovex-crun-binary "$(NEOVEX_CRUN_BINARY)" --version "$(VERSION)" $(if $(CRUN_VERSION),--crun-version "$(CRUN_VERSION)",) $(if $(ARCH),--arch "$(ARCH)",) $(foreach format,$(FORMAT),--format "$(format)") $(if $(NFPM),--nfpm "$(NFPM)",) $(if $(RENDER_ONLY),--render-only,)

# Build a static Debian/Ubuntu apt repository tree from prebuilt .deb packages
build-apt-repository:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(PACKAGES_DIR)" || (echo "set PACKAGES_DIR=/absolute/path/to/packages-dir" && exit 1)
	bash scripts/build-apt-repository.sh --output-dir "$(OUTPUT_DIR)" --packages-dir "$(PACKAGES_DIR)" $(if $(DISTRIBUTION),--distribution "$(DISTRIBUTION)",) $(if $(SUITE),--suite "$(SUITE)",) $(if $(COMPONENT),--component "$(COMPONENT)",) $(if $(ORIGIN),--origin "$(ORIGIN)",) $(if $(LABEL),--label "$(LABEL)",) $(if $(DESCRIPTION),--description "$(DESCRIPTION)",) $(foreach arch,$(ARCH),--arch "$(arch)") $(if $(APT_FTPARCHIVE),--apt-ftparchive "$(APT_FTPARCHIVE)",) $(if $(GPG_BIN),--gpg "$(GPG_BIN)",) $(if $(GPG_PRIVATE_KEY),--gpg-private-key "$(GPG_PRIVATE_KEY)",) $(if $(GPG_KEY_ID),--gpg-key-id "$(GPG_KEY_ID)",) $(if $(GPG_PASSPHRASE_FILE),--gpg-passphrase-file "$(GPG_PASSPHRASE_FILE)",) $(if $(KEYRING_NAME),--keyring-name "$(KEYRING_NAME)",)

# Build Fedora/COPR SRPMs from published Neovex release artifacts
build-fedora-release-srpms:
	@test -n "$(OUTPUT_DIR)" || (echo "set OUTPUT_DIR=/absolute/path/to/output-dir" && exit 1)
	@test -n "$(NEOVEX_VERSION)" || (echo "set NEOVEX_VERSION=X.Y.Z or NEOVEX_VERSION=vX.Y.Z" && exit 1)
	@test -n "$(NEOVEX_LINUX_AMD64_TARBALL)" || (echo "set NEOVEX_LINUX_AMD64_TARBALL=/absolute/path/to/neovex_linux_x86_64.tar.gz" && exit 1)
	@test -n "$(NEOVEX_LINUX_ARM64_TARBALL)" || (echo "set NEOVEX_LINUX_ARM64_TARBALL=/absolute/path/to/neovex_linux_arm64.tar.gz" && exit 1)
	@test -n "$(NEOVEX_CRUN_VERSION)" || (echo "set NEOVEX_CRUN_VERSION=X.Y.Z or NEOVEX_CRUN_VERSION=vX.Y.Z" && exit 1)
	@test -n "$(NEOVEX_CRUN_LINUX_AMD64)" || (echo "set NEOVEX_CRUN_LINUX_AMD64=/absolute/path/to/neovex-crun-linux-amd64" && exit 1)
	@test -n "$(NEOVEX_CRUN_LINUX_ARM64)" || (echo "set NEOVEX_CRUN_LINUX_ARM64=/absolute/path/to/neovex-crun-linux-arm64" && exit 1)
	bash scripts/build-fedora-release-srpms.sh --output-dir "$(OUTPUT_DIR)" --neovex-version "$(NEOVEX_VERSION)" --neovex-linux-amd64-tarball "$(NEOVEX_LINUX_AMD64_TARBALL)" --neovex-linux-arm64-tarball "$(NEOVEX_LINUX_ARM64_TARBALL)" --neovex-crun-version "$(NEOVEX_CRUN_VERSION)" --neovex-crun-linux-amd64 "$(NEOVEX_CRUN_LINUX_AMD64)" --neovex-crun-linux-arm64 "$(NEOVEX_CRUN_LINUX_ARM64)" $(if $(RELEASE),--release "$(RELEASE)",) $(if $(RPMBUILD),--rpmbuild "$(RPMBUILD)",) $(if $(RENDER_ONLY),--render-only,)

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

# Recreate a Neovex machine with the shipped machine CLI and capture diagnostics artifacts
recreate-neovex-machine:
	bash scripts/recreate-neovex-machine.sh $(if $(MACHINE),--machine "$(MACHINE)",) $(if $(HOME_DIR),--home "$(HOME_DIR)",) $(if $(RUNTIME_ROOT),--runtime-root "$(RUNTIME_ROOT)",) $(if $(OUTPUT_DIR),--output-dir "$(OUTPUT_DIR)",) $(if $(NEOVEX),--neovex "$(NEOVEX)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(SSH_IDENTITY),--identity "$(SSH_IDENTITY)",) $(if $(IGNITION_FILE),--ignition-path "$(IGNITION_FILE)",) $(if $(EFI_STORE),--firmware "$(EFI_STORE)",) $(if $(CPUS),--cpus "$(CPUS)",) $(if $(MEMORY_MIB),--memory "$(MEMORY_MIB)",) $(if $(DISK_GIB),--disk-size "$(DISK_GIB)",) $(if $(VOLUME),--volume "$(VOLUME)",) $(if $(SKIP_PRE_DIAGNOSTICS),--skip-pre-diagnostics,) $(if $(LOG_LINES),--log-lines "$(LOG_LINES)",)

# Prepare a deterministic Linux-host LH1-LH6 execution bundle
prepare-linux-vmm-validation-bundle:
	@test -n "$(CRUN_SRC)" || (echo "set CRUN_SRC=/absolute/path/to/crun-source" && exit 1)
	bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source "$(CRUN_SRC)" $(if $(OUTPUT_ROOT),--output-root "$(OUTPUT_ROOT)",) $(if $(STAGE_DIR),--stage-dir "$(STAGE_DIR)",) $(if $(STAGE_BINARY),--stage-binary "$(STAGE_BINARY)",) $(if $(INSTALL_PATH),--install-path "$(INSTALL_PATH)",) $(if $(SYSTEM_RUNTIME),--system-runtime "$(SYSTEM_RUNTIME)",) $(if $(BUNDLE_DIR),--bundle-dir "$(BUNDLE_DIR)",) $(if $(IMAGE),--image "$(IMAGE)",) $(if $(BUILDAH_NAME),--buildah-name "$(BUILDAH_NAME)",) $(if $(HOST_PORT),--host-port "$(HOST_PORT)",) $(if $(GUEST_PORT),--guest-port "$(GUEST_PORT)",) $(if $(DIRECT_STATE_ROOT),--direct-state-root "$(DIRECT_STATE_ROOT)",) $(if $(DIRECT_CONTAINER_ID),--direct-container-id "$(DIRECT_CONTAINER_ID)",) $(if $(CONMON_STATE_ROOT),--conmon-state-root "$(CONMON_STATE_ROOT)",) $(if $(CONMON),--conmon "$(CONMON)",) $(if $(CONMON_NAME),--conmon-name "$(CONMON_NAME)",) $(if $(PROBE_HOST),--probe-host "$(PROBE_HOST)",) $(if $(PROBE_PATH),--probe-path "$(PROBE_PATH)",)

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

# Verify that the system runtime remains separate from the private neovex runtime path
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

# Verify the Neovex machine diagnostics helper against deterministic fake host artifacts
verify-neovex-machine-diagnostics-helper:
	bash scripts/verify-neovex-machine-diagnostics-helper.sh

# Verify the Neovex machine recreate helper against deterministic fake host artifacts
verify-neovex-machine-recreate-helper:
	bash scripts/verify-neovex-machine-recreate-helper.sh

# Verify the isolated local-binary machine CLI proof helper against deterministic fake host artifacts
verify-neovex-machine-cli-proof-helper:
	bash scripts/verify-neovex-machine-cli-proof-helper.sh

# Verify the Neovex machine guest-proof helper against deterministic fake guest artifacts
verify-neovex-machine-guest-proof-helper:
	bash scripts/verify-neovex-machine-guest-proof-helper.sh

# Verify the machine guest-binary build helper against deterministic fake cargo/rustup/zig shims
verify-build-neovex-machine-guest-binary-helper:
	bash scripts/verify-build-neovex-machine-guest-binary-helper.sh

# Verify the Linux package builder helper against deterministic staged binaries and manifests
verify-build-linux-release-packages-helper:
	bash scripts/verify-build-linux-release-packages-helper.sh

# Verify the apt repository builder helper against deterministic stub packages and signed metadata
verify-build-apt-repository-helper:
	bash scripts/verify-build-apt-repository-helper.sh

# Verify the Fedora/COPR SRPM builder against deterministic release-asset stubs and Fedora userspace
verify-build-fedora-release-srpms-helper:
	bash scripts/verify-build-fedora-release-srpms-helper.sh

# Verify the Neovex machine service-proof helper against deterministic fake host artifacts
verify-neovex-machine-service-proof-helper:
	bash scripts/verify-neovex-machine-service-proof-helper.sh

# Verify the Neovex Homebrew/cask proof helper against deterministic fake brew and guest artifacts
verify-neovex-homebrew-cask-proof-helper:
	bash scripts/verify-neovex-homebrew-cask-proof-helper.sh

# Verify the install script helper against deterministic inputs
verify-install-helper:
	bash scripts/verify-install-helper.sh

# machine-os build/package/publish targets moved to agentstation/neovex-machine-os

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

# Prepare an upstream convex-demos overlay, then run codegen + Neovex against it
convex-demo: convex-demo-stop
	@test -n "$(CONVEX_DEMOS_DIR)" || (echo "Set CONVEX_DEMOS_DIR in .env first" && exit 1)
	@test -n "$(DEMO)" || (echo "Usage: make convex-demo DEMO=node|html|http" && exit 1)
	@overlay_dir="$$(node ./scripts/convex-demo-overlay.mjs "$(CONVEX_DEMOS_DIR)" "$(DEMO)")"; \
	echo "Prepared overlay at $$overlay_dir"; \
	npx convex codegen --app "$$overlay_dir"; \
	cargo run -p neovex-bin -- serve --port 8080 --app-dir "$$overlay_dir"

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
