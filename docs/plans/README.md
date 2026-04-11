# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/dependency-baseline-cleanup-plan.md`
  - active control plane for restoring a clean dependency baseline after the
    storage-layer hardening closeout; owns the `libsql` TLS/dependency-shape
    cleanup needed for `make deny` / `make ci`
- `docs/plans/convex-demos-compatibility-plan.md`
  - execution plan for closing the remaining Convex demo and compatibility gaps
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via the
    retained redb embedded provider

## Deferred design and experiment plans

- `docs/plans/vmm-infrastructure-plan.md`
  - canonical plan for VMM infrastructure: fork crun (+10 lines for TSI port
    mapping), system dependencies (conmon, buildah, libkrun, libkrunfw,
    catatonit, passt), conmon process model; follows Podman's architecture
- `docs/plans/microvm-runtime-plan.md`
  - canonical plan for the microVM runtime: buildah integration (replaces
    custom OCI code), OCI bundle generation, lifecycle probes, engine
    integration (ctx.services.*), developer experience
- `docs/plans/distribution-plan.md`
  - canonical plan for distributing neovex across all channels: install
    script, apt repo (Debian/Ubuntu), COPR (Fedora), Homebrew (macOS),
    binary tarballs, container images, cloud VM images (AWS AMI, GCP)
- `docs/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary
- `docs/plans/raw-v8-warm-backend-plan.md`
  - **closed** — activation gate never met; warm module pool succeeded through
    fork changes, making the raw-V8 backend unnecessary; preserved as research
    context only
- `docs/plans/wasmtime-backend-plan.md`
  - canonical plan for adding a wasmtime-based WASM backend alongside the
    existing `deno_core` V8 backend; covers backend abstraction refactor, WIT
    interface definitions, cooperative fuel-based scheduling, module caching,
    and bundle format extension; activation gate met (Locker fork Phase 5
    completed 2026-04-06)
- `docs/plans/wasi-agent-capabilities-plan.md`
  - canonical plan for adding agent OS primitives (virtual filesystem, sandboxed
    process execution, HTTP client) via WASI Component Model interfaces; covers
    `neovex:agent` WIT package, `AgentOsProvider` trait, capability-based tenant
    admission, and agent-os sidecar integration; activates after the wasmtime
    backend plan W3 completes

## Archived completed plans

Completed plans usually live in `docs/plans/archive/`. The historical umbrella
external-provider design plan remains at its original path because completed
storage migration records already cite it directly. Do not resume completed
plans unless explicitly asked to review historical work.

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
- `docs/plans/external-sql-storage-backends-plan.md`
  - completed umbrella provider-topology design baseline; records the settled
    `TenantPersistence` / `PersistenceProvider` seam, the control-plane and
    runtime-config cleanup slices, and the follow-on design decisions for
    replica-connected SQLite and MySQL

## How To Use This Folder

- Start with the plan that owns your workstream.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- For Convex demo and compatibility work, start with
  `convex-demos-compatibility-plan.md`.
- For dependency-baseline cleanup after the storage hardening closeout, start
  with `dependency-baseline-cleanup-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
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
  `external-sql-storage-backends-plan.md` is complete historical design
  context. For future replica-connected SQLite, MySQL, or other
  provider-topology implementation work, promote or author a new active plan
  using it as the architectural baseline.
- The replica-connected SQLite provider implementation plan is complete and
  archived at `archive/sqlite-replica-provider-plan.md`; use it only for
  historical review of the first `libsql`-first replica provider slice.
- The storage hardening follow-up plan is complete and archived at
  `archive/storage-layer-hardening-plan.md`; use it only for historical review
  of the verified post-migration cleanup and refresh-hardening pass.
- Do not revive the archived SQLite migration plan to own future non-local
  provider implementation details, pooling, replication, or coordination
  concerns; any new work there should start from a newly active plan rather
  than from an archived or completed historical record.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
