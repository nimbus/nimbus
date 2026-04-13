# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via the
    retained redb embedded provider
- `docs/plans/macos-machine-support-plan.md`
  - canonical execution plan for the Podman-aligned macOS developer-machine
    architecture: one Linux guest VM, standard guest containers, `neovex machine ...`,
    host-local control channel, and real-host verification

## Stable implementation baselines

- `docs/reference/microvm-service-baseline.md`
  - concise current baseline for the landed krun-backed microVM runtime,
    service activation, Compose-backed `neovex service ...` surface, and the
    Linux-versus-macOS platform model

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
    `neovex service ...` command wiring
- `docs/plans/archive/convex-demos-compatibility-plan.md`
  - completed Convex compatibility and demo baseline; records the landed
    browser/client ergonomics, repo-owned demo variants, served browser bundle,
    and external `convex-demos` overlay workflow

## How To Use This Folder

- Start with the plan that owns your workstream.
- For the landed krun-backed microVM and service-control architecture, start
  with `docs/reference/microvm-service-baseline.md` rather than opening the
  archived plans first.
- For the current macOS developer-machine workstream, open
  `docs/plans/macos-machine-support-plan.md` after the baseline.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- The Convex demo and compatibility plan is complete and archived at
  `archive/convex-demos-compatibility-plan.md`; use it for historical review
  of the landed compatibility baseline, then promote a new active plan before
  resuming further Convex compat work.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- For Compose-backed service lifecycle follow-on work, start with
  `docs/reference/microvm-service-baseline.md`, then promote or author a new
  active plan if the task is larger than a small focused change.
- The execution-runtime versus sandbox-orchestration cleanup plan is complete
  and archived at `archive/runtime-sandbox-architecture-plan.md`. Use it to
  understand the landed `neovex-runtime` versus `neovex-sandbox` split, then
  promote or author a new active plan before doing further cleanup work in
  that area.
- The SQLite storage migration plan is complete and archived at
  `archive/pluggable-storage-backend-plan.md`; do not resume it as live work
  unless you were explicitly asked for historical review.
- If no active cleanup, refactor, or verification hardening control plane is
  listed above, author or promote a new active plan before resuming generic
  work.
- For the deferred raw-V8 backend fallback (only if the fork approach is
  blocked), see `raw-v8-warm-backend-plan.md`.
- For future wasmtime WASM backend work, start with
  `wasmtime-backend-plan.md`.
- The Postgres-first provider implementation plan is complete and archived at
  `archive/postgres-storage-provider-plan.md`; use it only for historical
  review of the first non-local provider implementation.
- The MySQL provider implementation plan is complete and archived at
  `archive/mysql-storage-provider-plan.md`; use it only for historical review
  of the second opt-in external provider implementation.
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
