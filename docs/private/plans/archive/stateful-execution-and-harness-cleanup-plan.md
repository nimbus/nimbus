# Stateful Execution And Harness Cleanup Control Plan

Archived on 2026-04-06 after `SE0` through `SE7` completed. Historical record
only; do not resume this plan as a live control plane.

This is the canonical execution control plane for the next modularity,
readability, and idiomatic-Rust cleanup pass after the operational-state
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/nimbus-runtime/src/runtime.rs`
- `crates/nimbus-runtime/src/worker_loop/cooperative.rs`
- `crates/nimbus-storage/src/simulation.rs`
- `crates/nimbus-engine/src/service/execution_units.rs`
- `crates/nimbus-engine/src/tenant/materialized_reads/backend.rs`
- `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`

Baseline verification status for this plan:

- the immediately preceding operational-state cleanup workstream was already
  completed and archived as
  `docs/plans/archive/operational-state-and-scenario-surface-cleanup-plan.md`
- this new control plane is being authored as a docs-only review-and-planning
  pass on 2026-04-06 while the repo still contains other active worktree dirt,
  especially in the V8 Locker fork and runtime surfaces
- no new workspace-wide verification is claimed by this planning pass
- every `SE*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup workstreams removed the biggest top-level god files and
made the codebase materially easier to navigate. The next worthwhile cleanup is
now concentrated in deeper stateful execution surfaces: runtime invocation and
cooperative-worker internals, the deterministic storage simulation harness,
engine mutation execution units, the serving backend, and a few remaining
concept-mixed scenario test roots.

This plan exists to continue that modularity work without repeating the
anti-pattern of splitting files just to reduce line counts. The goal is clearer
concept ownership, more local debugging, more canonical naming and helper
placement, and production code that is easier to extend safely.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/warm-module-pool-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/archive/convex-demos-compatibility-plan.md`,
  and `docs/plans/layered-admission-control-plan.md`.
- If work turns into Locker-fork feature development, upstream/fork swap work,
  warm execution design, or admission-control redesign, stop and move to the
  owning plan instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- remaining concept-mixed runtime execution ownership inside
  `crates/nimbus-runtime/src/runtime.rs`
- cooperative worker-loop ownership inside
  `crates/nimbus-runtime/src/worker_loop/cooperative.rs`
- deterministic storage simulation and verification-harness ownership inside
  `crates/nimbus-storage/src/simulation.rs`
- mutation execution-unit ownership inside
  `crates/nimbus-engine/src/service/execution_units.rs`
- serving-backend residency, catch-up, and retention ownership inside
  `crates/nimbus-engine/src/tenant/materialized_reads/backend.rs`
- movement of the highest-value remaining runtime, simulation, and scenario
  tests toward concept-owned surfaces
- final naming, visibility, helper-placement, and documentation cleanup that
  falls out of the new ownership map

This plan does not cover:

- new product features
- intentional route, wire, or API behavior changes unless explicitly recorded
- Locker-fork feature work or runtime backend experiments
- admission-control redesign
- speculative performance work that is not justified by ownership,
  maintainability, or correctness
- compatibility code for pre-launch behavior

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Runtime invocation semantics, HTTP/WS behavior, storage durability, serving
   behavior, and verification-harness semantics stay unchanged unless a
   specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes the owning concept easier to identify and test.

4. Keep composition roots thin once ownership moves out.
   Do not create renamed god files.

5. Keep runtime cancellation, timeout, permit, fairness, retention, and
   shutdown semantics explicit and testable.

6. Keep serving retention, catch-up, publication, and pin semantics explicit
   and testable.

7. Add focused regression coverage before moving high-risk runtime, execution,
   serving, or harness seams.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- The earlier cleanup passes already split the highest-level engine, storage,
  scheduler, subscription, and direct-mutation roots into clearer module trees.
- `crates/nimbus-runtime/src/executor.rs`,
  `crates/nimbus-storage/src/async_storage.rs`,
  `crates/nimbus-engine/src/service/queries/planner/mod.rs`,
  and `crates/nimbus-runtime/src/metrics.rs` are still sizable, but they are
  no longer the clearest next concept-mix targets for this pass.
- The highest-value remaining production cleanup is now concentrated in deeper
  execution and harness files that still own several stateful concerns at once.
- The runtime files are also adjacent to the active Locker fork workstream, so
  cleanup work there should wait until the overlapping fork dirt is reconciled
  in the live worktree.
- The current cleanup handoff itself should be committed before any `SE1+`
  implementation work starts so this document becomes the durable control
  plane.

---

## Current Review Findings

1. `crates/nimbus-storage/src/simulation.rs` is the clearest remaining mixed
   harness surface.
   It combines clocks, fault injection, restart signaling, deterministic
   harness orchestration, generated-history modeling, seed-corpus selection,
   and replay helpers in one production module.

2. `crates/nimbus-engine/src/service/execution_units.rs` is still a dense
   engine hotspot.
   Snapshot acquisition, staged document writes, scheduler staging, dependency
   capture, materialized table views, and final OCC-style validation all live
   together.

3. `crates/nimbus-engine/src/tenant/materialized_reads/backend.rs` remains a
   deep serving-state hotspot.
   Residency, LRU-ish access tracking, warm-load coordination, commit catch-up,
   retained-version pruning, and stats/pause seams still live in one module.

4. `crates/nimbus-runtime/src/runtime.rs` is still the largest remaining
   runtime production surface even after earlier cleanup.
   Public runtime construction, unmanaged bundle invocation, cooperative slot
   wake/poll state, invocation finalization, and runtime error classification
   remain coupled together ahead of a very large inline test module.

5. `crates/nimbus-runtime/src/worker_loop/cooperative.rs` is now the deepest
   runtime operational hotspot.
   Cooperative scheduling, parked-slot resumption, permit completion, retained
   runtime reuse, deferred runtime drop ownership, and worker shutdown behavior
   are expressed together.

6. The largest remaining high-value test roots are now clustered in runtime,
   simulation, and demo scenario surfaces rather than the old crate-root test
   files.
   In particular, `crates/nimbus-runtime/src/runtime.rs`,
   `crates/nimbus-storage/src/simulation.rs`, and
   `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`
   still carry test inventories that are harder to extend and debug than they
   should be.

---

## Success Criteria

This plan is successful only when all of the following are true:

- the remaining execution and harness hotspots have concept-owned boundaries
- runtime, serving, and simulation state ownership are easier to name and
  reason about
- the highest-value remaining scenario tests live closer to the concepts they
  protect
- naming, visibility, and helper placement are more idiomatic and consistent
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Feature Preservation Matrix

- Runtime invocation, timeout, cancellation, fairness, and shutdown semantics
  must remain unchanged.
- Cooperative worker-loop parking, resumption, retained-runtime reuse, and
  deferred-drop semantics must remain unchanged.
- Deterministic storage simulation semantics must remain unchanged:
  clock behavior, fault scheduling, restart scheduling, signal coordination,
  seed corpus selection, and replay expectations.
- Mutation execution-unit snapshot sequencing, dependency tracking, staged
  write collapse, and OCC conflict detection must remain unchanged.
- Materialized serving backend warm-load, commit catch-up, publication,
  retention, pinning, and stats semantics must remain unchanged.
- Existing broad runtime, storage, engine, and demo integration coverage must
  remain intact even when tests move to concept-owned surfaces.

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
- `in_progress`: actively being implemented; keep exactly one `SE*` item in
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

- `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`
- `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`
- `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`
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
| SE0 | `done` | reviewed the current post-operational-state architecture and identified the next meaningful cleanup hotspots in simulation, execution units, serving backend, runtime invocation, cooperative worker loop, and remaining scenario tests | none | docs-only review and planning pass on 2026-04-06 |
| SE1 | `done` | decomposed `crates/nimbus-storage/src/simulation.rs` into concept-owned harness modules while keeping the public simulation API stable | none | completed 2026-04-06; storage tests, format check, and workspace check are green |
| SE2 | `done` | split `crates/nimbus-engine/src/service/execution_units.rs` into concept-owned execution-unit modules while keeping the public `MutationExecutionUnit` surface stable | SE1 recommended first, but not strictly required | completed 2026-04-06; engine tests, format check, and workspace check are green |
| SE3 | `done` | decompose `crates/nimbus-engine/src/tenant/materialized_reads/backend.rs` around serving residency, warm-load, and retention ownership | SE1 and SE2 recommended first, but not strictly required | completed 2026-04-06; engine tests, format check, and workspace check are green |
| SE4 | `done` | split `crates/nimbus-runtime/src/runtime.rs` around runtime invocation ownership | runtime Locker baseline now compiles cleanly enough for cleanup-only edits | completed 2026-04-06; runtime and server verification are green on the landed split, while default parallel runtime-suite stabilization carries forward into `SE6` |
| SE5 | `done` | decompose `crates/nimbus-runtime/src/worker_loop/cooperative.rs` around cooperative worker ownership | SE4 recommended first; runtime Locker baseline is now reconciled enough for cleanup-only edits | completed 2026-04-06; workspace check and single-threaded runtime verification are green, and the server test rerun was attempted but hit local disk exhaustion after the runtime rebuild |
| SE6 | `done` | move the highest-value remaining runtime, simulation, and demo scenario tests to concept-owned surfaces and sweep leftover idiomatic cleanup | SE1 through SE5 | completed 2026-04-06; runtime test modules landed, default parallel `nimbus-runtime` verification is green again, demo-flow scenarios moved into owned modules, and focused runtime/storage/engine/server/workspace verification is recorded |
| SE7 | `done` | update docs, run the full verification sweep, and archive the completed plan cleanly | SE1 through SE6 | completed 2026-04-06; the plan is archived, docs entrypoints no longer treat it as live, `make check` and `make clippy` are green, and the remaining repo-wide closure limitations are recorded as environmental (`make test` target-disk exhaustion and `make ci` advisory DB lock on a read-only Cargo path) |

---

## Dependency Graph

- `SE1`, `SE2`, and `SE3` are the first-wave production slices.
- `SE1` is the recommended first item because it is isolated from the active
  Locker runtime work.
- `SE4` and `SE5` should wait until the overlapping Locker runtime worktree is
  reconciled and stable enough for cleanup-only edits.
- `SE5` should usually follow `SE4` because both share the cooperative runtime
  slot and retained-runtime vocabulary.
- `SE6` comes after the production seams stabilize.
- `SE7` closes the workstream after all production and test items land.

---

## Recommended Delivery Order

1. `SE1` — deterministic simulation harness ownership
2. `SE2` — mutation execution-unit ownership
3. `SE3` — serving-backend ownership
4. `SE4` — runtime invocation ownership
5. `SE5` — cooperative worker-loop ownership
6. `SE6` — concept-owned test surfaces and idiomatic sweep
7. `SE7` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| SE0 | done | commit this docs-only handoff so the plan becomes the durable control plane before implementation starts |
| SE1 | done; `crates/nimbus-storage/src/simulation.rs` is now a thin composition root over `simulation/clocks.rs`, `faults.rs`, `coordination.rs`, `harness.rs`, `generated.rs`, `verification.rs`, and `tests.rs`. The public `nimbus_storage::simulation` surface stayed stable while clocks/faults, scenario coordination, generated-history modeling, and verification-corpus or replay helpers moved into their owning modules. | start `SE2` by mapping `execution_units.rs` into staged state, dependency capture, query/materialized-view helpers, and finalization/conflict validation seams |
| SE2 | done; `service/execution_units.rs` is now the `service/execution_units/` module tree with `mod.rs` as the construction/public-surface root, `reads.rs` for snapshot-backed read helpers and dependency capture, `staging.rs` for staged write or scheduler transitions, `state.rs` for staged-state lifecycle plus resolved write/schedule-op construction, and `commit.rs` for finalization plus OCC validation. `ARCHITECTURE.md` now records that ownership map. | start `SE3` by mapping `materialized_reads/backend.rs` into residency, warm-load catch-up, publication/retention, and diagnostics/test-hook seams |
| SE3 | done; `tenant/materialized_reads/backend.rs` is now the `tenant/materialized_reads/backend/` module tree with `mod.rs` as the thin composition root, `state.rs` for table residency plus access tracking, `loading.rs` for warm-load catch-up and waiter behavior, `publication.rs` for publication ordering plus retention management, and `diagnostics.rs` for backend stats plus test hooks. The serving backend surface stayed stable, and `ARCHITECTURE.md` now records the new ownership map. | reconcile the current runtime worktree before starting `SE4`, then map `runtime.rs` into public facade, invocation driver, cooperative slot lifecycle, and error/helper seams |
| SE4 | done; `crates/nimbus-runtime/src/runtime.rs` is now the runtime composition root over `runtime/facade.rs` (public runtime construction and convenience invocation entrypoints), `driver.rs` (invocation-driver lifecycle plus runtime creation or reset helpers), `cooperative.rs` (cooperative slot startup and wake/poll handling), and `helpers.rs` (runtime error or serialization helpers). `ARCHITECTURE.md` now records that ownership map. The landed split is green under workspace check, server tests, and the single-threaded runtime suite. | start `SE5` by mapping `worker_loop/cooperative.rs` into scheduler state, parked-slot lifecycle, retained-runtime ownership, deferred-drop handling, and shutdown/drain seams |
| SE5 | done; `worker_loop/cooperative.rs` is now the cooperative worker composition root over `cooperative/execution.rs` (admission plus completion flow), `scheduler.rs` (slot state and parked/runnable scheduling), `retention.rs` (retained-runtime plus deferred-drop ownership), and `run.rs` (main worker run/shutdown loop). `ARCHITECTURE.md` now records that ownership map. | start `SE6` by moving the highest-value runtime, simulation, and demo scenario tests into concept-owned surfaces while also stabilizing the default parallel runtime suite |
| SE6 | done; `crates/nimbus-runtime/src/runtime.rs` is now materially smaller and delegates high-value suite coverage into `runtime/tests/locker.rs`, `retained_pool.rs`, and `cooperative.rs`. The default parallel `cargo test -p nimbus-runtime` path is green again, the old duplicated locker/cooperative coverage is gone from the root module, and `crates/nimbus-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs` is now a smaller fixture root over `helpers.rs`, `scenarios.rs`, and `seeded_usage.rs`. Focused runtime, storage, engine, server, format, and workspace verification are all green. | start `SE7` by updating closure docs, running the repo-wide sweep, and archiving this plan cleanly |
| SE7 | done; archived this control plane, removed it from the active plan index, and updated `AGENTS.md` so agents no longer resume it as live progress. Repo-wide closure verification recorded `make check` and `make clippy` green. `make test` still hit `No space left on device` while linking workspace test artifacts into `target/debug`, and `make ci` progressed through format/check/clippy but failed at `cargo deny` because the advisory DB lock path under `~/.cargo` is read-only in this environment. | no known plan/code mismatch remains; start a new active plan if another cleanup pass is requested |

---

## Work Items

### SE0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### SE1. Split `simulation.rs` by deterministic-harness ownership

#### Implementation plan

1. Extract concept-owned modules for clocks and fault injection, restart or
   signal coordination, harness orchestration, generated task-history modeling,
   and seed-corpus or replay helpers.
2. Keep `simulation.rs` or `simulation/mod.rs` as a thin composition root.
3. Preserve deterministic seed selection and replay behavior exactly.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- simulation concepts are easier to find and extend
- the verification harness still exposes the same named-seed and replay
  behavior

### SE2. Split `execution_units.rs` by execution-unit ownership

#### Implementation plan

1. Separate staged write or scheduler state, dependency capture, query and
   materialized-view helpers, and finalization/conflict validation into
   concept-owned modules.
2. Keep `execution_units.rs` as a thin execution-unit composition root.
3. Preserve snapshot sequencing, staged write collapse, and OCC validation
   semantics exactly.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- execution-unit state ownership is easier to reason about locally
- dependency capture and finalization no longer live in one dense file

### SE3. Split `materialized_reads/backend.rs` by serving-backend ownership

#### Implementation plan

1. Separate residency and access tracking, warm-load catch-up, publication and
   retention management, and diagnostics or test-hook ownership into clearer
   modules.
2. Keep the backend composition root thin.
3. Preserve publication ordering, version retention, pin safety, and stats
   semantics exactly.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- serving-backend state is easier to trace during debugging
- warm-load and retention logic are no longer mixed with unrelated helpers

### SE4. Split `runtime.rs` by runtime invocation ownership

#### Implementation plan

1. Separate the public runtime facade, invocation-driver lifecycle,
   cooperative-slot wake or poll handling, and runtime error or
   serialization helpers into concept-owned modules.
2. Keep the public runtime surface stable for callers.
3. Preserve cancellation, timeout, V8 lock handoff, and invocation result
   semantics exactly.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- runtime invocation ownership is easier to navigate
- the remaining `runtime.rs` root reads as a facade instead of an implementation pile

### SE5. Split `worker_loop/cooperative.rs` by cooperative-worker ownership

#### Implementation plan

1. Separate scheduler state, parked-slot lifecycle, retained-runtime ownership,
   deferred runtime drop handling, and shutdown/drain behavior into clearer
   modules.
2. Keep the worker-loop composition root thin.
3. Preserve cooperative scheduling, permit release/reacquisition, and
   retention semantics exactly.

#### Focused verification

- `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- cooperative worker behavior is easier to reason about under failure or
  shutdown
- retained-runtime and scheduler behavior no longer live in one file

### SE6. Concept-owned scenario surfaces and idiomatic cleanup sweep

#### Implementation plan

1. Move the highest-value remaining runtime, simulation, and demo scenario
   tests into concept-owned surfaces where it improves maintainability.
2. Keep broad integration coverage intact while reducing the size of the
   concept-mixed roots.
3. Tighten leftover naming, visibility, helper placement, and glue after the
   new production ownership boundaries stabilize.

#### Focused verification

- targeted crate tests for every moved surface
- `bash scripts/cargo-isolated.sh -- test -p nimbus-runtime`
- `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`
- `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`
- `cargo test -p nimbus-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the remaining scenario roots are materially smaller and more concept-owned
- the codebase reads more canonically after the structural items land

### SE7. Docs and full verification closure

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
| 2026-04-06 | SE0 | done | Reviewed the live post-operational-state architecture and identified the next meaningful cleanup hotspots in deterministic simulation, mutation execution units, serving-backend state, runtime invocation, cooperative worker loops, and remaining runtime or demo scenario tests. Authored this new active cleanup control plane and promoted the already-completed operational-state plan to archive in the docs index and agent entrypoints. | docs-only review and planning pass; no new code verification claimed in this handoff | commit the docs-only handoff, then start `SE1` with a concept map for `crates/nimbus-storage/src/simulation.rs` |
| 2026-04-06 | SE1 | done | Split `nimbus-storage::simulation` into concept-owned modules for clocks, fault injectors, scenario coordination, deterministic harness orchestration, generated task-history modeling, and verification corpus or replay helpers. Kept `simulation.rs` as the public composition root, moved the local simulation tests into `simulation/tests.rs`, and updated `ARCHITECTURE.md` to reflect the landed ownership map. | `bash scripts/cargo-isolated.sh -- test -p nimbus-storage`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | start `SE2` by decomposing `crates/nimbus-engine/src/service/execution_units.rs` |
| 2026-04-06 | SE2 | done | Split `MutationExecutionUnit` ownership into the `service/execution_units/` module tree. The stable public surface stayed in `mod.rs`, while read/dependency helpers moved to `reads.rs`, staged document or scheduler transitions moved to `staging.rs`, staged-state lifecycle plus resolved write construction moved to `state.rs`, and finalization plus OCC validation moved to `commit.rs`. `ARCHITECTURE.md` now records the new engine code-map ownership. | `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | start `SE3` by decomposing `crates/nimbus-engine/src/tenant/materialized_reads/backend.rs` |
| 2026-04-06 | SE3 | done | Split the materialized serving backend into the `tenant/materialized_reads/backend/` module tree. The stable backend surface stayed in `mod.rs`, while table residency plus access tracking moved to `state.rs`, warm-load catch-up and waiter handling moved to `loading.rs`, publication ordering plus retained-version management moved to `publication.rs`, and backend stats plus test hooks moved to `diagnostics.rs`. `ARCHITECTURE.md` now records the new serving-backend ownership map. | `bash scripts/cargo-isolated.sh -- test -p nimbus-engine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | reconcile the current runtime worktree, then decide whether `SE4` can start as a cleanup-only runtime split |
| 2026-04-06 | SE4 | done | Split `nimbus-runtime::runtime` into concept-owned modules. The stable root stayed in `runtime.rs`, while public construction and convenience invocation entrypoints moved to `runtime/facade.rs`, invocation-driver lifecycle plus runtime creation/reset helpers moved to `runtime/driver.rs`, cooperative slot startup plus wake/poll handling moved to `runtime/cooperative.rs`, and runtime error or serialization helpers moved to `runtime/helpers.rs`. `ARCHITECTURE.md` now records the new runtime ownership map. | `cargo fmt --all`; `cargo test -p nimbus-runtime -- --test-threads=1`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace` | start `SE5` by decomposing `crates/nimbus-runtime/src/worker_loop/cooperative.rs`; carry the default parallel runtime-suite V8 hard-assertion stabilization into `SE6` |
| 2026-04-06 | SE5 | done | Split the cooperative worker loop into concept-owned modules. The stable root stayed in `worker_loop/cooperative.rs`, while admission plus completion flow moved to `cooperative/execution.rs`, slot-state and parked/runnable scheduling moved to `cooperative/scheduler.rs`, retained-runtime plus deferred-drop ownership moved to `cooperative/retention.rs`, and the main worker run/shutdown loop moved to `cooperative/run.rs`. `ARCHITECTURE.md` now records the new worker-loop ownership map. | `cargo fmt --all`; `cargo test -p nimbus-runtime -- --test-threads=1`; `cargo fmt --all --check`; `cargo check --workspace`; attempted `cargo test -p nimbus-server` but the rerun hit `No space left on device (os error 28)` while rebuilding `nimbus-server` after the runtime changes | start `SE6` by moving the highest-value runtime/simulation/demo scenario tests into concept-owned surfaces and stabilizing the default parallel runtime suite |
| 2026-04-06 | SE6 | done | Moved the highest-value remaining runtime suite coverage into concept-owned modules under `runtime/tests/` and removed the old locker/cooperative duplicates from `runtime.rs`. The default parallel `cargo test -p nimbus-runtime` path is green again. Split the Convex demo-flow scenario root into a smaller fixture root plus `helpers.rs`, `scenarios.rs`, and existing `seeded_usage.rs`, so the scenario ownership is explicit instead of living in one mixed file. | `cargo fmt --all`; `cargo test -p nimbus-runtime runtime_builds_locker_jsruntime_from_snapshot -- --test-threads=1`; `cargo test -p nimbus-runtime runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion -- --test-threads=1`; `cargo test -p nimbus-runtime retained_runtime_pool_mode_reuses_runtime_after_main_realm_reset -- --test-threads=1`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-storage`; `cargo test -p nimbus-engine`; `cargo test -p nimbus-server`; `cargo fmt --all --check`; `cargo check --workspace` | finish `SE7` with the repo-wide sweep, doc/index updates, and archive handoff |
| 2026-04-06 | SE7 | done | Archived this completed control plane to `docs/plans/archive/`, removed it from the active plan index, and updated `AGENTS.md` so new agents do not resume it as a live cleanup workstream. | `make check`; `make clippy`; attempted `make test` twice, both failing with `No space left on device (os error 28)` while linking workspace test artifacts into `target/debug`; attempted `make ci`, which progressed through format/check/clippy and then failed at `cargo deny check` because `~/.cargo/advisory-dbs/db.lock` is read-only in this environment | no active generic cleanup plan remains; author or promote a new plan before starting another cleanup pass |
