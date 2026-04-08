# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/convex-demos-compatibility-plan.md`
  - execution plan for closing the remaining Convex demo and compatibility gaps
- `docs/plans/deterministic-test-and-harness-hardening-plan.md`
  - canonical execution plan for TigerBeetle-style test and harness hardening:
    explicit profiles, real runtime isolation, deterministic waiters, and CI
    alignment
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait
- `docs/plans/v8-locker-fork-plan.md`
  - plan for forking rusty_v8 and deno_core into agentstation/* to merge V8
    Locker API (PR #1896) for multi-isolate pooling and cooperative scheduling

## Deferred design and experiment plans

- `docs/plans/pluggable-storage-backend-plan.md`
  - canonical plan for abstracting the storage layer behind a backend-agnostic
    trait boundary, implementing SQLite as the primary embedded backend, and
    establishing the architecture for Postgres/MySQL backends and D1
    compatibility
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
    and bundle format extension; activates after the Locker fork plan Phase 5
    completes
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
- For the current test and harness hardening workstream, start with
  `deterministic-test-and-harness-hardening-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- For the Locker fork and cooperative runtime workstream, start with
  `v8-locker-fork-plan.md`.
- If no active cleanup, refactor, or verification hardening control plane is
  listed above, author or promote a new active plan before resuming generic
  work.
- For the deferred raw-V8 backend fallback (only if the fork approach is
  blocked), see `raw-v8-warm-backend-plan.md`.
- For future wasmtime WASM backend work, start with
  `wasmtime-backend-plan.md`.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
