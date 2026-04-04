# Deep Module Ownership And Canonical Cleanup Control Plan

Archived on 2026-04-03 after `DM0` through `DM7` completed. Historical record
only; do not resume this plan as a live control plane.

This is the canonical execution control plane for the next deeper
module-ownership, canonical naming, and idiomatic Rust cleanup workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-engine/src/tenant/materialized_reads.rs`
- `crates/neovex-storage/src/index.rs`
- `crates/neovex-engine/src/service/mutations/direct.rs`
- `crates/neovex-engine/src/service/queries/planner.rs`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow.rs`

Baseline verification status for this plan:

- the immediately preceding deeper concept-ownership cleanup workstream closed
  green on 2026-04-03 with:
  `bash scripts/cargo-isolated.sh -- test -p neovex-engine`,
  `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`,
  `bash scripts/cargo-isolated.sh -- test -p neovex-storage`,
  `cargo check --workspace`,
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `make check`,
  `make test`,
  and `make clippy`
- this control plane was authored as a docs-only review-and-planning pass on
  2026-04-03 after that verified baseline
- no `DM*` implementation work has landed yet; `DM1` must record its own
  focused verification before it can be marked `done`

---

## Purpose

Neovex is in a much healthier architectural state than it was before the last
two cleanup passes. The transport, runtime bootstrap, storage root, scheduler,
subscription delivery, and direct Convex ctx-op seams now have clearer
composition roots and more explicit ownership.

The remaining cleanup work is deeper in the stack. The next pass should focus
on the implementation hotspots that still mix multiple concepts inside one
module, and on the highest-value scenario test surfaces that are still too
clumped to navigate comfortably. The goal is not to chase lower line counts.
The goal is to make ownership more obvious, future features easier to place,
debugging more local, and naming plus helper placement more canonical.

This is a maintainability and correctness roadmap, not a product-feature
roadmap.

---

## Relationship To Other Plans

Use `docs/plans/README.md` as the owning plan index. If work turns into
encryption-at-rest, Locker fork, Convex compatibility, or layered
admission-control work, stop and move to the owning plan instead of stretching
this one across multiple streams.

---

## Scope

This plan covers:

- deeper concept-owned decomposition of the remaining serving, indexing,
  direct-mutation, and planner hotspots
- cleanup of naming, helper placement, and visibility in those stabilized
  module trees
- movement of the highest-value clumped storage and server scenario tests
  toward concept-owned surfaces after the production boundaries settle
- documentation and control-plane updates needed to keep the work resumable
  through compaction, interruption, and handoff

This plan does not cover:

- new product features
- intentional wire or route behavior changes unless an item explicitly records
  them
- storage-format changes
- planner capability expansion beyond preserving current semantics
- admission-control redesign
- Locker fork or runtime scheduling redesign
- speculative performance work that is not justified by ownership,
  correctness, or maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Native CRUD, query, paginated query, scheduler, journal, Convex runtime,
   and WebSocket semantics stay unchanged unless a specific item explicitly
   records otherwise.

2. Keep core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split is one where the owning concept is easier to identify,
   not one where code merely moved into more files.

4. Keep composition roots thin once ownership moves out.
   Do not create renamed god files.

5. Keep serving, durability, planner, cancellation, and shutdown semantics
   explicit and testable.

6. Add focused regression coverage before moving high-risk serving, indexing,
   planner, or direct-mutation seams.

7. Prefer canonical naming, visibility, and state ownership over clever
   abstractions.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

This plan assumes the codebase described in `ARCHITECTURE.md` today:

- `runtime/bootstrap/`, `store/`, `service/scheduler/`, and
  `tenant/subscription_delivery/` are now concept-owned module trees instead
  of monolithic implementation files.
- `runtime.rs`, `executor.rs`, `store.rs`, `service/scheduler.rs`, and
  `tenant/subscription_delivery.rs` are now thin enough that they are not the
  first-wave production hotspots for this pass.
- the last cleanup pass also moved the highest-value scheduler and
  subscription-delivery engine regressions into module-local test files.

The current hotspot map from the live worktree is:

- `crates/neovex-engine/src/tenant/materialized_reads.rs` is 1339 lines and
  still mixes serving snapshots, backend residency and eviction, warm-load
  coordination, stats snapshots, and test pause seams
- `crates/neovex-storage/src/index.rs` is 1077 lines and still mixes scalar
  encoding, key construction, scan execution, composite bound synthesis,
  transaction-side maintenance, and read-snapshot convenience methods
- `crates/neovex-engine/src/service/mutations/direct.rs` is 961 lines and
  still mixes public CRUD convenience APIs, async and cancellable wrappers,
  principal variants, execution-mode/result helpers, and direct store-backed
  mutation execution
- `crates/neovex-engine/src/service/queries/planner.rs` is 929 lines and
  still mixes exact and range planning, residual-filter derivation, candidate
  scoring, order support, and plan-backed document loading
- `crates/neovex-storage/src/tests.rs` is 3014 lines and
  `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow.rs` is
  2312 lines; both remain high-value but still concept-mixed scenario surfaces

Large files that are currently more cohesive and are not first-wave targets
for this plan:

- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-storage/src/simulation.rs`

They may still deserve future cleanup, but they are not the best first slices
unless a later item reveals a clearer ownership break.

---

## Current Review Findings

These findings describe the current reasons this plan exists.

1. `crates/neovex-engine/src/tenant/materialized_reads.rs` is the clearest
   remaining production god file.
   It combines serving snapshot retention, in-memory serving backend
   ownership, warm-load coordination, residency accounting, public stats, and
   test-only publish pause seams.

2. `crates/neovex-storage/src/index.rs` still mixes several different storage
   concepts.
   Public index encoding and key helpers, exact/range/prefix scans, composite
   bound synthesis, transaction-side index maintenance, and read-snapshot
   convenience methods all live together.

3. `crates/neovex-engine/src/service/mutations/direct.rs` is still dense at
   the direct service boundary.
   Public CRUD convenience APIs, async/principal/cancellable wrapper flows,
   scheduled execution-mode branching, and direct store-backed mutation
   helpers are all expressed in one module.

4. `crates/neovex-engine/src/service/queries/planner.rs` still combines too
   many planner concerns.
   Candidate selection, exact and range bound synthesis, order support
   scoring, residual-filter derivation, and plan-backed document loading are
   all coupled together.

5. The biggest remaining test surfaces are still concept-mixed.
   `neovex-storage/src/tests.rs` and the Convex demo-flow runtime tests are
   valuable coverage, but they are harder to extend and debug than they should
   be.

6. Some large files are now deceptive hotspots.
   `runtime.rs`, `executor.rs`, and `simulation.rs` are large, but they are
   not the best next modularity slices compared with the serving, indexing,
   planner, and direct-mutation surfaces above.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The current feature set and stable behavior still work after the cleanup.
2. `tenant/materialized_reads.rs` is split around clear serving concepts
   instead of remaining a serving catch-all.
3. `index.rs` no longer owns unrelated index concepts in one file and the
   encoding, bounds, scan, and maintenance ownership map is clearer.
4. `service/mutations/direct.rs` and `service/queries/planner.rs` are grouped
   by concept and use more canonical internal patterns.
5. The highest-value storage and Convex demo scenario tests are easier to
   navigate by concept without losing integration coverage.
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
| Materialized read surface and serving snapshots | full-scan warmup, pinned snapshot visibility, reuse, retention, and stats shapes stay unchanged | targeted engine tests and server runtime-only full-scan tests when touched |
| Storage indexing and scan behavior | scalar encoding, exact/range/prefix scan semantics, and index maintenance stay unchanged | targeted storage tests plus engine query tests when touched |
| Direct mutation service surface | insert/update/delete sync, async, cancellable, principal-aware, and scheduled semantics stay unchanged | targeted engine tests plus runtime/server tests when host-call paths are touched |
| Query planning and prepared execution | exact/composite/range selection, residual filters, order handling, and pagination semantics stay unchanged | targeted engine planner and query tests |
| Native and Convex integration scenarios | demo runtime HTTP flows, native journal/query routes, and storage recovery scenarios stay unchanged | targeted server/storage scenario tests |
| Runtime admission, cancellation, timeout, fairness, and shutdown | no semantic drift from cleanup in runtime-facing behavior | runtime tests when touched indirectly by shared refactors |

Exact targeted commands used for each item must be recorded in `Execution Log`.

---

## Control Plane Rules

This document is the durable control plane for this cleanup workstream. The
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
7. Update this plan's ledger, checkpoint, and log in the same change set as
   the code or docs.
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
- Do not mix behavior change and structural cleanup in the same item unless
  the item explicitly allows it.
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
Historical note: this prompt applied while the plan was active. Now that the
plan is archived, start from docs/plans/README.md and choose an active plan
instead of resuming this one.
```

---

## Verification Contract

### Minimum verification per implementation item

- targeted tests for the touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`

### Additional verification by scope

- for serving-snapshot or materialized-read cleanup:
  `cargo test -p neovex-engine`
- for storage index or scan cleanup:
  `cargo test -p neovex-storage`
- for direct mutation or planner cleanup:
  `cargo test -p neovex-engine`
- for server demo-flow or Convex runtime scenario test cleanup:
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
| DM0 | done | Baseline review and hotspot map for the next deeper module-ownership cleanup pass | none |
| DM1 | done | Split `crates/neovex-engine/src/tenant/materialized_reads.rs` into a concept-owned `tenant/materialized_reads/` serving module tree | DM0 |
| DM2 | done | Decompose `crates/neovex-storage/src/index.rs` into a concept-owned `index/` module tree for encoding, key layout, bounds, scan, and maintenance ownership | DM0 |
| DM3 | done | Split `crates/neovex-engine/src/service/mutations/direct.rs` into a concept-owned `direct/` module tree for API, execution, store-apply, and execution-type ownership | DM0 |
| DM4 | done | Decompose `crates/neovex-engine/src/service/queries/planner.rs` into a concept-owned `planner/` module tree for exact planning, range planning, loading, and scoring ownership | DM0, DM2 |
| DM5 | done | Split the highest-value clumped storage and Convex demo scenario tests into more concept-owned surfaces | DM1, DM2, DM3, DM4 |
| DM6 | done | Perform the idiomatic Rust, naming, visibility, and helper-placement sweep across the stabilized module trees | DM1, DM2, DM3, DM4, DM5 |
| DM7 | done | Update docs and run the full verification sweep | DM1, DM2, DM3, DM4, DM5, DM6 |

---

## Dependency Graph

- `DM0` is the current architecture review and hotspot baseline.
- `DM1`, `DM2`, and `DM3` are the first major structural items because they
  target the clearest remaining production concept mixes.
- `DM4` follows `DM2` so planner cleanup can align with the stabilized index
  and bound-synthesis ownership map.
- `DM5` waits until the main production boundaries land so scenario and
  storage test movement follows the final structure.
- `DM6` is the naming, visibility, and helper-placement sweep after the major
  boundaries and test surfaces stabilize.
- `DM7` is the closure pass that updates docs and runs the full verification
  sweep.

---

## Recommended Delivery Order

1. `DM1`
2. `DM2`
3. `DM3`
4. `DM4`
5. `DM5`
6. `DM6`
7. `DM7`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| DM0 | done; reviewed the live post-cleanup architecture, confirmed the next real production hotspots are `tenant/materialized_reads.rs`, `storage/index.rs`, `service/mutations/direct.rs`, and `service/queries/planner.rs`, and confirmed the next high-value test surfaces are `storage/tests.rs` and the Convex demo-flow runtime scenarios | start `DM1` by mapping `tenant/materialized_reads.rs` into concept-owned seams for serving snapshots, the in-memory serving backend, warm-load coordination, stats, and test-only publish pause controls |
| DM1 | done; replaced `tenant/materialized_reads.rs` with a `tenant/materialized_reads/` module tree where `mod.rs` is the composition root, `snapshot.rs` owns serving snapshots and tenant-level retention, `backend.rs` owns in-memory publication/residency/eviction, `warm_load.rs` owns shared load coordination, `stats.rs` owns public metrics snapshots, and `pause.rs` keeps the test-only publish pause seam. `ARCHITECTURE.md` now reflects that ownership map. | start `DM2` by decomposing `storage/index.rs` around encoding/key helpers, scan execution, composite bounds, and transaction-side maintenance |
| DM2 | done; replaced `storage/index.rs` with an `index/` module tree where `mod.rs` is the composition root, `encoding.rs` owns scalar/tuple encoding, `keyspace.rs` owns key construction and prefixes, `bounds.rs` owns composite range-bound synthesis, `scan.rs` owns exact/prefix/range scan execution plus read-snapshot convenience methods, and `maintenance.rs` owns transaction-side index maintenance plus table rebuild/clear helpers. `ARCHITECTURE.md` now reflects that ownership map. | start `DM3` by splitting `service/mutations/direct.rs` around public CRUD wrappers, wrapper normalization, execution-mode/result helpers, and direct store-backed execution/auth staging |
| DM3 | done; replaced `service/mutations/direct.rs` with a `direct/` module tree where `api.rs` owns the direct CRUD service surface and wrapper normalization, `execution.rs` owns execution-mode dispatch and auth staging, `store.rs` owns direct store-apply helpers, and `types.rs` owns the shared execution mode/result contract. `ARCHITECTURE.md` now reflects that ownership map. | start `DM4` by decomposing `service/queries/planner.rs` around exact planning, range planning, residual-filter derivation, candidate scoring, and plan-backed loading helpers |
| DM4 | done; replaced `service/queries/planner.rs` with a `planner/` module tree where `mod.rs` remains the planner surface, `exact.rs` owns exact-prefix planning, `range.rs` owns range-bound derivation and range-plan selection, `loading.rs` owns plan-backed document loading over stores/snapshots/in-memory slices, and `scoring.rs` owns candidate scoring plus order support. `ARCHITECTURE.md` now reflects that ownership map. | start `DM5` by moving the highest-value storage and Convex demo scenario tests into more concept-owned surfaces while keeping the broad integration coverage intact |
| DM5 | done; moved the storage index semantics cluster from `storage/tests.rs` into `index/tests.rs`, and split the Convex demo runtime scenarios into `http_routes/demo_flow/mod.rs` plus `seeded_usage.rs` so the deterministic/faulted usage scenarios no longer live in one monolithic test file. Broad storage and server integration coverage remains intact. | start `DM6` by tightening naming, visibility, helper placement, and leftover glue across the stabilized serving, indexing, mutation, planner, and test module trees |
| DM6 | done; tightened the refactor-loosened visibility in the direct-mutation module tree and trimmed leftover public helper surface from the storage `index/` tree so the landed architecture is also canonical at the API/helper-placement level. `MutationExecutionMode` and `MutationExecutionResult` are now scoped to `crate::service::mutations`, and the indexing keyspace helpers now expose only the planner/storage entrypoints that the rest of the crate actually uses. | start `DM7` by reconciling the plan index and agent entrypoints with the final workstream status, then run the repo-wide verification sweep |
| DM7 | done; archived this completed control plane under `docs/plans/archive/`, removed its live-agent entrypoint references, and recorded the repo-wide verification closure state. `make check`, `make test`, and `make clippy` all passed. `make ci` was also attempted, but the environment does not have `cargo deny` installed, so the `deny` step could not run locally. | if future cleanup work is promoted, start from `docs/plans/README.md` and author or promote a new active control plane instead of resuming this archived one |

---

## Work Items

### DM0. Baseline review and hotspot map

Completed in this planning pass.

Acceptance criteria:

- the next cleanup plan is grounded in the live post-cleanup architecture
- the roadmap targets real remaining concept mixes instead of stale hotspots
- the plan is self-sufficient enough to resume after compaction or handoff

### DM1. Split `tenant/materialized_reads.rs` by serving concept ownership

#### Implementation plan

1. Keep `tenant/materialized_reads.rs` as a thin composition root or replace
   it with a `tenant/materialized_reads/` module tree.
2. Extract grouped concepts into dedicated modules, likely including:
   - serving snapshot types and retention management
   - in-memory serving backend residency, eviction, and publication ownership
   - warm-load coordination and waiter behavior
   - stats and public metrics snapshot types
   - test-only publish pause controls
3. Preserve serving-snapshot coverage, pinning, and reuse semantics.
4. Keep public testing hooks intentional and minimal.

#### Focused verification

- targeted engine tests for full-scan warmup, serving snapshot pinning, and
  publication or retention behavior
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `tenant/materialized_reads.rs` no longer owns unrelated serving concepts in
  one file
- serving snapshot, backend, warm-load, and stats ownership are explicit
- serving semantics and stats shapes stay unchanged

### DM2. Decompose `storage/index.rs` around indexing concepts

#### Implementation plan

1. Keep `storage/index.rs` as a thin composition root or replace it with an
   `index/` module tree.
2. Extract grouped concepts into dedicated modules, likely including:
   - scalar and tuple encoding helpers
   - key construction and prefix helpers
   - exact, prefix, and range scan execution
   - composite range-bound synthesis
   - transaction-side index maintenance and read-snapshot convenience methods
3. Preserve index maintenance semantics, range ordering, and scan behavior.
4. Keep public exports intentional and minimal.

#### Focused verification

- targeted storage tests for index encoding, exact/range/prefix scans, and
  index maintenance
- `cargo test -p neovex-storage`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `index.rs` is no longer the owner of unrelated index concepts
- encoding, bounds, scan, and maintenance ownership are explicit
- index semantics and ordering stay unchanged

### DM3. Split direct mutation service ownership by concept

#### Implementation plan

1. Split or normalize `service/mutations/direct.rs` so the direct mutation
   service surface has one clear canonical home for each concept.
2. Separate grouped concepts, likely including:
   - public CRUD convenience APIs
   - async, principal-aware, and cancellable wrapper normalization
   - execution-mode and result helper ownership
   - direct store-backed execution helpers and auth staging
3. Keep direct mutation behavior, result mapping, and scheduled execution
   semantics unchanged.

#### Focused verification

- targeted engine tests for direct insert/update/delete, async mutation
  behavior, and scheduled execution flows
- targeted server or runtime tests when host-call mutation paths are touched
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the direct mutation service surface no longer repeats large parallel wrapper
  shapes in one file
- direct mutation behavior, auth, and result mapping stay unchanged
- code is grouped by owning concept instead of wrapper-shape duplication

### DM4. Decompose planner ownership by concept

#### Implementation plan

1. Split or normalize `service/queries/planner.rs` so exact planning, range
   planning, candidate scoring, residual-filter derivation, and plan-backed
   loading have clearer ownership.
2. Keep current planner capability and semantics unchanged.
3. Preserve the current exact vs range selection behavior and metric mapping.
4. Keep the planner surface easy to read from top to bottom.

#### Focused verification

- targeted engine tests for exact, composite, range, pagination, and fallback
  planner behavior
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- planner ownership is clearer and more local by concept
- plan selection, residual filters, and plan-backed loading stay unchanged
- the planner surface is easier to extend without cross-cutting edits

### DM5. Concept-owned scenario and storage test surfaces

#### Implementation plan

1. Move or split the highest-value clumped storage and Convex demo-flow tests
   toward concept-owned surfaces where that improves maintainability.
2. Keep broad integration coverage intact; do not over-fragment tests that are
   more valuable as cross-subsystem scenarios.
3. Likely targets include:
   - breaking `storage/tests.rs` into clearer journal, index, recovery, and
     usage-oriented surfaces
   - breaking the Convex demo-flow runtime tests into registry or fixture
     setup, request helpers, seeded scenario modeling, and scenario-specific
     test modules
4. Keep test-only helpers close to the scenarios they serve.

#### Focused verification

- targeted storage and server test modules for every moved scenario cluster
- `cargo test -p neovex-storage`
- `cargo test -p neovex-server`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the highest-value scenario tests are easier to navigate by concept
- integration coverage stays intact
- test helpers are closer to the scenarios they support

### DM6. Idiomatic Rust and canonical naming sweep

#### Implementation plan

1. Tighten naming, visibility, helper placement, and state ownership across
   the stabilized module trees.
2. Remove leftover glue, dead helpers, or no-longer-needed indirection created
   by the earlier structural items.
3. Prefer the simplest idiomatic Rust shape that clarifies ownership.

#### Focused verification

- targeted crate tests for every subsystem whose naming, visibility, or helper
  placement changed
- `cargo test -p neovex-engine`
- `cargo test -p neovex-storage`
- `cargo test -p neovex-server`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- naming, visibility, and helper placement are more idiomatic and consistent
- cleanup does not reintroduce helper piles or false modularity

### DM7. Docs and full verification closure

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
| 2026-04-03 | DM0 | done | reviewed the live post-cleanup architecture, confirmed the next real hotspots are `tenant/materialized_reads.rs`, `storage/index.rs`, `service/mutations/direct.rs`, `service/queries/planner.rs`, and the remaining clumped storage plus Convex demo scenario tests, then promoted this plan as the next active control plane | docs-only planning pass; relied on the previously green deeper concept-ownership cleanup baseline recorded above | update the plan index and agent entrypoints to this new control plane, commit the completed earlier cleanup handoff plus this new planning state, then start `DM1` |
| 2026-04-03 | DM1 | done | Replaced `crates/neovex-engine/src/tenant/materialized_reads.rs` with a concept-owned `tenant/materialized_reads/` module tree. `mod.rs` is now the thin `TenantMaterializedReadSurface` composition root over `snapshot.rs`, `backend.rs`, `warm_load.rs`, `stats.rs`, and the test-only `pause.rs`; serving semantics, retention, warm-load waiting, stats, and publish-pause behavior were preserved, and `ARCHITECTURE.md` was updated to describe the new ownership map. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; attempted `cargo check --workspace`, but it was blocked by an unrelated external Cargo process holding the shared package cache; attempted `bash scripts/cargo-isolated.sh -- check --workspace`, but sandbox network restrictions prevented `rusty_v8` from downloading its prebuilt artifact | start `DM2` by splitting `crates/neovex-storage/src/index.rs` around scalar/tuple encoding, key construction, scan execution, composite bounds, and transaction-side maintenance |
| 2026-04-03 | DM2 | done | Replaced `crates/neovex-storage/src/index.rs` with a concept-owned `index/` module tree. `mod.rs` now composes `encoding.rs`, `keyspace.rs`, `bounds.rs`, `scan.rs`, and `maintenance.rs` while keeping the public storage and planner-facing index API stable. `ARCHITECTURE.md` now records the new storage indexing ownership map. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-storage`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; attempted `cargo check --workspace`, but it was still blocked by an unrelated external Cargo process holding the shared package cache | start `DM3` by splitting `crates/neovex-engine/src/service/mutations/direct.rs` around CRUD wrappers, wrapper normalization, execution-mode/result helpers, and direct store-backed execution |
| 2026-04-03 | DM3 | done | Replaced `crates/neovex-engine/src/service/mutations/direct.rs` with a concept-owned `direct/` module tree. `api.rs` now owns the direct CRUD service surface plus async/principal/cancellable wrapper normalization, `execution.rs` owns execution-mode dispatch and mutation auth staging, `store.rs` owns the direct store-apply helpers, and `types.rs` owns the shared execution mode/result contract. `ARCHITECTURE.md` now records that ownership map. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check --workspace` | start `DM4` by decomposing `crates/neovex-engine/src/service/queries/planner.rs` around exact planning, range planning, residual-filter derivation, candidate scoring, and plan-backed loading helpers |
| 2026-04-03 | DM4 | done | Replaced `crates/neovex-engine/src/service/queries/planner.rs` with a concept-owned `planner/` module tree. `mod.rs` remains the planner surface, `exact.rs` owns exact-prefix planning, `range.rs` owns range-bound derivation and range-plan selection, `loading.rs` owns plan-backed loading over stores, snapshots, and in-memory documents, and `scoring.rs` owns candidate scoring plus requested-order support. `ARCHITECTURE.md` now records that ownership map. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check --workspace` | start `DM5` by moving the highest-value storage and Convex demo scenario tests into concept-owned test surfaces without dropping broad coverage |
| 2026-04-03 | DM5 | done | Moved the storage index regression cluster out of `crates/neovex-storage/src/tests.rs` into `crates/neovex-storage/src/index/tests.rs`, and split the Convex demo runtime scenario file into `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs` plus `seeded_usage.rs` so the seeded and faulted usage-model scenarios now live beside their own helpers instead of in one 2,300-line file. The storage and server suites still provide the same broad integration coverage, but the moved tests now report under concept-owned modules. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-storage`; `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check --workspace` | start `DM6` by tightening naming, visibility, helper placement, and leftover glue across the stabilized module trees |
| 2026-04-03 | DM6 | done | Tightened the public surface that had widened during the earlier refactors so the stabilized module trees are also idiomatic at the visibility boundary. The direct-mutation execution types are now scoped to `crate::service::mutations`, and the storage `index/` tree now exposes only the encoding helpers plus the crate-internal document-key helper while keeping the lower-level keyspace helpers private to the indexing modules. | `cargo fmt --all`; `bash scripts/cargo-isolated.sh -- test -p neovex-storage`; `bash scripts/cargo-isolated.sh -- test -p neovex-engine`; `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check --workspace` | finish `DM7` by reconciling docs and plan ownership, then run the repo-wide verification sweep and archive the completed control plane cleanly |
| 2026-04-03 | DM7 | done | Reconciled the docs and control-plane entrypoints for a finished workstream by archiving this plan under `docs/plans/archive/`, removing its live-plan references from `docs/plans/README.md` and `AGENTS.md`, and recording the final repo-wide verification state. No architecture-level ownership docs changed during closure, so `ARCHITECTURE.md`, `README.md`, and `docs/README.md` stayed as-is. | `make check`; `make test`; `make clippy`; attempted `make ci`, but it failed in the local environment because `cargo deny` is not installed (`cargo deny check` could not run) | if new deeper cleanup work is promoted later, create or promote a new active control plane from `docs/plans/README.md` |
