# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/convex-demos-compatibility-plan.md`
  - execution plan for closing the remaining Convex demo and compatibility gaps
- `docs/plans/deep-module-ownership-and-canonical-cleanup-plan.md`
  - canonical execution plan for the next deeper serving, indexing,
    planner, direct-mutation, and concept-owned scenario cleanup pass
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait
- `docs/plans/v8-locker-fork-plan.md`
  - plan for forking rusty_v8 and deno_core into agentstation/* to merge V8
    Locker API (PR #1896) for multi-isolate pooling and cooperative scheduling

## Deferred design and experiment plans

- `docs/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary

## Archived completed plans

- `docs/plans/archive/modularity-and-idiomatic-rust-cleanup-plan.md`
  - completed control plane for the runtime and engine modularity cleanup
    workstream; historical record only
- `docs/plans/archive/concept-owned-modularity-and-canonical-cleanup-plan.md`
  - completed control plane for the deeper concept-ownership, canonical
    naming, and idiomatic Rust cleanup pass; historical record only
- `docs/plans/archive/refactor-and-cleanup-control-plane.md`
  - completed control plane for the behavior-preserving engine, server, and
    runtime refactor and cleanup pass; historical record only
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
- For the current deeper serving, indexing, planner, and canonical cleanup
  workstream, start with
  `deep-module-ownership-and-canonical-cleanup-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- For the Locker fork and cooperative runtime workstream, start with
  `v8-locker-fork-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
