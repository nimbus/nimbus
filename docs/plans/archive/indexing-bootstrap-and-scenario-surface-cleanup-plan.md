# Indexing, Bootstrap, And Scenario Surface Cleanup Control Plan

Archived on 2026-04-06 after `IB1` through `IB8` completed. Historical record
only; do not resume as an active control plane.

This is the canonical execution control plane for the next modularity,
readability, and idiomatic-Rust cleanup pass after the execution-boundaries
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-storage/src/index/scan.rs`
- `crates/neovex-storage/src/index/maintenance.rs`
- `crates/neovex-storage/src/tests.rs`
- `crates/neovex-server/src/tests/core_http/documents_and_commits.rs`
- `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`
- `crates/neovex-runtime/src/runtime/bootstrap/snapshot.rs`
- `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`
- `crates/neovex-runtime/src/executor/admission.rs`

Baseline verification status for this plan:

- the immediately preceding cleanup workstream was completed and archived as
  `docs/plans/archive/execution-boundaries-and-integration-surface-cleanup-plan.md`
- this new control plane is being authored as a docs-only review-and-planning
  pass on 2026-04-06 while the repo still contains other active worktree dirt,
  especially the Locker fork and adjacent runtime/server surfaces
- no new workspace-wide verification is claimed by this planning pass
- every `IB*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup passes removed the largest top-level god files and thinned
many composition roots. The next worthwhile cleanup is narrower and deeper:
storage indexing internals, the remaining broad generated-history and demo
scenario surfaces, and the runtime bootstrap or admission seams that still
carry too many grouped concepts in one place.

This plan exists to keep moving the codebase toward concept-owned modules,
canonical naming, and easier debugging without splitting files merely to reduce
line counts. The target is clearer ownership at the indexing, bootstrap, and
integration-scenario seams so future feature work lands in obvious homes.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/warm-module-pool-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/archive/convex-demos-compatibility-plan.md`,
  `docs/plans/wasmtime-backend-plan.md`,
  and `docs/plans/layered-admission-control-plan.md`.
- If work turns into Locker-fork feature development, warm execution design,
  Wasmtime backend work, admission-control redesign, or compatibility-product
  work, stop and move to the owning plan instead of stretching this cleanup
  plan across multiple streams.

---

## Scope

This plan covers:

- storage index scan ownership inside
  `crates/neovex-storage/src/index/scan.rs`
- storage index-maintenance ownership inside
  `crates/neovex-storage/src/index/maintenance.rs`
- movement of the highest-value remaining generated-history, recovery, and
  native HTTP scenario roots toward concept-owned test surfaces, especially
  `crates/neovex-storage/src/tests.rs` and
  `crates/neovex-server/src/tests/core_http/documents_and_commits.rs`
- further decomposition of the Convex demo-flow fixture and scenario root in
  `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`
- runtime bootstrap snapshot or retained-runtime ownership inside
  `crates/neovex-runtime/src/runtime/bootstrap/snapshot.rs`
- runtime bootstrap host-op ownership inside
  `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`
- runtime executor admission and permit ownership inside
  `crates/neovex-runtime/src/executor/admission.rs`
- final naming, visibility, helper-placement, and documentation cleanup that
  falls out of the new ownership map

This plan does not cover:

- new product features
- intentional route, wire, or API behavior changes unless explicitly recorded
- Locker-fork feature work or runtime backend experiments
- warm execution backend design
- admission-control redesign
- compatibility code for pre-launch behavior

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Storage scan behavior, runtime bootstrap behavior, executor admission,
   and demo or native HTTP scenario semantics stay unchanged unless a specific
   item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes the owning concept easier to name, test, and debug.

4. Keep composition roots thin once ownership moves out.
   Do not rename a god file into a facade without actually moving ownership.

5. Keep index scan semantics, index maintenance, runtime bootstrap, and
   executor admission semantics explicit and testable.

6. Add focused regression coverage before moving high-risk indexing, bootstrap,
   or scenario seams.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

8. Do not overlap cleanup-only runtime edits with active Locker fork work until
   the runtime worktree is reconciled enough for safe ownership-only changes.

---

## Current Assessed State

- The repo no longer has an active general cleanup control plane, so another
  cleanup pass should start by promoting a fresh active plan rather than
  reopening archived work.
- The strongest remaining production hotspots are now deeper ownership seams
  inside storage indexing and runtime bootstrap or executor internals.
- The current worktree still contains overlapping Locker fork/runtime dirt, so
  storage and server scenario cleanup is the safest first production slice.
- The remaining largest test roots are broad integration and generated-history
  surfaces. They should be split only where concept ownership becomes clearer,
  not just because a file is large.

---

## Current Review Findings

1. `crates/neovex-storage/src/index/scan.rs` is the clearest remaining storage
   indexing hotspot.
   It combines exact, prefix, range, and composite-range scan algorithms,
   low-level document decode loops, read-transaction ownership, and
   `TenantStore` public adapters in one file.

2. `crates/neovex-storage/src/index/maintenance.rs` is still a mixed write-side
   indexing surface.
   Transaction-side insert/update/delete index maintenance, table index clear
   helpers, and `TenantStore` convenience wrappers still live together.

3. `crates/neovex-runtime/src/runtime/bootstrap/snapshot.rs` is the deepest
   remaining runtime bootstrap hotspot.
   Startup-snapshot ownership, construction-mode vocabulary, retained-runtime
   pool state, affinity-aware reuse, LRU bounds enforcement, and test-only
   bootstrap counters still live together.

4. `crates/neovex-runtime/src/runtime/bootstrap/ops.rs` still mixes every
   bootstrap host-op family in one module.
   Query-builder sync ops, async db/query terminals, mutation/action/scheduler
   ops, nested-call ops, and the shared async permit-lease glue all sit in one
   file.

5. `crates/neovex-runtime/src/executor/admission.rs` remains a dense
   operational hotspot.
   Dispatch handles, shared permit state, async host-call suspend or resume,
   queueing, and fairness bookkeeping still live in one boundary.

6. `crates/neovex-server/src/tests/core_http/documents_and_commits.rs`,
   `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`,
   and `crates/neovex-storage/src/tests.rs` are the strongest remaining
   concept-mixed scenario roots.
   Generated-history replay helpers, blocking fault injectors, recovery
   harness glue, and broad scenario assertions still live together in a way
   that makes new cases harder to place.

7. `crates/neovex-server/src/ws/socket.rs`,
   `crates/neovex-server/src/adapters/convex/subscriptions/socket/named_subscriptions.rs`,
   and `crates/neovex-runtime/src/metrics.rs` are no longer the right next
   targets.
   They now mostly read as facades or coherent composition roots rather than
   the most urgent cleanup seams for this pass.

---

## Success Criteria

This plan is successful only when all of the following are true:

- storage index scan and maintenance ownership are easier to trace locally
- the highest-value generated-history and demo scenario roots live closer to
  the concepts they protect
- runtime bootstrap snapshot, bootstrap host-op, and executor admission
  ownership are easier to name and debug locally
- naming, visibility, and helper placement are more idiomatic and consistent
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Feature Preservation Matrix

- Index exact, prefix, range, and composite-range scan behavior must remain
  unchanged, including cancellation and decoded-document semantics.
- Index maintenance and table index rebuild/clear semantics must remain
  unchanged.
- Runtime bootstrap startup-snapshot, retained-runtime reuse, affinity, reset,
  and pool-bound semantics must remain unchanged.
- Runtime bootstrap host-op behavior and executor admission, fairness,
  suspend/resume, timeout, cancellation, and shutdown semantics must remain
  unchanged.
- Generated-history, recovery, native HTTP, and Convex demo scenario semantics
  must remain unchanged.
- Existing broad storage, runtime, engine, and server integration coverage
  must remain intact even when tests move to concept-owned surfaces.

---

## Control Plane Rules

This document is the durable control plane for the current cleanup workstream.
The source of truth is:

1. the current git worktree
2. this plan's `Roadmap Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed ownership map and invariants
4. the referenced code, tests, and docs called out by the active item

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one `IB*` item in
  this state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a recorded gate

### Recovery loop for every new session

1. Reread `Cleanup Invariants`, `Current Assessed State`,
   `Current Review Findings`, `Feature Preservation Matrix`,
   `Verification Contract`, `Roadmap Status Ledger`,
   `Implementation Checkpoints`, `Dependency Graph`,
   `Recommended Delivery Order`, and `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan before
   picking new scope.
3. If any item is already `in_progress`, resume that item first.
4. If the worktree is dirty, identify which item owns the changes and update
   that item's checkpoint or log entry before starting new work.
5. Implement exactly one item by default.
6. Record verification in `Execution Log` before marking an item `done`.
7. If blocked, record the blocker here before stopping.

---

## Verification Contract

Always run the focused verification listed on the active item before marking it
`done`.

Always run:

- `cargo fmt --all --check`
- `cargo check --workspace`

Run, as appropriate:

- `cargo test -p neovex-storage`
- `cargo test -p neovex-server`
- `cargo test -p neovex-runtime`
- `cargo clippy --workspace --all-targets -- -D warnings`

Before considering the whole workstream complete, run:

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

If sandbox or environment restrictions block a command, do not silently skip
it. Run the best focused alternative, record the limitation in `Execution Log`,
and continue only when the blocker is environmental rather than architectural.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| IB0 | `done` | reviewed the current post-execution-boundaries architecture and identified the next meaningful cleanup hotspots in storage indexing, generated-history or demo scenario roots, and runtime bootstrap or admission ownership | none | docs-only review and planning pass on 2026-04-06 |
| IB1 | `done` | split `crates/neovex-storage/src/index/scan.rs` by index-scan ownership | none | landed as a thin `scan.rs` composition root over exact, prefix, range, read, and adapter submodules |
| IB2 | `done` | split `crates/neovex-storage/src/index/maintenance.rs` by write-side index-maintenance ownership | IB1 recommended first | landed as a thin `maintenance.rs` root over transaction, rebuild, and store-facing write wrapper submodules |
| IB3 | `done` | move the highest-value remaining generated-history, recovery, and native HTTP scenario helpers toward concept-owned surfaces | IB1 and IB2 recommended first | landed as dedicated storage generated-history tests plus owned native HTTP generated-history and fault helper submodules |
| IB4 | `done` | split the remaining Convex demo-flow fixture and scenario root by concept ownership | IB3 recommended first | the root is now a thin composition surface over manifest, bundle, registry, helpers, scenarios, and seeded-usage modules |
| IB5 | `done` | split `crates/neovex-runtime/src/runtime/bootstrap/snapshot.rs` by startup-snapshot and retained-runtime-pool ownership | IB1 through IB4 recommended first | landed as a thin `snapshot.rs` root over `snapshot/startup.rs` and `snapshot/retained_pool.rs` |
| IB6 | `done` | split `crates/neovex-runtime/src/runtime/bootstrap/ops.rs` by host-op family ownership | IB5 recommended first | landed as a thin `ops.rs` root over sync query-builder, async query, async effects, nested runtime, and shared host-call glue submodules |
| IB7 | `done` | split `crates/neovex-runtime/src/executor/admission.rs` by admission, permit, and fairness ownership | IB5 and IB6 recommended first | landed as a thin `admission.rs` root over dispatch-handle, shared permit, and tenant-fairness submodules |
| IB8 | `done` | update docs, run the full verification sweep, and archive the completed plan cleanly | IB1 through IB7 | completed with repo-wide verification plus an environment-only `make ci` limitation on `cargo deny` advisory-db locking |

---

## Dependency Graph

- `IB1` is the recommended first slice because it is isolated from the active
  runtime Locker work and sharpens the storage indexing vocabulary.
- `IB2` should usually follow `IB1` because both live in the storage index
  boundary and share transaction-side index-maintenance terminology.
- `IB3` and `IB4` come after the storage indexing seams stabilize.
- `IB5`, `IB6`, and `IB7` now proceed from the committed Locker/runtime
  baseline instead of waiting on a separate moving worktree.
- `IB6` should usually follow `IB5` because the bootstrap host-op families and
  retained-runtime construction vocabulary share the same runtime bootstrap
  seam.
- `IB7` should usually follow `IB6` because the runtime admission and permit
  vocabulary gets easier to name once the bootstrap seam is clearer.
- `IB8` closes the workstream after all production and scenario items land.

---

## Recommended Delivery Order

1. `IB1` — storage index scan ownership
2. `IB2` — storage index maintenance ownership
3. `IB3` — generated-history, recovery, and native HTTP scenario surfaces
4. `IB4` — Convex demo-flow fixture surface ownership
5. `IB5` — runtime bootstrap snapshot and retained-runtime-pool ownership
6. `IB6` — runtime bootstrap host-op family ownership
7. `IB7` — runtime executor admission ownership
8. `IB8` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| IB0 | done | start `IB1` by mapping `index/scan.rs` into low-level scan iterators, exact/prefix/range algorithms, and public read adapters |
| IB1 | done; `scan.rs` is now a composition root over `scan/read.rs`, `scan/exact.rs`, `scan/prefix.rs`, `scan/range.rs`, and `scan/adapters.rs` | start `IB2` by mapping transaction-side maintenance, rebuild/clear helpers, and store-facing wrappers inside `index/maintenance.rs` |
| IB2 | done; `maintenance.rs` is now a composition root over `maintenance/transaction.rs`, `maintenance/writes.rs`, and `maintenance/rebuild.rs` | start `IB3` by mapping the broad storage and native HTTP scenario roots into reusable generated-history, recovery, and helper seams |
| IB3 | done; storage generated-history and recovery coverage now lives under `tests/generated_history.rs`, and the native HTTP document/commit suite now owns generated-history and fault helpers in dedicated submodules | start `IB4` by mapping what still lives in the demo-flow root versus the already split helpers, scenarios, and seeded-usage surfaces |
| IB4 | done; the demo-flow root is now a composition surface over `manifest.rs`, `bundle.rs`, `registry.rs`, `helpers.rs`, `scenarios.rs`, and `seeded_usage/` | start `IB5` from the current committed bootstrap boundary |
| IB5 | done; `snapshot.rs` is now a thin root over `snapshot/startup.rs` for construction vocabulary and build accounting plus `snapshot/retained_pool.rs` for retained-runtime reuse, affinity, and bounds enforcement | start `IB6` by mapping host-op families and shared async permit-lease glue inside `runtime/bootstrap/ops.rs` |
| IB6 | done; `ops.rs` is now a thin root over `ops/sync_query_builder.rs`, `ops/async_query.rs`, `ops/async_effects.rs`, `ops/nested_runtime.rs`, and `ops/shared.rs` | start `IB7` by mapping dispatch handles, permit state, suspend/resume flow, and fairness bookkeeping inside `executor/admission.rs` |
| IB7 | done; `admission.rs` is now a thin root over `admission/dispatch.rs`, `admission/permit.rs`, and `admission/tenant_fairness.rs` | start `IB8` by running the repo-wide sweep, reconciling docs, and archiving the completed plan |
| IB8 | done; repo-wide verification completed, archive handoff recorded, and active entrypoints updated to stop treating this plan as live work | archived |

---

## Work Items

### IB0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### IB1. Split `index/scan.rs` by index-scan ownership

#### Implementation plan

1. Separate low-level scan iteration and document decode helpers from exact,
   prefix, range, and composite-range scan algorithms.
2. Keep `TenantStore` and `TenantReadSnapshot` public scan surfaces stable.
3. Preserve scan ordering, cancellation, and decode semantics exactly.

#### Focused verification

- `cargo test -p neovex-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- index-scan concepts are easier to find and extend
- exact, prefix, and range behavior no longer live in one mixed file

### IB2. Split `index/maintenance.rs` by index-maintenance ownership

#### Implementation plan

1. Separate transaction-side insert/update/delete index mutation helpers from
   table rebuild/clear helpers and `TenantStore` write-facing adapters.
2. Keep write-side index maintenance and commit semantics stable.
3. Preserve atomicity and schema-aware rebuild behavior exactly.

#### Focused verification

- `cargo test -p neovex-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- write-side indexing ownership is easier to reason about locally
- transaction internals and store-facing wrappers no longer live in one mixed
  file

### IB3. Concept-owned generated-history and native HTTP scenario surfaces

#### Implementation plan

1. Move the highest-value remaining generated-history, recovery, and native
   HTTP scenario helpers into concept-owned surfaces where it improves
   maintainability.
2. Prioritize `crates/neovex-storage/src/tests.rs` and
   `crates/neovex-server/src/tests/core_http/documents_and_commits.rs`.
3. Keep broad integration coverage intact while reducing the size of the
   remaining mixed roots.

#### Focused verification

- targeted crate tests for every moved surface
- `cargo test -p neovex-storage`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- generated-history and recovery helpers live closer to the concepts they
  protect
- the remaining storage and native HTTP test roots are materially smaller and
  more concept-owned

### IB4. Split the remaining Convex demo-flow fixture root by concept ownership

#### Implementation plan

1. Separate demo manifest/route builders, reusable fixture helpers, and broad
   scenario assertions into clearer surfaces under the demo-flow test tree.
2. Keep seeded usage and previously split scenario ownership stable.
3. Preserve current demo-flow and faulted-overlap semantics exactly.

#### Focused verification

- targeted demo-flow server tests
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the demo-flow root reads as a fixture composition surface rather than a mixed
  implementation pile
- shared demo-flow setup is easier to reuse for new cases

### IB5. Split `runtime/bootstrap/snapshot.rs` by startup-snapshot and retained-runtime ownership

#### Implementation plan

1. Separate construction-mode and startup-snapshot vocabulary from
   retained-runtime entry or pool state, reuse, and bounds enforcement.
2. Keep runtime bootstrap and retained-runtime semantics stable.
3. Preserve startup-snapshot build, retained-runtime reuse, affinity, and LRU
   pool behavior exactly.

#### Focused verification

- `cargo test -p neovex-runtime`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- bootstrap snapshot ownership is easier to debug locally
- retained-runtime pool state no longer lives in the same dense file as
  construction vocabulary and test counters

### IB6. Split `runtime/bootstrap/ops.rs` by host-op family ownership

#### Implementation plan

1. Separate sync query-builder ops, async db/query terminal ops,
   mutation/action/scheduler ops, nested runtime ops, and shared async
   permit-lease glue into clearer submodules.
2. Keep the runtime bootstrap op surface stable.
3. Preserve host-op naming, async lease, and cancellation semantics exactly.

#### Focused verification

- `cargo test -p neovex-runtime`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- bootstrap host-op ownership is easier to trace during debugging
- op families no longer live in one mixed file

### IB7. Split `executor/admission.rs` by admission and permit ownership

#### Implementation plan

1. Separate dispatch handles, shared permit state, async host-call
   suspend/resume flow, and tenant fairness bookkeeping into clearer modules.
2. Keep the public executor surface stable.
3. Preserve admission, fairness, timeout, cancellation, and shutdown semantics
   exactly.

#### Focused verification

- `cargo test -p neovex-runtime`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- executor admission ownership is easier to navigate under load or failure
- permit lifecycle and fairness logic no longer live in one dense file

### IB8. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md` if any architecture-level ownership map changed.
2. Update `docs/plans/README.md`, `AGENTS.md`, and other entrypoint docs if
   plan ownership changes during the workstream.
3. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
4. Run the repo-wide verification sweep and record it here.
5. Archive the completed plan once the workstream is fully done.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `make ci` if practical

#### Acceptance criteria

- docs reflect the landed architecture and plan ownership
- the full verification sweep is recorded
- the plan can be archived cleanly once the workstream is complete

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-06 | IB0 | done | Reviewed the live post-execution-boundaries architecture and identified the next meaningful cleanup hotspots in storage indexing, remaining generated-history and demo scenario roots, and runtime bootstrap or executor-admission ownership. Authored this new active cleanup control plane and prepared it for promotion in the plans index and agent entrypoint. | docs-only review and planning pass; no new code verification claimed in this handoff | start `IB1` with a concept map for `crates/neovex-storage/src/index/scan.rs` |
| 2026-04-06 | IB1 | done | Reconciled the active control plane against the current dirty worktree, then split `crates/neovex-storage/src/index/scan.rs` into concept-owned `read`, `exact`, `prefix`, `range`, and `adapters` submodules while keeping `scan.rs` as the thin composition root. Public `TenantStore` and `TenantReadSnapshot` scan entrypoints stayed stable. | `cargo test -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace` | start `IB2` by mapping transaction-side index maintenance, rebuild/clear helpers, and store-facing adapters in `crates/neovex-storage/src/index/maintenance.rs` |
| 2026-04-06 | IB2 | done | Continued directly within the same storage indexing boundary after `IB1` and split `crates/neovex-storage/src/index/maintenance.rs` into transaction-side mutation helpers, rebuild or clear helpers, and store-facing indexed write wrappers. `maintenance.rs` is now the thin composition root for those owned surfaces. | `cargo test -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace` | start `IB3` by mapping reusable generated-history, recovery, and native HTTP helper seams in the remaining broad scenario roots |
| 2026-04-06 | IB3 | done | Moved the storage generated-history and recovery cluster into `crates/neovex-storage/src/tests/generated_history.rs`, and moved the native HTTP generated-history oracle plus blocking fault harness into owned submodules under `crates/neovex-server/src/tests/core_http/documents_and_commits/`. The remaining roots now read more like broad scenario surfaces instead of mixed helper piles. | `cargo test -p neovex-storage`; `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo check --workspace` | start `IB4` by mapping what still belongs in the demo-flow root versus the already-split helpers, scenarios, and seeded-usage surfaces |
| 2026-04-06 | IB4 | done | Split the remaining Convex demo-flow root so `mod.rs` is now a thin composition surface over dedicated manifest builders, runtime-bundle generation, registry composition, helpers, broad scenarios, and the already-separated seeded-usage tree. | `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo check --workspace` | the next item is `IB5`, but only after the overlapping Locker/runtime worktree is reconciled enough for cleanup-only edits |
| 2026-04-06 | IB5 | done | Split `crates/neovex-runtime/src/runtime/bootstrap/snapshot.rs` into a thin composition root over `snapshot/startup.rs` and `snapshot/retained_pool.rs`. Startup-snapshot construction vocabulary, build accounting, retained-runtime reuse, affinity selection, and pool-bounds enforcement now have owned runtime-bootstrap surfaces without changing the external bootstrap API. | `cargo test -p neovex-runtime`; `cargo test -p neovex-server`; `cargo fmt --all --check`; `cargo check --workspace` | start `IB6` by separating host-op families and shared async permit-lease glue inside `crates/neovex-runtime/src/runtime/bootstrap/ops.rs` |
| 2026-04-06 | IB6 | done | Split `crates/neovex-runtime/src/runtime/bootstrap/ops.rs` into a thin extension-registration root over dedicated sync query-builder, async query, async effects, nested runtime, and shared host-call glue submodules. The serialized runtime host-op contract and op registration list stayed unchanged while the owned host-op families became easier to navigate. | `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p neovex-runtime`; `cargo test -p neovex-server` (one transient failure in the snapshot-aware delayed-async subprocess harness was observed on the first full runtime-suite pass, then the exact repro reruns and a clean full rerun passed) | start `IB7` by separating dispatch handles, permit state, suspend/resume flow, and fairness bookkeeping in `crates/neovex-runtime/src/executor/admission.rs` |
| 2026-04-06 | IB7 | done | Split `crates/neovex-runtime/src/executor/admission.rs` into a thin root over `admission/dispatch.rs`, `admission/permit.rs`, and `admission/tenant_fairness.rs`. Dispatch-handle ownership, shared permit suspend/resume lifecycle, and tenant fairness bookkeeping now live in owned executor submodules without changing the executor-facing contract. | `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p neovex-runtime`; `cargo test -p neovex-server`; `cargo clippy --workspace --all-targets -- -D warnings` | finish `IB8` with repo-wide verification, doc/index updates, and plan archiving |
| 2026-04-06 | IB8 | done | Ran the repo-wide closure sweep, updated `ARCHITECTURE.md` plus the plan index and `AGENTS.md`, and archived this completed cleanup control plane. | `make check`; `make test`; `make clippy`; `make ci` attempted but failed only because `cargo deny` could not lock `/Users/jack/.cargo/advisory-dbs/db.lock` on a read-only path | archived |
