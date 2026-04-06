# Stateful Execution And Harness Cleanup Control Plan

This is the canonical execution control plane for the next modularity,
readability, and idiomatic-Rust cleanup pass after the operational-state
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/worker_loop/cooperative.rs`
- `crates/neovex-storage/src/simulation.rs`
- `crates/neovex-engine/src/service/execution_units.rs`
- `crates/neovex-engine/src/tenant/materialized_reads/backend.rs`
- `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`

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
  `docs/plans/convex-demos-compatibility-plan.md`,
  and `docs/plans/layered-admission-control-plan.md`.
- If work turns into Locker-fork feature development, upstream/fork swap work,
  warm execution design, or admission-control redesign, stop and move to the
  owning plan instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- remaining concept-mixed runtime execution ownership inside
  `crates/neovex-runtime/src/runtime.rs`
- cooperative worker-loop ownership inside
  `crates/neovex-runtime/src/worker_loop/cooperative.rs`
- deterministic storage simulation and verification-harness ownership inside
  `crates/neovex-storage/src/simulation.rs`
- mutation execution-unit ownership inside
  `crates/neovex-engine/src/service/execution_units.rs`
- serving-backend residency, catch-up, and retention ownership inside
  `crates/neovex-engine/src/tenant/materialized_reads/backend.rs`
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
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
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
- `crates/neovex-runtime/src/executor.rs`,
  `crates/neovex-storage/src/async_storage.rs`,
  `crates/neovex-engine/src/service/queries/planner/mod.rs`,
  and `crates/neovex-runtime/src/metrics.rs` are still sizable, but they are
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

1. `crates/neovex-storage/src/simulation.rs` is the clearest remaining mixed
   harness surface.
   It combines clocks, fault injection, restart signaling, deterministic
   harness orchestration, generated-history modeling, seed-corpus selection,
   and replay helpers in one production module.

2. `crates/neovex-engine/src/service/execution_units.rs` is still a dense
   engine hotspot.
   Snapshot acquisition, staged document writes, scheduler staging, dependency
   capture, materialized table views, and final OCC-style validation all live
   together.

3. `crates/neovex-engine/src/tenant/materialized_reads/backend.rs` remains a
   deep serving-state hotspot.
   Residency, LRU-ish access tracking, warm-load coordination, commit catch-up,
   retained-version pruning, and stats/pause seams still live in one module.

4. `crates/neovex-runtime/src/runtime.rs` is still the largest remaining
   runtime production surface even after earlier cleanup.
   Public runtime construction, unmanaged bundle invocation, cooperative slot
   wake/poll state, invocation finalization, and runtime error classification
   remain coupled together ahead of a very large inline test module.

5. `crates/neovex-runtime/src/worker_loop/cooperative.rs` is now the deepest
   runtime operational hotspot.
   Cooperative scheduling, parked-slot resumption, permit completion, retained
   runtime reuse, deferred runtime drop ownership, and worker shutdown behavior
   are expressed together.

6. The largest remaining high-value test roots are now clustered in runtime,
   simulation, and demo scenario surfaces rather than the old crate-root test
   files.
   In particular, `crates/neovex-runtime/src/runtime.rs`,
   `crates/neovex-storage/src/simulation.rs`, and
   `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/mod.rs`
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

- `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`
- `bash scripts/cargo-isolated.sh -- test -p neovex-storage`
- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
- `cargo test -p neovex-server`
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
| SE1 | `todo` | decompose `crates/neovex-storage/src/simulation.rs` around harness concepts | none | preferred first slice because it avoids overlap with the active Locker runtime worktree |
| SE2 | `todo` | split `crates/neovex-engine/src/service/execution_units.rs` around execution-unit ownership | SE1 recommended first, but not strictly required | independent engine slice once the new control plane is committed |
| SE3 | `todo` | decompose `crates/neovex-engine/src/tenant/materialized_reads/backend.rs` around serving residency, warm-load, and retention ownership | SE1 and SE2 recommended first, but not strictly required | serving slice should land before the final scenario-test sweep |
| SE4 | `todo` | split `crates/neovex-runtime/src/runtime.rs` around runtime invocation ownership | reconcile active Locker-fork dirt first | do not start while overlapping Locker runtime changes remain unreconciled in the worktree |
| SE5 | `todo` | decompose `crates/neovex-runtime/src/worker_loop/cooperative.rs` around cooperative worker ownership | SE4 recommended first; reconcile active Locker-fork dirt first | keep runtime cleanup separate from Locker feature work |
| SE6 | `todo` | move the highest-value remaining runtime, simulation, and demo scenario tests to concept-owned surfaces and sweep leftover idiomatic cleanup | SE1 through SE5 | broad test movement should happen after the production ownership map stabilizes |
| SE7 | `todo` | update docs, run the full verification sweep, and archive the completed plan cleanly | SE1 through SE6 | final closure only |

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
| SE1 | not started | map `simulation.rs` into clock/fault, signal/harness, generated-history model, seed-corpus selection, and replay seams; add focused regression coverage around corpus selection and replay behavior |
| SE2 | not started | map `execution_units.rs` into staged state, dependency capture, query/materialized view helpers, and finalization/conflict validation seams |
| SE3 | not started | map `materialized_reads/backend.rs` into table residency state, warm-load catch-up, publication/retention, and diagnostics/test hooks |
| SE4 | waiting on Locker runtime dirt reconciliation | once the overlapping runtime worktree settles, map `runtime.rs` into public facade, invocation driver, cooperative slot lifecycle, and error/helper seams |
| SE5 | waiting on `SE4` direction plus Locker runtime dirt reconciliation | split cooperative worker scheduling, parked-slot lifecycle, retained-runtime ownership, and shutdown/drain behavior into concept-owned modules |
| SE6 | not started | move the highest-value runtime/simulation/demo scenario tests after the production seams stabilize, then tighten leftover naming and helper placement |
| SE7 | not started | update architecture and plan docs, run the full verification sweep, and archive the completed plan |

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

- `bash scripts/cargo-isolated.sh -- test -p neovex-storage`
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

- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
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

- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
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

- `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`
- `cargo test -p neovex-server`
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

- `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`
- `cargo test -p neovex-server`
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
- `bash scripts/cargo-isolated.sh -- test -p neovex-runtime`
- `bash scripts/cargo-isolated.sh -- test -p neovex-storage`
- `bash scripts/cargo-isolated.sh -- test -p neovex-engine`
- `cargo test -p neovex-server`
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
| 2026-04-06 | SE0 | done | Reviewed the live post-operational-state architecture and identified the next meaningful cleanup hotspots in deterministic simulation, mutation execution units, serving-backend state, runtime invocation, cooperative worker loops, and remaining runtime or demo scenario tests. Authored this new active cleanup control plane and promoted the already-completed operational-state plan to archive in the docs index and agent entrypoints. | docs-only review and planning pass; no new code verification claimed in this handoff | commit the docs-only handoff, then start `SE1` with a concept map for `crates/neovex-storage/src/simulation.rs` |
