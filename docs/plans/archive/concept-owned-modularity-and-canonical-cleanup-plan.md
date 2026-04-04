# Concept-Owned Modularity And Canonical Cleanup Control Plan

Archived on 2026-04-03 after `CO0` through `CO7` completed. Historical record
only; do not resume this plan as a live control plane.

This is the canonical execution control plane for the current deeper
modularity, concept-ownership, canonical naming, and idiomatic Rust cleanup
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-runtime/src/runtime/bootstrap.rs`
- `crates/neovex-storage/src/store.rs`
- `crates/neovex-engine/src/service/scheduler.rs`
- `crates/neovex-engine/src/tenant/subscription_delivery.rs`
- `crates/neovex-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct.rs`
- `crates/neovex-engine/src/tests.rs`
- `crates/neovex-storage/src/tests.rs`

Baseline verification status for this plan:

- the immediately preceding modularity cleanup workstream closed green on
  2026-04-03 with:
  `cargo test -p neovex-runtime`,
  `cargo test -p neovex-engine`,
  `cargo test -p neovex-server`,
  `cargo check --workspace`,
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `make check`,
  `make test`,
  and `make clippy`
- this control plane was authored as a docs-only review-and-planning pass on
  2026-04-03 after that verified baseline
- no `CO*` implementation work has landed yet; `CO1` must record its own
  focused verification before it can be marked `done`

---

## Purpose

Neovex's architecture is in a better place than it was before the last cleanup
pass. The runtime, engine read path, subscription ownership, and tenant facade
now have clearer composition roots, and `ARCHITECTURE.md` reflects that landed
shape.

The next cleanup pass should build on that baseline by focusing on the
remaining dense multi-concept production hotspots and the still-clumped test
surfaces around them. The goal is not to split files just to lower line counts.
The goal is to make ownership obvious, naming canonical, tests easier to
localize, and future feature work easier to place beside the concept it
belongs to.

This is not a feature roadmap. It is a code-organization, maintainability, and
correctness roadmap.

---

## Relationship To Other Plans

Use `docs/plans/README.md` as the plan index. If a change turns into
encryption-at-rest work, Locker fork work, admission-control work, Convex demo
compatibility work, or another separately owned stream, stop and move to the
owning plan instead of stretching this one.

---

## Scope

This plan covers:

- deeper concept-owned decomposition of the remaining runtime, storage,
  engine, and server hotspots
- cleanup of canonical public and internal naming where the current ownership
  map is still muddy
- removal of repeated sync, async, and cancellable wrapper patterns where the
  concept boundaries are already stable
- movement of still-clumped tests toward more concept-owned surfaces after the
  production module boundaries settle
- architecture and plan updates needed to keep the cleanup resumable through
  compaction, interruption, and handoff

This plan does not cover:

- new product features
- intentional route, wire, or externally observable behavior changes unless a
  specific item explicitly records them
- storage-format changes
- planner capability expansion beyond preserving already-landed behavior
- admission-control redesign
- Locker fork or cooperative-runtime implementation work
- speculative performance rewrites that are not justified by ownership,
  correctness, or maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Native routes, runtime host-call semantics, durable journal behavior,
   scheduler behavior, auth behavior, and subscription behavior must stay
   unchanged unless a specific item explicitly says otherwise and the change is
   recorded.

2. Keep core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split is one where the owning concept is easier to identify,
   not one where code simply moved into more files.

4. Keep composition roots thin once ownership moves out.
   Do not turn new root modules into renamed god files.

5. Keep shutdown, cancellation, fairness, queueing, and durability semantics
   explicit and testable.

6. Add focused regression coverage before moving high-risk runtime, storage,
   scheduler, or subscription seams.

7. Prefer canonical naming, visibility, and state ownership over clever
   abstraction.
   Use the simplest idiomatic Rust shape that makes the owning concept clear.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

This plan assumes the codebase described in `ARCHITECTURE.md` today:

- `service/queries.rs` is now a thin composition root over concept-owned read
  surface modules.
- `subscriptions.rs` is now a composition root over registry, dependency,
  queue, delivery, and invalidation ownership.
- `tenant.rs` is the `TenantRuntime` facade and composition root, not the main
  cleanup hotspot it used to be.
- `runtime.rs` and `executor.rs` are now composition roots with extracted
  module trees; their raw line counts are inflated by inline tests and are no
  longer the first production hotspots to attack.

The current hotspot map from the live worktree is:

- `crates/neovex-runtime/src/runtime.rs` is 2189 lines, but its inline test
  module begins at line 445
- `crates/neovex-runtime/src/executor.rs` is 1503 lines, but its inline test
  module begins at line 437
- `crates/neovex-runtime/src/runtime/bootstrap.rs` is 1222 lines and is now
  the largest remaining runtime production hotspot
- `crates/neovex-storage/src/store.rs` is 1719 lines and remains the clearest
  true storage god file
- `crates/neovex-engine/src/service/scheduler.rs` is 524 lines and still mixes
  scheduled-job admin, cron admin, async wrappers, and coordination logic
- `crates/neovex-engine/src/tenant/subscription_delivery.rs` is 390 lines and
  still mixes queue state, worker lifecycle, metrics, and test pause seams
- `crates/neovex-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct.rs`
  is 491 lines and still repeats sync, async, and cancellable wrapper shapes
- `crates/neovex-engine/src/tests.rs` is 10881 lines and
  `crates/neovex-storage/src/tests.rs` is 3014 lines; both still bundle many
  concepts into single test surfaces

Large files that are currently more concept-cohesive and are not first-wave
targets for this plan:

- `crates/neovex-engine/src/tenant/materialized_reads.rs`
- `crates/neovex-storage/src/index.rs`

They may still deserve future cleanup, but they are not the best first slices
for this workstream unless a later item reveals a clearer ownership break.

---

## Current Review Findings

These findings describe the current reasons this plan exists.

1. `crates/neovex-storage/src/store.rs` is still a true god file.
   It mixes the redb transaction model, write-path helpers, durable journal
   replay and recovery, read snapshot behavior, scan pushdown, low-level
   MessagePack probing, and schema or index rewrite helpers in one module.

2. `crates/neovex-runtime/src/runtime/bootstrap.rs` is now the deepest runtime
   production hotspot.
   It mixes host-call payload schemas, op registration, async and sync host
   call glue, bootstrap JavaScript, startup snapshot creation, timeout
   control, runtime host state, and isolate-pool concerns in one file.

3. `crates/neovex-engine/src/service/scheduler.rs` still mixes distinct
   scheduler concepts.
   Scheduled-job CRUD, cron CRUD, async and cancellable wrappers, loaded
   tenant scanning, and next-due coordination are all expressed together.

4. `crates/neovex-engine/src/tenant/subscription_delivery.rs` still combines
   queue ownership, dedicated-worker lifecycle, stats accounting, and testing
   pause controls.

5. `crates/neovex-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct.rs`
   still duplicates sync, async, and cancellable host-bridge wrapper flows for
   query, mutation, pagination, and action paths.

6. The remaining largest test surfaces are still concept-mixed.
   That makes it harder to keep characterization coverage near the owning
   module and harder to debug or extend one subsystem in isolation.

7. Raw line counts alone are now misleading in some places.
   `runtime.rs` and `executor.rs` still look large at a glance, but the next
   cleanup pass should focus on true remaining concept mixes rather than
   re-breaking already-thin composition roots.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The current feature set and stable behavior still work after the cleanup.
2. `runtime/bootstrap.rs` is split around clear runtime bootstrap concepts
   instead of remaining a runtime catch-all.
3. `store.rs` no longer owns unrelated storage concerns in one file and the
   durable write, read-snapshot, and journal ownership map is clearer.
4. Scheduler, subscription-delivery, and Convex host-bridge direct surfaces
   are grouped by concept and use more canonical internal patterns.
5. The highest-value clumped tests are moved closer to the concepts they
   protect without losing integration coverage.
6. Naming, visibility, helper placement, and module boundaries are more
   idiomatic and easier to extend.
7. The landed ownership map is reflected in docs and in a resumable execution
   log so a future agent can continue from this plan and the worktree alone.
8. Final verification proves the cleanup did not introduce regressions.

---

## Feature Preservation Matrix

Every implementation item must preserve these surfaces.

| Surface | Must stay true | Minimum item-level verification |
| --- | --- | --- |
| Native CRUD, query, paginated query, schema, scheduler, and journal routes | route semantics and durable behavior stay unchanged | targeted engine tests; `cargo test -p neovex-server` for touched HTTP paths |
| Durable journal and storage atomicity | document write, index update, and commit log append remain one transaction; replay and recovery behavior stay unchanged | targeted storage tests; engine tests when apply visibility or recovery paths are touched |
| Native WebSocket subscriptions | initial bootstrap, live delivery, cleanup on disconnect, and unsubscribe behavior stay unchanged | targeted engine and server reactive tests |
| Convex runtime query, mutation, action, scheduler, and nested-call paths | host-call semantics, error mapping, and ctx-op behavior stay unchanged | targeted runtime and server Convex tests |
| Runtime admission, cancellation, timeout, fairness, and shutdown semantics | queued, active, cancelled, timed-out, and shutdown behavior stay unchanged | targeted runtime tests plus `cargo test -p neovex-runtime` |
| Materialized read surface, diagnostics, and metrics snapshots | serving behavior and snapshot or metrics shapes stay unchanged unless explicitly documented | targeted engine tests; diagnostics or metrics tests when touched |

Exact targeted commands used for each item must be recorded in `Execution Log`.

---

## Control Plane Rules

This document is the durable control plane for the cleanup workstream. The
source of truth is:

1. the current git worktree
2. this plan's `Roadmap Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md`
4. the immediately relevant code and tests for the active item

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies are done
- `in_progress`: actively being implemented; keep exactly one item in this
  state per autonomous run
- `blocked`: cannot continue until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded
- `deferred`: intentionally parked outside the current cleanup pass

### Recovery loop for every new session or post-compaction resume

1. Reread `Cleanup Invariants`, `Current Assessed State`,
   `Current Review Findings`, `Feature Preservation Matrix`,
   `Roadmap Status Ledger`, `Implementation Checkpoints`,
   `Dependency Graph`, `Recommended Delivery Order`, and `Execution Log`.
2. Inspect the current git worktree before choosing new scope.
3. If any item is already `in_progress`, resume it first.
4. If the worktree is dirty, reconcile those changes to an owning item before
   starting anything new.
5. If no item is `in_progress`, pick the first eligible item in
   `Recommended Delivery Order` whose hard dependencies are `done`.
6. Add or confirm characterization coverage before large structural movement.
7. Update this plan's ledger, checkpoint, and log in the same change set as the
   code or docs.
8. If interrupted, compacted, or handing off, checkpoint before stopping.

### Dirty-worktree reconciliation rules

- A dirty worktree outranks remembered intent.
- If changed files clearly belong to one item, keep that item `in_progress`
  until it is either `done` or explicitly `blocked`.
- If changed files unexpectedly span multiple items, stop, record the conflict
  in `Execution Log`, and narrow the next slice before continuing.
- Do not treat partial refactor edits as disposable scratch state.

### Non-deviation rules

- Do not skip an existing `in_progress` item to start a later `todo` item.
- Do not mix behavior change and structural cleanup in the same item unless the
  item explicitly allows it.
- Do not mark an item `done` without recording verification.
- Do not close a work burst without writing the next concrete step into
  `Implementation Checkpoints` when anything remains partially complete.

### Required write-back after each work session

- update the item's status in `Roadmap Status Ledger`
- update or add the item's note in `Implementation Checkpoints` if the item
  remains partial
- append a row to `Execution Log` with date, item, outcome, verification, and
  next step
- update `ARCHITECTURE.md` in the same change set when the session lands an
  architecture-level ownership change promised by this plan

### Suggested autonomous prompt

```text
Use docs/plans/concept-owned-modularity-and-canonical-cleanup-plan.md as the
control plane. Reread Cleanup Invariants, Current Assessed State,
Current Review Findings, Feature Preservation Matrix, Control Plane Rules,
Verification Contract, Roadmap Status Ledger, Implementation Checkpoints,
Dependency Graph, Recommended Delivery Order, and Execution Log, then inspect
the current git worktree. If any item is in_progress, resume it first.
Reconcile dirty worktree changes to the owning item before starting new scope.
Implement exactly one item, run the required verification, update the ledger,
checkpoint, and log, and continue. If blocked, record the blocker in the plan
before stopping. Do not rely on chat history as progress state.
```

---

## Verification Contract

### Minimum verification per implementation item

- targeted tests for the touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`

### Additional verification by scope

- for runtime bootstrap or runtime host-ABI cleanup:
  `cargo test -p neovex-runtime`
- for storage store, durable journal, or read-snapshot cleanup:
  `cargo test -p neovex-storage`
- for engine scheduler or subscription-delivery cleanup:
  `cargo test -p neovex-engine`
- for server Convex host-bridge cleanup:
  `cargo test -p neovex-server`
- before marking any item `done`:
  `cargo clippy --workspace --all-targets -- -D warnings`

Run additional crate tests when a touched item spans multiple layers.

### Final verification before closing the plan

- `make check`
- `make test`
- `make clippy`

If `make ci` is practical at the end of the workstream, record that too.

If environmental limits block a command, record the limitation in
`Execution Log` and continue with the best focused verification available.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| CO0 | done | Baseline review and hotspot map for the current deeper modularity and canonical cleanup pass | none |
| CO1 | done | Split `crates/neovex-runtime/src/runtime/bootstrap.rs` into concept-owned runtime bootstrap modules | CO0 |
| CO2 | done | Decompose `crates/neovex-storage/src/store.rs` around durable write, journal, recovery, and read-snapshot ownership | CO0 |
| CO3 | done | Split `crates/neovex-engine/src/service/scheduler.rs` by grouped scheduler concepts and normalize wrapper patterns | CO0, CO2 |
| CO4 | done | Decompose `crates/neovex-engine/src/tenant/subscription_delivery.rs` around queue state, worker lifecycle, stats, and testing seams | CO0 |
| CO5 | done | Normalize the Convex host-bridge direct ctx-op surface to remove repeated sync, async, and cancellable wrapper logic | CO0, CO1, CO3 |
| CO6 | done | Perform the concept-owned test-surface and idiomatic Rust cleanup sweep after the main production ownership boundaries stabilize | CO1, CO2, CO3, CO4, CO5 |
| CO7 | done | Update docs and run the full verification sweep | CO1, CO2, CO3, CO4, CO5, CO6 |

---

## Dependency Graph

- `CO0` is the current architecture review and hotspot baseline for the
  workstream.
- `CO1` and `CO2` are the first major structural items because they target the
  biggest remaining production concept mixes in runtime and storage.
- `CO3` depends on `CO2` because scheduler cleanup should follow the clearer
  storage ownership map instead of freezing current `store.rs` sprawl into
  more wrappers.
- `CO4` can proceed once the baseline is established; it does not need to wait
  on the storage or runtime splits.
- `CO5` depends on `CO1` and `CO3` because the direct host-bridge surface
  should reflect the stabilized runtime bootstrap and scheduler ownership map
  rather than a moving intermediate shape.
- `CO6` waits until the production ownership changes land so test movement,
  visibility cleanup, and naming cleanup happen against the final structure.
- `CO7` is the closure pass that updates docs, removes leftover glue, and runs
  the full verification sweep.

---

## Recommended Delivery Order

1. `CO1`
2. `CO2`
3. `CO3`
4. `CO4`
5. `CO5`
6. `CO6`
7. `CO7`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| CO0 | done; reviewed the live post-modularity architecture, identified the remaining production concept mixes, promoted this plan as the next control plane, and prepared the completed modularity plan for archival so future agents do not resume it as active work | start `CO1` by mapping `runtime/bootstrap.rs` into concept-owned seams for host ABI payloads, op handlers, bootstrap source and snapshot creation, timeout and host state, and isolate-pool ownership |
| CO1 | done; replaced the single `runtime/bootstrap.rs` hotspot with a `runtime/bootstrap/` module tree: `payloads.rs` owns host-call schemas and envelopes, `ops.rs` owns op registration and host-call glue, `source.rs` owns bootstrap script install/finalize helpers, `state.rs` owns `OpState` installation and timeout-controller state, and `snapshot.rs` owns startup snapshot plus isolate-pool behavior while `mod.rs` stays thin | start `CO2` by mapping `store.rs` into concept-owned seams for write transactions, journal and recovery, read snapshots, scan behavior, and low-level probing helpers before editing storage code |
| CO2 | done; `store.rs` is now a storage composition root over `store/write.rs`, `journal.rs`, `read.rs`, `scan.rs`, and `schema_rewrite.rs`, while the existing journal snapshot/stream helpers keep their own files; direct write transaction ownership, durable journal/recovery, read snapshots, scan pushdown/probing, and schema-aware index rewrite no longer live in one file | start `CO3` by splitting `service/scheduler.rs` into concept-owned scheduler modules for scheduled-job CRUD, cron CRUD, async/cancellable wrapper normalization, and tenant-loading coordination |
| CO3 | done; `service/scheduler.rs` is now a thin scheduler composition root over `service/scheduler/access.rs`, `scheduled_jobs.rs`, `cron.rs`, and `coordination.rs`, so scheduled-job CRUD and result persistence, cron CRUD, shared sync/async/cancellable tenant-store access, and loaded-tenant startup recovery or next-due coordination no longer live in one file | start `CO4` by splitting `tenant/subscription_delivery.rs` into concept-owned queue-state, worker-lifecycle, stats-accounting, and test-pause modules without changing shutdown or delivery ordering |
| CO4 | done; `tenant/subscription_delivery.rs` is now a tenant-local delivery composition root over `tenant/subscription_delivery/queue.rs`, `worker.rs`, `stats.rs`, and `pause.rs`, so queue state, dedicated worker lifecycle, stats snapshots, and test-only pause control no longer live in one file while shutdown, queue overflow fallback, and delivery ordering semantics stay intact | start `CO5` by normalizing `server/.../ctx_ops/direct.rs` so the direct Convex ctx-op surface has one canonical home for sync, async, and cancellable wrapper behavior without changing auth, result encoding, or execution-unit short-circuiting |
| CO5 | done; `server/.../ctx_ops/direct.rs` is now a thin composition root over `direct/execution.rs` and `direct/invocation.rs`, so execution-context dispatch and execution-unit short-circuit behavior are separated from runtime payload decode/validate/encode and default-cancellation wrappers while auth, result encoding, and host-call semantics stay unchanged | start `CO6` by moving the highest-value clumped tests toward concept-owned surfaces and cleaning up leftover naming, visibility, and helper placement across the stabilized module trees |
| CO6 | done; moved the highest-value scheduler and subscription-delivery regressions out of `crates/neovex-engine/src/tests.rs` into `service/scheduler/tests.rs` and `tenant/subscription_delivery/tests.rs`, removed the duplicate root-level copies and dead helper glue, and corrected the moved scheduler schema helper to preserve the old no-policy baseline | start `CO7` by deciding the completed-plan closure shape, then update plan entrypoints and archive or retire this control plane after the repo-wide verification sweep is recorded |
| CO7 | done; recorded the green repo-wide `make check`, `make test`, and `make clippy` sweep, then archived this completed control plane and updated the plan index plus `AGENTS.md` so future agents do not resume finished cleanup work as active state | none; if follow-on modularity or cleanup work is promoted later, start from `docs/plans/README.md` and create or promote a new active plan instead of resuming this archived one |

---

## Work Items

### CO0. Baseline review and hotspot map

Completed in this planning pass.

Acceptance criteria:

- the next cleanup plan is grounded in the live post-modularity architecture
- the roadmap targets real remaining concept mixes instead of stale hotspots
- the plan is self-sufficient enough to resume after compaction or handoff

### CO1. Split `runtime/bootstrap.rs` by concept ownership

#### Implementation plan

1. Keep `runtime/bootstrap.rs` as a thin composition root or replace it with a
   `runtime/bootstrap/` module tree.
2. Extract grouped concepts into dedicated modules, likely including:
   - runtime host-call payload schemas and value envelopes
   - op registration and op-handler ownership
   - runtime timeout and host state plumbing
   - startup snapshot creation and isolate-pool ownership
   - bootstrap source installation helpers
3. Preserve the current `HostCallOperation` contract and existing bootstrap
   ordering.
4. Move tests toward the modules that own the behavior when that improves
   clarity.

#### Focused verification

- targeted runtime tests for bundle loading, bootstrap snapshot creation, and
  host-call registration
- `cargo test -p neovex-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `runtime/bootstrap.rs` no longer owns unrelated runtime bootstrap concerns in
  one file
- bootstrap source, host ABI payloads, and runtime state wiring have clear
  ownership
- runtime behavior and error mapping stay unchanged

### CO2. Decompose `store.rs` around storage concepts

#### Implementation plan

1. Keep `store.rs` as a storage composition root or replace it with a
   `store/` module tree.
2. Extract grouped concepts into dedicated modules, likely including:
   - `TenantWriteTransaction` and direct durable write helpers
   - durable journal append, replay, apply, and recovery ownership
   - read-snapshot and scan behavior
   - scan pushdown and low-level MessagePack probing helpers
   - schema or index rewrite helpers that currently live inline
3. Preserve the one-transaction durability contract for document writes, index
   updates, and commit log append.
4. Keep public storage exports intentional and minimal.

#### Focused verification

- targeted storage tests for write transactions, journal recovery, and read
  snapshot behavior
- `cargo test -p neovex-storage`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `store.rs` is no longer the owner of unrelated storage concepts
- write-path, read-snapshot, and journal ownership are explicit
- storage atomicity and durable recovery behavior stay unchanged

### CO3. Split scheduler service ownership by concept

#### Implementation plan

1. Split `service/scheduler.rs` into concept-owned modules, likely including:
   - scheduled-job CRUD
   - cron-job CRUD
   - async and cancellable wrapper normalization
   - loaded-tenant scan and next-work coordination helpers
2. Keep `Service` as the public surface while moving implementation under it.
3. Avoid reintroducing duplicated async or sync wrapper logic where a shared
   canonical helper is clearer.
4. Preserve scheduler wakeup, recovery, and result-recording behavior.

#### Focused verification

- targeted engine tests for scheduled jobs, cron behavior, and scheduler
  recovery
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- scheduler code is grouped by concept rather than mixed in one module
- wakeup, recovery, and async cancellation behavior stay unchanged
- public service naming and helper placement are clearer

### CO4. Decompose subscription-delivery ownership

#### Implementation plan

1. Split `tenant/subscription_delivery.rs` into concept-owned modules, likely
   including:
   - queue state and coalescing
   - dedicated worker lifecycle and shutdown
   - stats and accounting
   - testing pause controls
2. Keep explicit tenant-owned worker shutdown and join behavior.
3. Preserve delivery ordering, queue overflow fallback behavior, and monotonic
   stats reporting.

#### Focused verification

- targeted engine tests for subscription delivery ordering, queueing, and
  shutdown cleanup
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- delivery queue ownership, worker lifecycle, and stats accounting are no
  longer intertwined in one file
- delivery behavior and shutdown semantics stay unchanged
- test-only pause seams are isolated from production control flow

### CO5. Normalize Convex host-bridge direct ctx-op ownership

#### Implementation plan

1. Split or normalize
   `adapters/convex/host_bridge/function_ops/ctx_ops/direct.rs` so repeated
   sync, async, and cancellable query or mutation wrapper flows have one clear
   canonical home.
2. Keep the current runtime result encoding, auth handling, and execution-unit
   short-circuit behavior unchanged.
3. Group helper code by query, pagination, mutation, action, or common ctx-op
   wrapper ownership rather than by transport shape alone.

#### Focused verification

- targeted server and runtime tests for Convex query, pagination, mutation, and
  action host calls
- `cargo test -p neovex-runtime`
- `cargo test -p neovex-server`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the direct ctx-op surface no longer repeats large parallel wrapper shapes
- runtime host-call behavior, auth, and error mapping stay unchanged
- the code is grouped by owning concept instead of transport-shape duplication

### CO6. Concept-owned test surfaces and idiomatic Rust cleanup sweep

#### Implementation plan

1. Move or split the highest-value clumped tests toward concept-owned module
   surfaces where that improves maintainability.
2. Keep broad integration coverage intact; do not over-fragment tests that are
   more valuable as cross-subsystem coverage.
3. Tighten visibility, helper placement, naming, and local state ownership
   across the stabilized module trees.
4. Remove leftover glue, dead helpers, or no-longer-needed indirection created
   by the earlier structural items.

#### Focused verification

- targeted crate tests for every subsystem whose tests moved or whose naming or
  visibility changed
- `cargo test -p neovex-runtime`
- `cargo test -p neovex-engine`
- `cargo test -p neovex-storage`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the highest-value test surfaces are easier to navigate by concept
- naming, visibility, and helper placement are more idiomatic and consistent
- the cleanup does not reintroduce helper piles or false modularity

### CO7. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md` to reflect the landed ownership map if any
   architecture-level module ownership changed.
2. Update `docs/plans/README.md`, `README.md`, `docs/README.md`, and
   `AGENTS.md` if control-plane ownership or entrypoints changed during the
   workstream.
3. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
4. Run the repo-wide verification sweep and record it here.

#### Focused verification

- `make check`
- `make test`
- `make clippy`

#### Acceptance criteria

- docs reflect the landed architecture and plan ownership
- the full verification sweep is recorded
- the plan can be archived cleanly once the workstream is complete

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-03 | CO0 | done | reviewed the live post-modularity architecture, confirmed the remaining hotspots are `runtime/bootstrap.rs`, `store.rs`, scheduler, subscription delivery, direct ctx-op wrappers, and clumped tests, then promoted this plan as the next control plane | docs-only planning pass; relied on the previously green MC workstream baseline recorded above | archive the completed modularity plan, update repo entrypoints to the new control plane, commit the completed modularity cleanup plus this new planning state, then start `CO1` |
| 2026-04-03 | CO1 | done | replaced the monolithic `runtime/bootstrap.rs` file with a concept-owned bootstrap module tree for payloads, op registration/handlers, bootstrap source install, runtime state and timeout control, and startup snapshot/isolate-pool ownership, while keeping `runtime.rs` as the stable public root | `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | start `CO2` by decomposing `crates/neovex-storage/src/store.rs` into a storage module tree rooted in durable writes, journal/recovery, read snapshots, and scan/probing ownership |
| 2026-04-03 | CO2 | done | replaced the storage `store.rs` god file with a concept-owned storage module tree: `write.rs` owns direct durable write and batch-apply behavior, `journal.rs` owns durable journal append/read/replay/recovery, `read.rs` owns read snapshots and scan entrypoints, `scan.rs` owns pushdown plus MessagePack probing, and `schema_rewrite.rs` owns schema-aware index rewrite helpers | `bash scripts/cargo-isolated.sh -- test -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | start `CO3` by splitting `crates/neovex-engine/src/service/scheduler.rs` into concept-owned modules for scheduled jobs, cron jobs, async/cancellable wrappers, and tenant coordination |
| 2026-04-03 | CO3 | done | replaced the mixed scheduler service file with a concept-owned scheduler module tree: `scheduled_jobs.rs` owns scheduled-job CRUD and result persistence, `cron.rs` owns cron CRUD, `access.rs` owns the shared sync/async/cancellable tenant-store wrapper flows, and `coordination.rs` owns loaded-tenant scans, startup recovery, and next-due work discovery | `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | start `CO4` by decomposing `crates/neovex-engine/src/tenant/subscription_delivery.rs` around queue state, worker lifecycle, stats, and test-only pause seams |
| 2026-04-03 | CO4 | done | replaced the mixed tenant subscription-delivery file with a concept-owned module tree: `queue.rs` owns bounded queue state and batch draining, `worker.rs` owns the dedicated worker lifecycle and shutdown, `stats.rs` owns delivery metrics and stats snapshots, and `pause.rs` isolates the test-only pause seam while `subscription_delivery.rs` stays the tenant-facing composition root | `bash scripts/cargo-isolated.sh -- test -p neovex-engine subscription_delivery_queue_`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | start `CO5` by normalizing `crates/neovex-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/direct.rs` around one canonical direct ctx-op wrapper flow |
| 2026-04-03 | CO5 | done | replaced the repeated direct Convex ctx-op wrapper file with a concept-owned direct module tree: `direct/execution.rs` owns execution-context dispatch and execution-unit short-circuit behavior, while `direct/invocation.rs` owns runtime payload decode/validate/encode and the default-cancellation wrapper flow for query, pagination, mutation, and action entrypoints | `cargo test -p neovex-server`; `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | start `CO6` by moving the highest-value clumped tests toward concept-owned surfaces and cleaning up leftover naming, visibility, and helper placement across the stabilized module trees |
| 2026-04-03 | CO6 | done | moved the highest-value scheduler and subscription-delivery regression clusters from the giant engine root test module into `service/scheduler/tests.rs` and `tenant/subscription_delivery/tests.rs`, removed the duplicate root-level copies and dead helpers from `crates/neovex-engine/src/tests.rs`, and tightened the moved scheduler schema helper so the concept-owned test surfaces preserve the prior no-policy baseline | `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`; `bash scripts/cargo-isolated.sh -- test -p neovex-storage`; `cargo check --workspace`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | finish `CO7` by recording the make-based repo-wide sweep, then archive this completed control plane and update the repo entrypoints so new agents do not resume finished cleanup work |
| 2026-04-03 | CO7 | done | recorded the green repo-wide `make check`, `make test`, and `make clippy` sweep, then archived this completed control plane and updated the plan index plus `AGENTS.md` so the repo no longer presents this finished cleanup pass as active work | `make check`; `make test`; `make clippy` | completed; future modularity follow-on work should start from a new active plan rather than this archived record |
