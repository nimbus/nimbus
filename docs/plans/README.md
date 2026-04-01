# Plans

This directory uses a single active-roadmap model.

## Canonical roadmap

- `docs/plans/performance-and-architecture-plan.md`
  - the only active execution roadmap
  - owns sequencing, dependencies, test expectations, and acceptance criteria

## Archived plans

- `docs/plans/archive/runtime-http-cancellation-and-storage-plan.md`
  - historical context for the original cancellation workstreams
- `docs/plans/archive/async-storage-and-service-rewrite-plan.md`
  - historical intermediate rewrite plan now absorbed into the master roadmap

## How To Use This Folder

- Start with `performance-and-architecture-plan.md` for any implementation work.
- For autonomous Codex runs, reread the `Codex Execution Protocol`,
  `Roadmap Status Ledger`, and `Execution Log` near the top of the master plan
  before coding.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item.
- Use archived plans only for historical rationale or earlier exploration.
- Use `docs/research/` for north-star architecture and background research, not
  execution sequencing.
