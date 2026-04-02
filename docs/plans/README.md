# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Active execution plans

- `docs/plans/performance-and-architecture-plan.md`
  - canonical execution record for the completed architecture and performance
    cycle it covered
  - still owns any remaining work explicitly tracked inside that roadmap
- `docs/plans/verification-harness-plan.md`
  - canonical execution plan for the follow-on verification, simulation,
    differential-testing, and consistency-verifier workstream
- `docs/plans/scalability-and-architecture-follow-on-plan.md`
  - canonical execution plan for the remaining follow-on scalability, query,
    reactivity, and task-lifecycle architecture work from the April 2026 review
- `docs/plans/materialized-serving-hardening-plan.md`
  - canonical execution plan for hardening the first `SA8` materialized-serving
    slice into an explicit, bounded, sequence-stamped serving subsystem
- `docs/plans/encryption-at-rest-plan.md`
  - canonical execution plan for per-tenant encryption at rest via redb
    `StorageBackend` trait

## Archived plans

- `docs/plans/archive/runtime-http-cancellation-and-storage-plan.md`
  - historical context for the original cancellation workstreams
- `docs/plans/archive/async-storage-and-service-rewrite-plan.md`
  - historical intermediate rewrite plan now absorbed into the master roadmap

## How To Use This Folder

- Start with the plan that owns your workstream.
- For architecture-cycle work, start with
  `performance-and-architecture-plan.md`.
- For harness, simulation, adversarial-testing, and differential-verification
  work, start with `verification-harness-plan.md`.
- For remaining follow-on scalability, subscription-delivery, planner, scan,
  and task-lifecycle work, start with
  `scalability-and-architecture-follow-on-plan.md`.
- For hardening the tenant-local materialized-serving slice from `SA8`, start
  with `materialized-serving-hardening-plan.md`.
- For encryption at rest work, start with `encryption-at-rest-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use archived plans only for historical rationale or earlier exploration.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
