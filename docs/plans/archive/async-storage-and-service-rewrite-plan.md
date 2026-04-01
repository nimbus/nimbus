# Async Storage And Service Rewrite Plan

Verified against repo state on 2026-04-01.

## Status

As of 2026-04-01, this document is historical context only.

The remaining async storage and service rewrite work now lives in the canonical
master roadmap:

- `docs/plans/performance-and-architecture-plan.md`

Specifically, see:

- Phase 5A: async storage traits and read-path migration
- Phase 5B: explicit async transaction model and write-path migration
- Phase 5C: removal of blocking adaptation layers and async server/runtime
  integration

## Why This Doc Is No Longer Canonical

The previous split-plan arrangement created ambiguity around:

- whether this document or the broader performance plan owned the async rewrite
- whether backend replacement was in scope
- how post-commit success semantics should be described at the transport
  boundary

Those issues are now resolved in the master roadmap. In particular:

- the first async rewrite stays redb-backed
- engine/storage post-commit semantics are separated from client disconnect
  behavior
- the rewrite remains aligned with the single mutation path and storage atomicity
  invariants

## Historical Summary

This document previously carried the standalone continuation of Workstream 3
from `docs/plans/archive/runtime-http-cancellation-and-storage-plan.md`:

- introduce an async trait hierarchy for storage
- move reads onto real async execution first
- add an explicit async write-transaction boundary
- rework tenant lifecycle around async handles
- remove blocking engine and runtime host-call adapters

That content has been preserved and expanded in the master roadmap so agents can
execute against a single canonical plan.
