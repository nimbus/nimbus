# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for optional, enterprise-ready encryption at
    rest across Neovex-owned local persistence: embedded SQLite, retained
    redb, the retained redb control plane, and local libsql replica caches
- `docs/plans/websocket-protocol-plan.md`
  - canonical execution plan for versioned WebSocket protocol negotiation and
    unified error schema: subprotocol negotiation, hello/client_hello
    handshake, structured error types with code taxonomy, reference
    documents (`websocket-protocol.md`, `errors.md`); prerequisite for the
    desktop UI plan
- `docs/plans/localhost-server-security-plan.md`
  - canonical execution plan for localhost server security hardening:
    XDG-compliant token file lifecycle, origin allowlist middleware, session
    cookie bootstrap, CSP headers, server discovery file, audit log, `neovex
    token rotate` CLI subcommand; prerequisite for the desktop UI plan
- `docs/plans/system-tenant-api-plan.md`
  - canonical execution plan for the `_neovex` system tenant and management
    API: machine/service state persistence as documents, HTTP lifecycle
    endpoints, Convex function bundle with typed query surface, read/write
    path split; prerequisite for the desktop UI plan
- `docs/plans/desktop-ui-plan.md`
  - canonical execution plan for a Docker Desktop / Podman Desktop-style
    graphical interface: embedded React SPA at `/ui/*` via `rust-embed`,
    dashboard/machines/services/functions/data/logs/runs/settings tabs,
    dark mode, a11y, optional Electron shell (Phase 2); depends on the three
    prerequisite plans above

## Stable implementation baselines

- `docs/reference/microvm-service-baseline.md`
  - concise current baseline for the landed krun-backed microVM runtime,
    service activation, Compose-backed `neovex compose ...` surface, and the
    Linux-versus-macOS platform model
- `docs/reference/macos-machine-flow.md`
  - concise current reference for the settled macOS developer-machine contract:
    pinned Podman image digest, host-managed guest binary sync, forwarded
    machine API, host-resident `neovex start`, and proof-helper entrypoints

- `docs/plans/install-script-plan.md`
  - canonical execution plan for the neovex install script (Channel 1):
    `curl | sh` quick start for Linux (Debian/Ubuntu, Fedora/RHEL) and
    macOS (Apple Silicon). Covers platform detection, dependency
    installation, binary download, checksum verification, post-install
    verification helper, and the libkrun gap on Debian/Ubuntu.

## Pending plans

- `docs/plans/nimbus-rename-satellite-repos-plan.md`
  - prerequisite plan for renaming internals of satellite repositories
    (`nimbus-machine-os`, `nimbus-crun`) and creating a new `nimbus/homebrew-tap`
    before the main repo rename: guest image paths, systemd units, OCI media
    types, OCI annotations, build scripts, workflow inputs, Homebrew cask, and
    cross-repo interface coordination
- `docs/plans/nimbus-rename-plan.md`
  - canonical execution plan for renaming the project from "neovex" to "nimbus"
    and relocating all repositories from the `agentstation` GitHub organization
    to `nimbus`: GitHub repo transfers, Rust crate renames, JS package renames,
    CI/CD workflow updates, script renames, Makefile updates, config/doc bulk
    replacement, and verification; depends on the satellite repos plan above

## Deferred plans with defined scope

- `docs/plans/windows-machine-support-plan.md`
  - canonical execution plan for the Podman-aligned Windows developer-machine
    architecture, source-backed against the Podman WSL2 provider: Windows-native
    `neovex.exe` with WSL2 machine provider, win-sshproxy named-pipe API
    forwarding, shell-script bootstrap (not ignition), WSL2-native networking
    (not gvproxy); activation gate is macOS MAC5+ stabilization

## Deferred design and experiment plans

- `docs/plans/distribution-plan.md`
  - canonical plan for distributing neovex across all channels: install
    script, apt repo (Debian/Ubuntu), COPR (Fedora), Homebrew + machine VM
    (macOS via krunkit/libkrun), binary tarballs, container images, cloud
    VM images (AWS AMI, GCP). Channel 4 covers the macOS machine VM
    architecture (krunkit, guest image, control channel, virtiofs, gvproxy)
- `docs/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary
- `docs/plans/raw-v8-warm-backend-plan.md`
  - **closed** — activation gate never met; warm module pool succeeded through
    fork changes, making the raw-V8 backend unnecessary; preserved as research
    context only
- `docs/plans/wasmtime-backend-plan.md`
  - canonical plan for adding a wasmtime-based WASM backend alongside the
    existing V8 backend (currently implemented via `deno_core`); covers
    backend abstraction refactor, WIT interface definitions, cooperative
    fuel-based scheduling, module caching, and bundle format extension;
    activation gate met (Locker fork Phase 5 completed 2026-04-06)
- `docs/plans/wasi-agent-capabilities-plan.md`
  - canonical plan for adding agent OS primitives (virtual filesystem, sandboxed
    process execution, HTTP client) via WASI Component Model interfaces; covers
    `neovex:agent` WIT package, `AgentOsProvider` trait, capability-based tenant
    admission, and agent-os sidecar integration; activates after the wasmtime
    backend plan W3 completes

## Archived completed plans

Completed plans usually live in `docs/plans/archive/`. Do not resume
completed plans unless explicitly asked to review historical work.

- `docs/plans/archive/pluggable-storage-backend-plan.md`
  - completed SQLite storage migration control plan; records the cutover to
    SQLite as the default embedded provider, the retained redb provider, and
    the benchmark/provider-seam history that future work may need as context
- `docs/plans/archive/postgres-storage-provider-plan.md`
  - completed Postgres-first tenant persistence provider plan; records the
    first non-local provider implementation, benchmark gate, operational
    drills, and the decision to keep Postgres as an opt-in external mode
- `docs/plans/archive/codegen-cli-plan.md`
  - completed first-party CLI/codegen integration plan; records the `neovex
    codegen` command, the `--app-dir` serve contract, one-shot preflight
    codegen, and the manifest-loading UX closeout
- `docs/plans/archive/mysql-storage-provider-plan.md`
  - completed MySQL tenant persistence provider plan; records the
    `mysql_async`-based provider implementation, benchmark/RTT gate, reconnect
    drill fixes, and the decision to keep MySQL as an opt-in external mode
- `docs/plans/archive/sqlite-replica-provider-plan.md`
  - completed replica-connected SQLite provider plan; records the `libsql`
    remote-primary plus provider-owned replica-cache implementation, the
    freshness-drill benchmark gate, and the decision to keep the benchmark
    harness env/CLI-driven on explicit `sqld` endpoints
- `docs/plans/archive/storage-layer-hardening-plan.md`
  - completed storage hardening follow-up plan; records the `QueryReadStore`
    de-duplication, embedded SQLite pool guardrail, Postgres/MySQL targeted
    planner reads, structured storage error kinds, replica-refresh hardening,
    and the final closeout verification baseline
- `docs/plans/archive/dependency-baseline-cleanup-plan.md`
  - completed dependency-baseline cleanup plan; records the remote-only
    `libsql` dependency-shape fix, the narrow `RUSTSEC-2026-0097` evidence,
    the direct `tokio-tungstenite` lift to `0.28`, and the final green
    `make deny` / `make ci` baseline
- `docs/plans/archive/architecture-modularity-and-maintainability-plan.md`
  - completed architecture, modularity, and maintainability control plane;
    records the persistence-provider decomposition, service bootstrap and
    router cleanup, Convex facade bounding, JS wrapper thinning, the AWS KMS
    dependency-chain lift that cleared the `cargo deny` closeout blocker, and
    the final green archive verification bundle
- `docs/plans/archive/codebase-architecture-and-maintainability-plan.md`
  - completed codebase architecture and maintainability control plane;
    records persistence/provider capability normalization, engine
    sync/async/cancellable flow convergence, the typed server build pipeline,
    Convex host-call family ownership, service-command ownership cleanup,
    architecture-doc repackaging, and the final green closeout verification
    sweep
- `docs/plans/archive/targeted-domain-modularity-cleanup-plan.md`
  - completed targeted-domain cleanup pass; records the final runtime test
    extraction, tenant facade split, auth module split, browser client split,
    and the maintainability baseline the later repo-wide cleanup wave built on
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
  - completed repo-wide maintainability cleanup plan; records the CLI,
    provider, and krun ownership splits, the final verification bundle, and
    the archive-state control-plane handoff for future broad cleanup waves
- `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
  - completed follow-on maintainability cleanup plan; records the thin-root
    regression extraction, Compose and machine API splits, OCI/buildah
    packaging cleanup, krun smoke and mutation-journal proof repackaging, the
    provider benchmark harness modularization, explicit closeout justification
    for the near-threshold embedded and libsql benchmark roots, and the final
    green verification sweep
- `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
  - completed hotspot maintainability cleanup plan; records the machine,
    service, and machine-manager regression repackaging, the remaining
    provider benchmark hotspot modularization, the architecture-doc reference
    extraction, the explicit `ARCHITECTURE.md` near-threshold justification,
    and the final green archive-closeout verification sweep
- `docs/plans/archive/codebase-reliability-and-maintainability-hardening-plan.md`
  - completed reliability and maintainability hardening wave; records the
    canonical timing and wait helper posture, checker-style async proof
    hardening, Postgres, engine, and storage proof repackaging, container
    runtime test extraction, the stable reliability reference docs, and the
    final green verification and archive closeout sweep
- `docs/plans/archive/storage-provider-contracts-and-observability-plan.md`
  - completed storage follow-up plan; records the `LibsqlReplica` naming
    cleanup, replica freshness observability surface, Postgres/MySQL schema
    metadata caches, and the final green `make check` / `make test` /
    `make clippy` closeout baseline
- `docs/plans/archive/postgres-listener-reconnect-schema-recovery-plan.md`
  - completed Postgres reconnect correctness follow-up; records the
    authoritative schema-plus-journal catch-up on LISTEN reattach and the
    focused regression for missed schema notifications during listener downtime
- `docs/plans/archive/external-sql-storage-backends-plan.md`
  - completed umbrella provider-topology design baseline; records the settled
    `TenantPersistence` / `PersistenceProvider` seam, the control-plane and
    runtime-config cleanup slices, and the follow-on design decisions for
    replica-connected SQLite and MySQL
- `docs/plans/archive/runtime-sandbox-architecture-plan.md`
  - completed execution-runtime versus sandbox-orchestration cleanup baseline;
    records the settled `neovex-runtime` versus `neovex-sandbox` naming and
    seam decisions that deferred runtime and sandbox plans build on
- `docs/plans/archive/vmm-infrastructure-plan.md`
  - completed patched-crun and host-validation execution record for the
    krun-backed VMM foundation
- `docs/plans/archive/microvm-runtime-plan.md`
  - completed execution record for the krun-backed microVM runtime:
    buildah/image integration, lifecycle probes, engine integration, and
    developer-facing service workflows
- `docs/plans/archive/service-control-plane-plan.md`
  - completed execution record for the Compose-backed service control plane:
    project identity, control-root layout, backend-owned lifecycle state, and
    the then-current `neovex service ...` command wiring superseded by
    `neovex compose ...`
- `docs/plans/archive/convex-demos-compatibility-plan.md`
  - completed Convex compatibility and demo baseline; records the landed
    browser/client ergonomics, repo-owned demo variants, served browser bundle,
    and external `convex-demos` overlay workflow
- `docs/plans/archive/neovex-source-root-plan.md`
  - completed native `neovex/` source-root rollout; records resolver-owned
    dual-root selection, namespace-aware `_generated/*` emission, CLI feedback
    when both roots exist, and the final docs/test alignment while preserving
    `.neovex/convex/`
- `docs/plans/archive/macos-machine-support-plan.md`
  - completed macOS developer-machine closeout plan; records the MAC1-MAC7
    execution history, real-host proof bundles, and the final Podman-aligned
    macOS developer contract
- `docs/plans/archive/machine-lifecycle-hardening-plan.md`
  - completed shared machine-lifecycle hardening plan; records the landed
    `MLH1`-`MLH7` Podman-aligned robustness rollout for stop/start lifecycle,
    file-locked SSH port allocation, atomic record writes, schema versioning,
    provider capabilities, and phased machine startup
- `docs/plans/archive/machine-cli-dx-plan.md`
  - completed machine CLI developer-experience plan; records the `DX1`-`DX11`
    rollout for version/help polish, Podman-aligned machine flags and flows,
    list/inspect/set/cp, quiet scripting modes, and the final real macOS proof
    bundles
- `docs/plans/archive/machine-cli-alignment-plan.md`
  - completed machine/service CLI alignment control plane; records the
    `CLIA1`-`CLIA10` rollout for shared help/output/progress/table contracts,
    deterministic proof helpers, and the final local-binary plus
    packaged/Homebrew macOS proof bundles
- `docs/plans/archive/machine-cli-follow-on-plan.md`
  - completed machine/service CLI follow-on wave; records the `CLIF1`-`CLIF5`
    rollout for Podman-aligned `machine info`, output-shaping parity, stronger
    `machine list` ergonomics, help/reference cleanup, and the final real
    macOS host proof bundle
- `docs/plans/archive/cli-command-surface-plan.md`
  - completed CLI command-surface wave; records the `neovex compose`
    replacement for the retired `neovex service` surface, `neovex dev`,
    deploy/admin API plus `neovex deploy`, `neovex start` replacement for the
    retired `neovex serve` surface, final naming/DX review, and verification
    bundle
- `docs/plans/archive/compose-discovery-plan.md`
  - completed Docker/Podman-compatible compose discovery plan; records the
    shared cwd-plus-parent discovery contract, supported filename family,
    provenance-aware selection model, default override pairing, and alignment
    across `neovex compose ...`, `neovex dev`, and `neovex start`
- `docs/plans/archive/compose-explicit-multifile-plan.md`
  - completed explicit multi-file Compose follow-on; records ordered repeated
    compose-path flags, `COMPOSE_FILE` and `COMPOSE_PATH_SEPARATOR`, shared
    multi-file selection semantics, help/reference follow-up polish, and cwd
    test-lock hardening on top of the landed discovery baseline

## How To Use This Folder

- Start with the plan that owns your workstream.
- For broad maintainability, refactor, modularity, readability, canonical
  naming, idiomatic-Rust, or god-file cleanup work, start with
  `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md`, use
  `docs/plans/archive/codebase-architecture-and-maintainability-plan.md` for
  the latest completed governance baseline when historical execution detail is
  needed, and promote a new active plan before landing another repo-wide
  cleanup wave unless another active plan already owns the slice.
- For the landed krun-backed microVM and service-control architecture, start
  with `docs/reference/microvm-service-baseline.md` rather than opening the
  archived plans first.
- For current macOS developer-machine behavior, start with
  `docs/reference/microvm-service-baseline.md` and
  `docs/reference/macos-machine-flow.md`.
- For historical machine/service CLI follow-on work, start with
  `docs/plans/archive/machine-cli-follow-on-plan.md` only when you need the
  completed `CLIF1`-`CLIF5` rollout, exact proof-bundle paths, or the settled
  follow-on command-surface contract.
- Use `docs/plans/archive/cli-command-surface-plan.md` only when you need the
  completed `compose` / `dev` / `deploy` / `start` rollout, exact
  verification bundle, or retired `service` / `serve` decision record.
- Use `docs/plans/archive/machine-cli-alignment-plan.md` only for the
  completed `CLIA1`-`CLIA10` rollout, older historical proof-bundle paths, and
  the baseline contract the follow-on wave refined.
- Use `docs/plans/archive/machine-cli-dx-plan.md` only for the completed first
  DX wave, older comparative audit context, or archived proof bundles.
- Promote a new active plan before landing another machine/service CLI UX
  wave.
- Open `docs/plans/archive/macos-machine-support-plan.md` only when you need
  the historical MAC1-MAC7 execution record or exact proof-bundle paths.
- For historical shared machine-lifecycle hardening context or Windows
  provider prerequisites, open
  `docs/plans/archive/machine-lifecycle-hardening-plan.md` after the baseline
  and the relevant platform plan.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- The Convex demo and compatibility plan is complete and archived at
  `archive/convex-demos-compatibility-plan.md`; use it for historical review
  of the landed compatibility baseline, then promote a new active plan before
  resuming further Convex compat work.
- For Convex or Neovex CLI/codegen workflow work (`packages/codegen/`,
  `packages/convex/`, `demos/convex/`, or the `neovex start --app-dir`
  contract), start with `docs/reference/convex-ai-guidelines.md`,
  `docs/reference/cli.md`, and `docs/convex/compatibility.md`. Use
  `archive/codegen-cli-plan.md` only for the completed CLI/codegen rollout's
  execution record or exact verification bundle, and use the archived
  `archive/neovex-source-root-plan.md` only for historical source-root
  context. Promote a new active plan before landing another CLI/codegen
  workflow wave.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- For Compose-backed service lifecycle follow-on work, start with
  `docs/reference/microvm-service-baseline.md`, then promote or author a new
  active plan if the task is larger than a small focused change.
- For repo-wide reliability-proof posture or CI flake investigation, start
  with `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md`. Open
  `archive/codebase-reliability-and-maintainability-hardening-plan.md` only
  when you need the completed hardening wave's execution record, closeout
  verification baseline, or proof-packaging decisions.
- The execution-runtime versus sandbox-orchestration cleanup plan is complete
  and archived at `archive/runtime-sandbox-architecture-plan.md`. Use it to
  understand the landed `neovex-runtime` versus `neovex-sandbox` split, then
  promote or author a new active plan before doing further cleanup work in
  that area.
- For broad maintainability, readability, modularity, reliability hardening,
  canonical naming, or god-file cleanup work, start with
  `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md`. Use
  `archive/architecture-modularity-and-maintainability-plan.md` for the most
  recent completed repo-wide maintainability wave's execution record,
  closeout verification bundle, and governance baseline for thin roots,
  concept-owned naming, helper-bucket avoidance, threshold exceptions, and
  wrapper-first JS compatibility guidance. Use
  `archive/codebase-reliability-and-maintainability-hardening-plan.md` only
  for the completed hardening wave's execution record, closeout verification
  baseline, and proof-packaging decisions. Use
  `archive/codebase-modularity-and-maintainability-hotspots-plan.md` only for
  the latest completed hotspot wave's execution record, closeout
  justifications, and architecture-doc packaging baseline. Use
  `archive/codebase-modularity-and-maintainability-follow-on-plan.md` only for
  the completed follow-on wave's execution record, closeout justifications,
  and benchmark-packaging baseline. Use
  `archive/codebase-modularity-and-maintainability-plan.md` only for
  historical context on the predecessor CLI, provider, and sandbox ownership
  map. Promote a new active plan before landing another repo-wide
  maintainability or reliability-hardening wave unless some other active plan
  already owns the slice.
- The SQLite storage migration plan is complete and archived at
  `archive/pluggable-storage-backend-plan.md`; do not resume it as live work
  unless you were explicitly asked for historical review.
- For future cleanup or verification-hardening work that is not already owned
  by another active plan, author or promote a new active plan instead of
  reviving an archived one.
- For the deferred raw-V8 backend fallback (only if the fork approach is
  blocked), see `raw-v8-warm-backend-plan.md`.
- For future wasmtime WASM backend work, start with
  `wasmtime-backend-plan.md`.
- The Postgres-first provider implementation plan is complete and archived at
  `archive/postgres-storage-provider-plan.md`; use it only for historical
  review of the first non-local provider implementation.
- The MySQL provider implementation plan is complete and archived at
  `archive/mysql-storage-provider-plan.md`; use it only for historical
  review of the second opt-in external provider implementation.
- The umbrella external-provider plan at
  `archive/external-sql-storage-backends-plan.md` is complete historical
  design context. For future replica-connected SQLite, MySQL, or other
  provider-topology implementation work, promote or author a new active plan
  using it as the architectural baseline.
- The replica-connected SQLite provider implementation plan is complete and
  archived at `archive/sqlite-replica-provider-plan.md`; use it only for
  historical review of the first `libsql`-first replica provider slice.
- The storage hardening follow-up plan is complete and archived at
  `archive/storage-layer-hardening-plan.md`; use it only for historical review
  of the verified post-migration cleanup and refresh-hardening pass.
- The dependency-baseline cleanup plan is complete and archived at
  `archive/dependency-baseline-cleanup-plan.md`; use it only for historical
  review of the `libsql` dependency-shape cleanup and deny/CI closeout.
- The storage-provider contracts and observability follow-up plan is complete
  and archived at
  `archive/storage-provider-contracts-and-observability-plan.md`; use it only
  for historical review of the verified storage naming, observability, and
  schema-cache cleanup pass. Promote a new active plan before resuming further
  storage-provider follow-up work.
- Do not revive the archived SQLite migration plan to own future non-local
  provider implementation details, pooling, replication, or coordination
  concerns; any new work there should start from a newly active plan rather
  than from an archived or completed historical record.
- The Postgres listener reconnect schema-recovery follow-up is complete and
  archived at `archive/postgres-listener-reconnect-schema-recovery-plan.md`;
  use it only for historical review of the missed-schema recovery fix.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
