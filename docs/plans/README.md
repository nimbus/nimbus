# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/convex-demos-compatibility-plan.md`
  - execution plan for closing the remaining Convex demo and compatibility gaps
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait
- `docs/plans/test-surface-and-queue-ownership-cleanup-plan.md`
  - canonical execution plan for the next runtime worker-queue, shared HTTP
    fixture, and concept-owned integration test cleanup pass
- `docs/plans/v8-locker-fork-plan.md`
  - plan for forking rusty_v8 and deno_core into agentstation/* to merge V8
    Locker API (PR #1896) for multi-isolate pooling and cooperative scheduling

- `docs/plans/warm-pool-default-and-retained-pool-deprecation-plan.md`
  - canonical plan for making WarmModulePool + CooperativeLocker the only
    production runtime path; removes RetainedJsRuntimePool and ~900 lines of
    reset_main_realm code across neovex, deno_core, and rusty_v8 forks

## Deferred design and experiment plans

- `docs/plans/pluggable-storage-backend-plan.md`
  - canonical plan for abstracting the storage layer behind a backend-agnostic
    trait boundary, implementing SQLite as the primary embedded backend, and
    establishing the architecture for Postgres/MySQL backends and D1
    compatibility
- `docs/plans/warm-module-pool-plan.md`
  - **done** — all 6 phases complete; WarmModulePool delivers 22µs warm-hit
    latency (50x over snapshot cache); follow-on deprecation work owned by
    `warm-pool-default-and-retained-pool-deprecation-plan.md`
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

- `docs/plans/archive/modularity-and-idiomatic-rust-cleanup-plan.md`
  - completed control plane for the runtime and engine modularity cleanup
    workstream; historical record only
- `docs/plans/archive/deep-module-ownership-and-canonical-cleanup-plan.md`
  - completed control plane for the deeper serving, indexing, planner,
    direct-mutation, and concept-owned scenario cleanup pass; historical
    record only
- `docs/plans/archive/concept-owned-modularity-and-canonical-cleanup-plan.md`
  - completed control plane for the deeper concept-ownership, canonical
    naming, and idiomatic Rust cleanup pass; historical record only
- `docs/plans/archive/refactor-and-cleanup-control-plane.md`
  - completed control plane for the behavior-preserving engine, server, and
    runtime refactor and cleanup pass; historical record only
- `docs/plans/archive/operational-state-and-scenario-surface-cleanup-plan.md`
  - completed control plane for the operational-state, runtime metrics,
    websocket-session, and concept-owned scenario-surface cleanup pass;
    historical record only
- `docs/plans/archive/stateful-execution-and-harness-cleanup-plan.md`
  - completed control plane for deterministic-harness, execution-unit,
    serving-backend, runtime invocation, cooperative-worker, and scenario-root
    cleanup; historical record only
- `docs/plans/archive/execution-boundaries-and-integration-surface-cleanup-plan.md`
  - completed control plane for async-storage, scheduler persistence,
    runtime executor/driver, and integration-surface cleanup; historical
    record only
- `docs/plans/archive/indexing-bootstrap-and-scenario-surface-cleanup-plan.md`
  - completed control plane for storage indexing, runtime bootstrap, executor
    admission, and scenario-surface cleanup; historical record only
- `docs/plans/archive/`
  - home for completed historical plans that should not be resumed as active
    control planes unless explicitly requested

## How To Use This Folder

- Start with the plan that owns your workstream.
- Do not resume a plan from `docs/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- For Convex demo and compatibility work, start with
  `convex-demos-compatibility-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- For the current cleanup workstream, start with
  `test-surface-and-queue-ownership-cleanup-plan.md`.
- For the Locker fork and cooperative runtime workstream, start with
  `v8-locker-fork-plan.md`.
- For warm execution via module persistence on the `deno_core` fork, start
  with `warm-module-pool-plan.md`.
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
