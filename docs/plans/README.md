# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait
- `docs/plans/execution-ownership-hardening-plan.md`
  - control plan for event loop ownership, worker lifecycle, backpressure, and
    shutdown hardening — cross-referenced against Convex, TigerBeetle, and
    CockroachDB

## How To Use This Folder

- Start with the plan that owns your workstream.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
