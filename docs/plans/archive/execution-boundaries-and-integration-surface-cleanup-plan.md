# Execution Boundaries And Integration Surface Cleanup Control Plan

Archived on 2026-04-06 after `EB0` through `EB7` completed. This document is a
historical record; do not resume it as an active control plane.

This is the canonical execution control plane for the next modularity,
readability, and idiomatic-Rust cleanup pass after the stateful-execution
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/nimbus-storage/src/async_storage.rs`
- `crates/nimbus-storage/src/scheduler.rs`
- `crates/nimbus-runtime/src/executor.rs`
- `crates/nimbus-runtime/src/runtime/driver.rs`
- `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage.rs`
- `crates/nimbus-engine/src/tests.rs`
- `crates/nimbus-storage/src/tests.rs`

Baseline verification status for this plan:

- the immediately preceding cleanup workstream was completed and archived as
  `docs/plans/archive/stateful-execution-and-harness-cleanup-plan.md`
- this new control plane is being authored as a docs-only review-and-planning
  pass on 2026-04-06 while the repo still contains other active worktree dirt,
  especially in the Locker fork and adjacent runtime/server surfaces
- no new workspace-wide verification is claimed by this planning pass
- every `EB*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup passes removed the largest top-level god files and pushed a
lot of logic into clearer module trees. The next worthwhile cleanup is deeper:
execution-boundary ownership in storage and runtime, plus the remaining
scenario and integration test roots that are still too concept-mixed to extend
comfortably.

This plan exists to keep moving the codebase toward concept-owned modules,
canonical naming, and easier debugging without splitting files just to reduce
line counts. The target is clearer operational ownership, thinner composition
roots, and integration surfaces that are easier to understand when adding new
features or tracking regressions.

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

- async storage-boundary ownership inside
  `crates/nimbus-storage/src/async_storage.rs`
- scheduled-job and cron persistence ownership inside
  `crates/nimbus-storage/src/scheduler.rs`
- runtime executor ownership inside `crates/nimbus-runtime/src/executor.rs`
- runtime invocation-driver ownership inside
  `crates/nimbus-runtime/src/runtime/driver.rs`
- movement of the highest-value remaining scenario and integration tests toward
  concept-owned surfaces, especially
  `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage.rs`,
  `crates/nimbus-engine/src/tests.rs`, and
  `crates/nimbus-storage/src/tests.rs`
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
   Storage durability, runtime invocation behavior, scheduler persistence,
   and scenario semantics stay unchanged unless a specific item explicitly
   records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes the owning concept easier to name, test, and debug.

4. Keep composition roots thin once ownership moves out.
   Do not rename a god file into a facade without actually moving ownership.

5. Keep async storage cancellation, permit, and durable-commit semantics
   explicit and testable.

6. Keep runtime executor admission, fairness, timeout, cancellation,
   retained-runtime reuse, and shutdown semantics explicit and testable.

7. Add focused regression coverage before moving high-risk execution or
   scenario seams.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- The repo no longer has an active general cleanup control plane, so another
  modularity pass should start by promoting a fresh active plan rather than
  reopening archived work.
- The strongest remaining production hotspots are no longer the engine query or
  mutation roots that earlier passes already split. They are now deeper
  operational surfaces in storage and runtime.
- The current worktree still contains overlapping Locker fork/runtime dirt, so
  storage-side cleanup is the safest first production slice.
- The remaining largest test roots are mostly broad integration or generated
  scenario surfaces. They should be split only where concept ownership becomes
  clearer, not just because a file is large.

---

## Current Review Findings

1. `crates/nimbus-storage/src/async_storage.rs` is the clearest remaining
   storage boundary hotspot.
   It combines trait definitions, blocking read execution, blocking write
   execution, usage-store execution, tenant directory/open/delete ownership,
   and boundary-wide error or permit helpers in one file.

2. `crates/nimbus-storage/src/scheduler.rs` is still a mixed storage surface.
   Transaction-side pending/running job transitions, scheduled-job results,
   cron persistence, next-work scans, and recovery/list helpers still live
   together.

3. `crates/nimbus-runtime/src/executor.rs` is now the deepest runtime
   operational hotspot.
   Executor construction, worker startup and shutdown, async invoke APIs,
   blocking invoke wrappers, admission/dispatch integration, and a very large
   inline test inventory still live in one root.

4. `crates/nimbus-runtime/src/runtime/driver.rs` remains a dense invocation
   lifecycle surface.
   Runtime acquisition, driver preparation, bundle loading, post-load settle,
   retained-runtime reset, runtime creation, and tracing/error helpers are
   still tightly coupled.

5. `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage.rs`
   is now the biggest remaining demo scenario hotspot.
   Fault injector arming, seeded operation selection, snapshot modeling,
   overlap orchestration, and verification-harness wrappers still live in one
   scenario root.

6. `crates/nimbus-engine/src/tests.rs` and
   `crates/nimbus-storage/src/tests.rs` are still the biggest remaining
   concept-mixed integration roots.
   They now carry fewer obvious production extracts, but still own broad test
   clusters that would be easier to maintain closer to their concepts.

7. `crates/nimbus-runtime/src/metrics.rs` and
   `crates/nimbus-engine/src/tenant.rs` remain large, but they now mostly read
   as coherent public composition or diagnostics surfaces rather than the most
   urgent cleanup targets for this pass.

---

## Success Criteria

This plan is successful only when all of the following are true:

- async storage, scheduler persistence, runtime executor, and runtime driver
  ownership are easier to name and reason about locally
- the highest-value remaining scenario and integration roots live closer to
  the concepts they protect
- naming, visibility, and helper placement are more idiomatic and consistent
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Feature Preservation Matrix

- Async storage read, write, and usage-store execution semantics must remain
  unchanged, including cancellable pre-commit versus committed-write behavior.
- Scheduled-job, cron, recovery, and history semantics must remain unchanged.
- Runtime executor admission, queueing, fairness, retained-runtime reuse,
  timeout, cancellation, and shutdown semantics must remain unchanged.
- Runtime invocation-driver bundle loading, post-load settle, reset, and
  error-classification semantics must remain unchanged.
- Convex demo seeded usage and faulted-overlap scenario semantics must remain
  unchanged.
- Existing broad engine, storage, runtime, and server integration coverage
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
- `in_progress`: actively being implemented; keep exactly one `EB*` item in
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

- `cargo test -p nimbus-storage`
- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-server`
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
| EB0 | `done` | reviewed the current post-stateful-cleanup architecture and identified the next meaningful cleanup hotspots in async storage, scheduler persistence, runtime executor/driver ownership, and remaining seeded/integration test roots | none | docs-only review and planning pass on 2026-04-06 |
| EB1 | `done` | decomposed the async-storage boundary into concept-owned modules under `crates/nimbus-storage/src/async_storage/` while preserving pre-commit versus committed-write semantics | none | completed on 2026-04-06 with focused storage verification recorded below |
| EB2 | `done` | split the scheduler persistence boundary into concept-owned modules under `crates/nimbus-storage/src/scheduler/` while preserving due-order, recovery, and history semantics | EB1 recommended first, but not strictly required | completed on 2026-04-06 with focused storage verification recorded below |
| EB3 | `done` | split the runtime executor production root into concept-owned facade and invoke modules while keeping the existing executor queue, admission, lifecycle, and inline test surfaces intact | EB1 and EB2 recommended first | completed on 2026-04-06; full runtime-suite abort persists outside the executor slice, but focused executor/runtime-server verification is recorded below |
| EB4 | `done` | split the runtime invocation-driver root into concept-owned lifecycle, loading, construction, and tracing modules while preserving runtime invocation and retained-runtime semantics | EB3 recommended first | completed on 2026-04-06 with runtime, server, and workspace verification recorded below |
| EB5 | `done` | moved the highest-value remaining seeded and integration scenario tests toward concept-owned surfaces under seeded demo-flow, execution-unit, and storage scheduler module-local test roots | EB1 through EB4 | completed on 2026-04-06 with targeted server, engine, storage, and workspace verification recorded below |
| EB6 | `done` | tightened helper placement and canonical test-support ownership after the structural splits by moving shared engine regression fixtures into `test_support.rs` and deduplicating seeded demo verification-harness loops | EB1 through EB5 | completed on 2026-04-06 with engine, server, clippy, and workspace verification recorded below |
| EB7 | `done` | updated docs, completed the repo-wide closure sweep, and archived the finished plan cleanly | EB1 through EB6 | completed on 2026-04-06; `make ci` remained environmentally blocked only by a read-only cargo-advisory lock path |

---

## Dependency Graph

- `EB1` is the recommended first slice because it is isolated from the active
  runtime Locker work and sharpens the storage execution vocabulary.
- `EB2` should usually follow `EB1` because both live in the storage
  persistence boundary and share `TenantWriteTransaction` vocabulary.
- `EB3` and `EB4` should wait until the overlapping runtime worktree is
  reconciled enough for cleanup-only edits.
- `EB4` should usually follow `EB3` because the executor and invocation-driver
  semantics share admission, reset, and retained-runtime terminology.
- `EB5` comes after the production seams stabilize.
- `EB6` comes after the structural and scenario items land.
- `EB7` closes the workstream after all production and test items land.

---

## Recommended Delivery Order

1. `EB1` — async storage-boundary ownership
2. `EB2` — scheduler persistence ownership
3. `EB3` — runtime executor ownership
4. `EB4` — runtime invocation-driver ownership
5. `EB5` — seeded/integration scenario surface ownership
6. `EB6` — idiomatic cleanup sweep
7. `EB7` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| EB0 | done | start `EB1` by mapping `async_storage.rs` into read execution, write execution, usage execution, tenant open/delete management, and shared boundary helpers |
| EB1 | done; the async storage boundary now lives under `async_storage/` with `traits.rs` for the contracts and write outcome model, `read.rs` for blocking read and usage-store execution, `write.rs` for blocking write execution, `engine.rs` for tenant open/create/delete management, and `helpers.rs` for shared blocking-task error mapping. | start `EB2` by mapping `scheduler.rs` into scheduled-job transitions, results/history persistence, cron persistence, next-work scans, and recovery/list helpers |
| EB2 | done; the scheduler persistence boundary now lives under `scheduler/` with `jobs.rs` for scheduled-job transitions and public CRUD, `results.rs` for result persistence, `cron.rs` for cron CRUD and next-run scans, `inspection.rs` for next-work and has-work reads, `recovery.rs` for orphaned-running-job recovery, and `codec.rs` for shared scheduler key or MessagePack helpers. | reconcile the current runtime worktree before starting `EB3` |
| EB3 | done; `executor.rs` now acts as a thinner composition root over `executor/facade.rs` for public executor construction plus worker lifecycle and `executor/invoke.rs` for direct and worker-backed invoke flows, while `admission.rs`, `queue.rs`, `lifecycle.rs`, and the large inline executor regression surface remain in place. | start `EB4` by mapping `runtime/driver.rs` into invocation lifecycle, bundle loading or settle, runtime construction or reset, and tracing helpers |
| EB4 | done; the invocation driver now lives behind a thin `runtime/driver.rs` composition root with `invocation.rs`, `loading.rs`, `construction.rs`, and `tracing.rs` owning the previously mixed lifecycle, bundle-load, runtime-reset, and snapshot-tracing concerns. | start `EB5` by mapping the remaining seeded demo-flow and broad engine or storage integration roots |
| EB5 | done; the seeded Convex demo-flow root now delegates operation modeling, scenario execution, and support helpers through `demo_flow/seeded_usage/{model,support,scenarios}.rs`; execution-unit conflict/finalization regressions now live beside `service/execution_units/`; and scheduler persistence regressions now live beside `scheduler/` in `scheduler/tests.rs`. | start `EB6` by tightening helper placement and visibility around the newly moved test and boundary surfaces |
| EB6 | done; shared engine regression fixtures now live in `test_support.rs` instead of leaking out of `tests.rs`, and the seeded Convex demo verification-harness loops now share one local corpus runner instead of repeating the same scenario glue four times. | start `EB7` by updating docs to reflect the landed test-surface ownership and then run the repo-wide closure sweep |
| EB7 | done; the architecture and plan entrypoints reflect the landed ownership map, the repo-wide closure sweep is recorded, and this completed plan is ready to live under `docs/plans/archive/`. | hand off from the archived record or promote a new active plan if another cleanup pass is needed |

---

## Work Items

### EB0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### EB1. Split `async_storage.rs` by async-storage boundary ownership

#### Implementation plan

1. Extract concept-owned modules for blocking read execution, blocking write
   execution, usage-store execution, tenant open/create/delete management, and
   shared boundary helpers.
2. Keep `async_storage.rs` or `async_storage/mod.rs` as a thin composition
   root over the async storage boundary.
3. Preserve cancellable pre-commit versus committed-write behavior exactly.

#### Focused verification

- `cargo test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- async storage concepts are easier to find and extend
- cancellation and durable-commit semantics stay explicit and unchanged

### EB2. Split `scheduler.rs` by scheduler persistence ownership

#### Implementation plan

1. Separate pending/running job transitions, results/history persistence, cron
   persistence, next-work scans, and recovery/list helpers into concept-owned
   modules.
2. Keep the public storage scheduler surface stable.
3. Preserve due-order, recovery, and history semantics exactly.

#### Focused verification

- `cargo test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- scheduler persistence ownership is easier to reason about locally
- cron and pending-job logic no longer live in one mixed file

### EB3. Split `executor.rs` by runtime executor ownership

#### Implementation plan

1. Separate executor construction/startup and shutdown, async invoke APIs,
   blocking invoke wrappers, and worker/admission integration into clearer
   modules.
2. Keep the public runtime executor surface stable.
3. Preserve admission, routing, fairness, cancellation, and shutdown semantics
   exactly.

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- executor ownership is easier to navigate under failure, load, and shutdown
- the remaining executor root reads as a facade instead of an implementation pile

### EB4. Split `runtime/driver.rs` by invocation-driver ownership

#### Implementation plan

1. Separate runtime acquisition/preparation, bundle loading or post-load settle,
   retained-runtime reset/reuse, and tracing/error helpers into concept-owned
   modules.
2. Keep invocation semantics and runtime reuse behavior stable.
3. Preserve timeout, cancellation, post-load settle, reset, and replacement
   behavior exactly.

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- invocation-driver ownership is easier to trace during debugging
- runtime setup/reset and tracing helpers no longer live in one dense file

### EB5. Concept-owned seeded and integration scenario surfaces

#### Implementation plan

1. Move the highest-value remaining seeded and integration test clusters into
   concept-owned surfaces where it improves maintainability.
2. Prioritize:
   `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/seeded_usage.rs`,
   `crates/nimbus-engine/src/tests.rs`, and
   `crates/nimbus-storage/src/tests.rs`.
3. Keep broad integration coverage intact while reducing the size of the
   remaining concept-mixed roots.

#### Focused verification

- targeted crate tests for every moved surface
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the remaining seeded/integration roots are materially smaller and more
  concept-owned
- test helpers live closer to the concepts they protect

### EB6. Idiomatic-Rust and canonical cleanup sweep

#### Implementation plan

1. Tighten leftover naming, visibility, helper placement, and thin-root glue
   after the structural items land.
2. Prefer local, concept-owned cleanup rather than broad style churn.
3. Only touch composition roots where ownership now clearly belongs elsewhere.

#### Focused verification

- targeted crate tests for touched areas
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the codebase reads more canonically after the structural items land
- no cleanup-only edit introduces semantic churn

### EB7. Docs and full verification closure

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
| 2026-04-06 | EB0 | done | Reviewed the live post-stateful-cleanup architecture and identified the next meaningful cleanup hotspots in async storage, scheduler persistence, runtime executor/driver ownership, and the remaining seeded/integration scenario roots. Authored this new active cleanup control plane and promoted it in the plans index and agent entrypoint. | docs-only review and planning pass; no new code verification claimed in this handoff | start `EB1` with a concept map for `crates/nimbus-storage/src/async_storage.rs` |
| 2026-04-06 | EB1 | done | Split the async storage boundary into a thin `async_storage/` composition root plus concept-owned `traits.rs`, `read.rs`, `write.rs`, `engine.rs`, and `helpers.rs` modules without changing cancellable pre-commit versus committed-write behavior. | `cargo fmt --all`; `cargo test -p nimbus-storage`; `cargo fmt --all --check`; `cargo check --workspace` | start `EB2` with the scheduler persistence boundary |
| 2026-04-06 | EB2 | done | Split the scheduler persistence boundary into a thin `scheduler/` composition root plus concept-owned `jobs.rs`, `results.rs`, `cron.rs`, `inspection.rs`, `recovery.rs`, and `codec.rs` modules without changing due-order, recovery, or scheduled-job result semantics. | `cargo fmt --all`; `cargo test -p nimbus-storage`; `cargo fmt --all --check`; `cargo check --workspace` | reconcile the current runtime worktree and decide whether `EB3` can start unchanged |
| 2026-04-06 | EB3 | done | Split the executor production root into `executor/facade.rs` for public executor construction and worker lifecycle plus `executor/invoke.rs` for direct and worker-backed invoke flows, while keeping the existing queue, admission, lifecycle, and inline executor regression surfaces stable. | `cargo fmt --all`; `cargo test -p nimbus-runtime` (full suite aborts later with an order-sensitive libc++ bounds assertion outside the executor slice); `cargo test -p nimbus-runtime executor::tests::`; `cargo test -p nimbus-runtime runtime::tests::retained_pool::`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace` | start `EB4` on the remaining dense invocation-driver root |
| 2026-04-06 | EB4 | done | Split the invocation-driver production root into a thin `runtime/driver.rs` composition root plus concept-owned `invocation.rs`, `loading.rs`, `construction.rs`, and `tracing.rs` modules without changing runtime invocation, retained-runtime reset, or snapshot-seeded tracing behavior. | `cargo fmt --all`; `cargo test -p nimbus-runtime runtime::tests::`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace` | start `EB5` on the remaining seeded and integration test roots |
| 2026-04-06 | EB5 | done | Moved the highest-value remaining concept-mixed test clusters into concept-owned surfaces: seeded Convex demo-flow modeling/support/scenario helpers now live under `demo_flow/seeded_usage/`, execution-unit conflict/finalization regressions now live beside `service/execution_units/`, and storage scheduler persistence regressions now live beside `scheduler/`. | `cargo fmt --all`; `cargo test -p nimbus-storage`; `cargo test -p nimbus-engine`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace` | continue with `EB6` to clean up leftover helper placement and visibility around the newly split surfaces |
| 2026-04-06 | EB6 | done | Completed the local idiomatic cleanup sweep by moving shared engine-only regression fixtures into `crates/nimbus-engine/src/test_support.rs` instead of re-exporting them from the giant root test file, and by deduplicating the seeded Convex demo verification-harness loops behind one local corpus runner. | `cargo fmt --all`; `cargo test -p nimbus-engine`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy --workspace --all-targets -- -D warnings` | finish `EB7` with docs, repo-wide verification, and archive handoff |
| 2026-04-06 | EB7 | done | Updated `ARCHITECTURE.md` and the repo entrypoints to reflect the landed test-surface ownership, ran the repo-wide closure sweep, and archived this completed cleanup plan as a historical record instead of an active control plane. | `make check`; `make test`; `make clippy`; `make ci` (environmentally blocked: `cargo deny` could not take `/Users/jack/.cargo/advisory-dbs/db.lock` because the advisory-db path is read-only) | keep this archived record for history; promote a new active plan instead of resuming it if further cleanup work is needed |
