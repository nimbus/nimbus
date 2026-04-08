# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/convex-demos-compatibility-plan.md`
  - execution plan for closing the remaining Convex demo and compatibility gaps
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait

## Deferred design and experiment plans

- `docs/plans/pluggable-storage-backend-plan.md`
  - canonical plan for migrating Neovex internal storage from redb to SQLite,
    benchmarking SQLite against redb before cutover, and then removing redb
- `docs/plans/external-sql-storage-backends-plan.md`
  - deferred follow-on plan for Postgres and MySQL internal storage backends
    after the SQLite migration is stable
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

Completed plans live in `docs/plans/archive/`. Do not resume them unless
explicitly asked to review historical work.

## How To Use This Folder

- Start with the plan that owns your workstream.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- For Convex demo and compatibility work, start with
  `convex-demos-compatibility-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- If no active cleanup, refactor, or verification hardening control plane is
  listed above, author or promote a new active plan before resuming generic
  work.
- For the deferred raw-V8 backend fallback (only if the fork approach is
  blocked), see `raw-v8-warm-backend-plan.md`.
- For future wasmtime WASM backend work, start with
  `wasmtime-backend-plan.md`.
- For future Postgres/MySQL internal storage work, start with
  `external-sql-storage-backends-plan.md`.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
