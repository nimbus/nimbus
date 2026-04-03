# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

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

## How To Use This Folder

- Start with the plan that owns your workstream.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
