# Modularity And Idiomatic Rust Cleanup Control Plan

This is the canonical execution control plane for the current runtime and
engine modularity, grouped-concept ownership, and idiomatic Rust cleanup
workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/queries.rs`
- `crates/neovex-engine/src/subscriptions.rs`
- `crates/neovex-runtime/src/lib.rs`
- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-runtime/src/worker_loop.rs`

Baseline verification status for this plan:

- this control plane was authored and reformatted as a docs-only pass on
  2026-04-03
- no new code verification was rerun as part of that docs-only authoring step
- the first implementation item must record its own focused verification and
  the required workspace checks before it can be marked `done`

---

## Purpose

Neovex already has a much clearer macro-architecture than it did earlier in the
project: the engine read path is split behind a query capability tree, tenant
state is organized around `TenantRuntime` as a facade and composition root, and
runtime host dispatch has a typed internal operation registry. The next cleanup
pass should build on that current architecture by making the remaining dense
runtime and engine hotspots more concept-owned, more canonical, and more
idiomatic to maintain.

This is not a lines-of-code reduction exercise. The goal is to make ownership
obvious, keep behavior stable, and leave the codebase in a shape where future
changes naturally land beside the concept they belong to.

This is not a feature roadmap. It is a code-organization, maintainability, and
correctness roadmap.

---

## Relationship To Other Plans

This plan owns the current modularity and idiomatic Rust cleanup workstream for
runtime and engine hotspots. If a change turns into encryption-at-rest work,
Locker fork work, Convex compatibility work, or admission-control design, stop
and move to the owning plan listed in `docs/plans/README.md`.

---

## Scope

This plan covers:

- making the public runtime invocation boundary canonical and unsurprising for
  embedders
- splitting `neovex-runtime` by concept ownership instead of leaving dense
  multi-concept root files
- removing duplicated runtime invocation lifecycle logic across executor and
  worker paths
- splitting remaining dense engine surfaces by grouped concepts
- tightening visibility, helper placement, and state ownership toward more
  idiomatic Rust once structural seams stabilize
- updating architecture docs and verification notes for the landed ownership
  map

This plan does not cover:

- new product features
- intentional wire or route changes unless explicitly recorded
- storage format changes
- the Locker fork or cooperative runtime implementation itself
- admission-control redesign
- speculative performance rewrites without a clear modularity or correctness
  benefit

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Public routes, runtime semantics, auth behavior, durable-journal semantics,
   and subscription semantics must stay unchanged unless a specific item says
   otherwise and the change is recorded.

2. Keep core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split is one where a reader can quickly answer who owns the
   behavior, not one where code simply moved into more files.

4. Remove duplicated lifecycle logic before adding new abstraction layers.
   Shared runtime invocation semantics should have one canonical home.

5. Keep shutdown, cancellation, fairness, and delivery ordering semantics
   explicit and testable.

6. Add focused regression coverage before moving high-risk runtime or
   subscription ownership seams.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

This plan assumes the codebase described in `ARCHITECTURE.md` today:

- `service/queries.rs` is the public read-path composition root over
  `service/queries/authorization.rs`, `planner.rs`, `prepared.rs`,
  `materialized.rs`, and `snapshot.rs`.
- `service/subscriptions.rs` owns subscribe and unsubscribe lifecycle, while
  `service/subscriptions/bootstrap.rs` owns initial evaluation and activation
  handoff.
- `tenant.rs` is the `TenantRuntime` facade and composition root over
  extracted tenant subsystems. It is no longer the primary cleanup hotspot.
- `runtime.rs` still owns public runtime types, bundle handling, bootstrap,
  op registration, and unmanaged invocation mechanics.
- `executor.rs` still owns queueing, per-tenant admission, worker threads,
  shared permit state, and watchdog ownership.
- `worker_loop.rs` still supplies the worker-loop seam that the runtime
  executor drives today and that the Locker workstream must preserve.

The current hotspot map from the live worktree is:

- `crates/neovex-runtime/src/runtime.rs` is 3620 lines
- `crates/neovex-runtime/src/executor.rs` is 2292 lines
- `crates/neovex-engine/src/service/queries.rs` is 1173 lines
- `crates/neovex-engine/src/subscriptions.rs` is 709 lines
- `crates/neovex-engine/src/tenant.rs` is 523 lines and no longer a primary
  cleanup target

The runtime layer is now the dominant modularity risk. Engine cleanup still
matters, but the runtime boundary and lifecycle ownership are the highest-value
next moves.

---

## Current Review Findings

These findings describe the current reasons this plan exists.

1. Public runtime convenience APIs still construct a fresh `RuntimeExecutor`
   for each invocation instead of enforcing one canonical runtime execution
   ownership model. That makes the public `NeovexRuntime` surface easy to use
   in a non-canonical way and obscures the intended pooled execution model.
   Sources: `crates/neovex-runtime/src/runtime.rs:1529`,
   `crates/neovex-runtime/src/runtime.rs:1554`,
   `crates/neovex-runtime/src/executor.rs:856`,
   `crates/neovex-runtime/src/lib.rs:15`

2. Runtime invocation lifecycle logic is duplicated between the direct executor
   path and the worker-loop path. Metrics, cancellation, permit handling, and
   execution accounting can drift because they are expressed in parallel
   shapes.
   Sources: `crates/neovex-runtime/src/executor.rs:925`,
   `crates/neovex-runtime/src/worker_loop.rs:103`

3. `runtime.rs` still mixes public invocation types, bundle integrity
   handling, host-op payload schemas, op registration, bootstrap JavaScript,
   isolate setup, and runtime invocation logic in one file. The seams exist,
   but the file is still not grouped by concept ownership.
   Sources: `crates/neovex-runtime/src/runtime.rs:31`,
   `crates/neovex-runtime/src/runtime.rs:486`,
   `crates/neovex-runtime/src/runtime.rs:1124`,
   `crates/neovex-runtime/src/runtime.rs:1470`

4. `service/queries.rs` still combines multiple concept surfaces in one public
   root: document access, query and pagination access, durable-journal reads,
   shadow-materializer bootstrap, consistency verification, and test hooks.
   It is cleaner than before, but still denser than the architecture it fronts.
   Sources: `crates/neovex-engine/src/service/queries.rs:65`,
   `crates/neovex-engine/src/service/queries.rs:724`,
   `crates/neovex-engine/src/service/queries.rs:813`,
   `crates/neovex-engine/src/service/queries.rs:930`

5. `subscriptions.rs` still bundles registry state, dependency derivation,
   batch wakeup coalescing, policy invalidation, and delivery reevaluation in
   one file. The behavior is explicit, but the ownership model is still too
   dense for safe future iteration.
   Sources: `crates/neovex-engine/src/subscriptions.rs:79`,
   `crates/neovex-engine/src/subscriptions.rs:151`,
   `crates/neovex-engine/src/subscriptions.rs:281`,
   `crates/neovex-engine/src/subscriptions.rs:534`

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The current feature set and stable behavior still work after the cleanup.
2. The public runtime invocation model is canonical and no longer invites
   accidental per-call executor construction.
3. `neovex-runtime/src/runtime.rs` is split around concept ownership rather
   than continuing as a multi-concept root file.
4. Runtime executor and worker lifecycle semantics have one canonical home and
   no longer drift across parallel implementations.
5. Engine read surfaces are grouped by concept instead of mixing production
   APIs, verification helpers, journal reads, and test hooks in one root.
6. Subscription registry, dependency, wakeup, and delivery ownership are
   explicit enough to evolve safely.
7. The landed module map is reflected in docs and in a resumable execution log
   so a future agent can continue from this plan and the worktree alone.
8. Final verification proves the cleanup did not introduce regressions.

---

## Feature Preservation Matrix

Every implementation item must preserve these surfaces.

| Surface | Must stay true | Minimum item-level verification |
| --- | --- | --- |
| Native CRUD, query, paginated query, schema, scheduler, and journal routes | route semantics and durable behavior stay unchanged | targeted engine tests; `cargo test -p neovex-server` for touched HTTP paths |
| Native WebSocket subscriptions | initial bootstrap, live delivery, cleanup on disconnect, and unsubscribe behavior stay unchanged | targeted engine and server reactive tests |
| Convex runtime query, mutation, action, scheduler, and nested-call paths | host-call semantics, error mapping, and executor ownership semantics stay unchanged unless explicitly recorded | targeted runtime and server Convex tests |
| Runtime admission, cancellation, timeout, and fairness semantics | queued, active, cancelled, timed-out, and shutdown behavior stay unchanged | targeted runtime tests plus `cargo test -p neovex-runtime` |
| Authorization and policy-aware invalidation | read filters, policy invalidation, and principal-aware behavior stay unchanged | targeted engine and server authorization tests |
| Materialized read surface, diagnostics, and metrics snapshots | serving behavior and snapshot shapes stay unchanged unless explicitly documented | targeted engine tests; diagnostics or metrics tests when touched |

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
   `Feature Preservation Matrix`, `Roadmap Status Ledger`,
   `Implementation Checkpoints`, `Dependency Graph`,
   `Recommended Delivery Order`, and `Execution Log`.
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
Use docs/plans/modularity-and-idiomatic-rust-cleanup-plan.md as the control
plane. Reread Cleanup Invariants, Current Assessed State,
Feature Preservation Matrix, Control Plane Rules, Verification Contract,
Roadmap Status Ledger, Implementation Checkpoints, Dependency Graph,
Recommended Delivery Order, and Execution Log, then inspect the current git
worktree. If any item is in_progress, resume it first. Reconcile dirty
worktree changes to the owning item before starting new scope. Implement
exactly one item, run the required verification, update the ledger,
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

- for runtime invocation, runtime module, or executor ownership refactors:
  `cargo test -p neovex-runtime`
- for engine read-surface or subscription ownership refactors:
  `cargo test -p neovex-engine`
- for server ownership, HTTP, WebSocket, or Convex bridge fallout:
  `cargo test -p neovex-server`
- before marking any item `done`:
  `cargo clippy --workspace --all-targets -- -D warnings`

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
| MC0 | done | Baseline review and hotspot map for the current modularity cleanup pass | none |
| MC1 | todo | Make the public runtime invocation boundary canonical and eliminate per-call executor construction footguns | MC0 |
| MC2 | todo | Split `neovex-runtime/src/runtime.rs` into concept-owned modules for invocation types, host-op ABI, bootstrap, and runtime construction | MC0, MC1 |
| MC3 | todo | Decompose runtime executor and worker lifecycle code so admission, queueing, permit state, and execution accounting have clear ownership with no duplicated semantics | MC0, MC1 |
| MC4 | todo | Split engine read-service surfaces by grouped concepts instead of keeping documents, journal, verification, and test hooks in one root module | MC0 |
| MC5 | todo | Decompose subscription ownership around registry, dependency derivation, batch wakeups, delivery, and policy invalidation | MC0, MC4 |
| MC6 | todo | Perform an idiomatic Rust cleanup sweep after the main ownership boundaries stabilize | MC1, MC2, MC3, MC4, MC5 |
| MC7 | todo | Architecture/docs update plus full verification sweep | MC1, MC2, MC3, MC4, MC5, MC6 |

---

## Dependency Graph

- `MC0` is the planning and hotspot baseline for the workstream.
- `MC1` should land first because it defines the canonical runtime ownership
  model that `MC2` and `MC3` must preserve.
- `MC2` depends on `MC1` because the runtime module tree should reflect the
  public invocation boundary rather than freezing the current convenience-path
  ambiguity into more files.
- `MC3` depends on `MC1` for the same reason and benefits from whatever
  `MC2` clarifies about runtime concept ownership.
- `MC4` can proceed once the current hotspot baseline is established; it does
  not need to wait on runtime cleanup.
- `MC5` depends on `MC4` because subscription ownership should not be split
  again before the read-service public root settles.
- `MC6` waits until the structural items land so visibility, helper placement,
  and duplication cleanup happen against the final ownership map.
- `MC7` is the closure pass that updates docs, removes leftover glue, and runs
  the full verification sweep.

---

## Recommended Delivery Order

1. `MC1`
2. `MC2`
3. `MC3`
4. `MC4`
5. `MC5`
6. `MC6`
7. `MC7`

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| MC0 | done; reviewed the current architecture and reformatted this workstream into a standalone control plane with explicit invariants, success criteria, preservation matrix, dependency graph, write-back rules, and resume guidance for future agents | start `MC1` by deciding the canonical public runtime ownership model and eliminating the per-call `RuntimeExecutor::new(...)` convenience path |
| MC1 | none yet | define the canonical public runtime execution model, then update the convenience APIs and targeted runtime tests together |
| MC2 | none yet | split runtime concept ownership after `MC1` settles the public runtime boundary |
| MC3 | none yet | extract runtime admission, permit, queue, and invocation lifecycle ownership after `MC1` and `MC2` narrow the executor surface |
| MC4 | none yet | split read APIs, journal and verification surfaces, and test hooks in the engine query service root |
| MC5 | none yet | split subscription registry, dependency, batch wakeup, and delivery ownership after the read-service cleanup settles |
| MC6 | none yet | run a narrow idiomatic-Rust cleanup pass after the structural seams stabilize |
| MC7 | none yet | update architecture docs, plan indexes, and run the repo-wide verification sweep |

---

## Work Items

### MC0. Baseline review and hotspot map

Completed in this planning pass.

Acceptance criteria:

- current hotspots are identified from the actual live worktree
- the next cleanup sequence is driven by concept ownership, not raw LOC
- the plan is self-sufficient enough to resume after compaction or handoff

### MC1. Canonical runtime invocation boundary

#### Implementation plan

1. Decide the intended public runtime ownership model for embedders:
   - explicit `RuntimeExecutor` ownership for pooled invocation, or
   - a stable executor owned behind `NeovexRuntime`
2. Eliminate the current public path that constructs a fresh executor for each
   `invoke_bundle*` call.
3. Keep the runtime and worker-loop architecture aligned with the intended
   public ownership model.
4. Add focused runtime tests that prove the canonical path does not spin up a
   new executor shape per invocation.

#### Focused verification

- targeted runtime tests covering public invocation entrypoints
- `cargo test -p neovex-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the public runtime surface has one clear, canonical invocation path
- pooled execution semantics are not optional by accident
- existing runtime behavior and error mapping stay unchanged

### MC2. Split `runtime.rs` by concept ownership

#### Implementation plan

1. Keep `runtime.rs` as a thin composition root or replace it with a module
   tree rooted at `runtime/mod.rs`.
2. Extract grouped concepts into dedicated modules, likely including:
   - invocation request and identity types
   - runtime bundle and integrity handling
   - host-op payload and op registration
   - bootstrap JavaScript and startup snapshot handling
   - runtime construction and unmanaged invocation
3. Keep the typed `HostCallOperation` contract as the single operation
   registry.
4. Move tests toward the modules that own the behavior when that improves
   clarity.

#### Focused verification

- targeted runtime tests for bundle loading, op registration, and invocation
  plumbing
- `cargo test -p neovex-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- `runtime.rs` no longer owns unrelated runtime concepts in one file
- runtime host ABI and bootstrap code are no longer entangled with unrelated
  public types
- public exports remain intentional and understandable

### MC3. Decompose executor and worker lifecycle ownership

#### Implementation plan

1. Split `executor.rs` into concept-owned modules such as:
   - admission and tenant fairness
   - queue and dispatch plumbing
   - shared permit and parked invocation state
   - public executor entrypoints
2. Remove duplicated invocation lifecycle logic between the direct executor path
   and `worker_loop.rs`.
3. Keep metrics, cancellation, fairness, timeout, and shutdown semantics in one
   canonical ownership path.
4. Preserve the `WorkerLoopFactory` seam for the Locker fork plan.

#### Focused verification

- targeted runtime tests for fairness, cancellation, timeouts, and shutdown
- `cargo test -p neovex-runtime`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- runtime lifecycle semantics have one canonical home
- worker-loop and executor code no longer drift independently
- fairness, cancellation, and shutdown behavior stay unchanged

### MC4. Split engine read-service surfaces by grouped concepts

#### Implementation plan

1. Keep the existing private query capability tree from the current
   architecture.
2. Split the public `Service` read root by grouped concept surfaces, likely:
   - document and list access
   - query and pagination access
   - durable-journal access
   - shadow and consistency verification
   - test hooks
3. Keep sync, async, and cancellable behavior aligned per concept surface.
4. Avoid moving large logic blocks back into one public root just to preserve
   legacy file names.

#### Focused verification

- targeted engine tests for CRUD, query, pagination, and journal reads
- `cargo test -p neovex-engine`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- read CRUD and query APIs are not mixed with verification and journal
  utilities in one file
- test hooks are quarantined from normal production API reading paths
- public behavior remains unchanged

### MC5. Decompose subscription ownership by concept

#### Implementation plan

1. Split `subscriptions.rs` into grouped modules, likely:
   - registry and registration cleanup
   - dependency derivation and affected-id scans
   - queued work and coalescing
   - delivery dispatch and reevaluation
   - policy invalidation and shutdown
2. Keep the current monotonic delivery and narrowed dependency semantics.
3. Preserve the explicit boundary between engine subscription ownership and
   server transport and session ownership.
4. Add focused coverage for any moved wakeup or delivery logic.

#### Focused verification

- targeted engine and server subscription tests for bootstrap, delivery, and
  disconnect cleanup
- `cargo test -p neovex-engine`
- `cargo test -p neovex-server`
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- registry, dependency, and delivery concepts have clearer ownership
- policy invalidation and shutdown paths remain explicit
- no subscription behavior regresses

### MC6. Idiomatic Rust cleanup sweep

#### Implementation plan

1. Narrow visibility where module boundaries now allow it.
2. Move ad hoc helper structs and enums beside the concept they support.
3. Remove avoidable duplication, clone-heavy glue, and ad hoc state plumbing
   only where it improves readability.
4. Prefer small explicit helpers over macro-heavy indirection unless a macro
   clearly improves the operation registry or repetitive boilerplate.

#### Focused verification

- targeted tests for each touched subsystem
- `cargo check --workspace`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

#### Acceptance criteria

- the cleaned modules read more idiomatically and locally
- no abstraction-for-its-own-sake layer is introduced
- clippy stays green under `-D warnings`

### MC7. Docs and full verification sweep

#### Implementation plan

1. Update `ARCHITECTURE.md` to reflect the landed ownership map.
2. Update `docs/plans/README.md` and any affected indexes.
3. Record the actual completion state in this plan.
4. Run the repo-wide verification contract.

#### Focused verification

- `make check`
- `make test`
- `make clippy`

#### Acceptance criteria

- docs match the landed code structure
- this plan is resumable without chat history
- verification is recorded and green

---

## Execution Log

Append new rows at the top. Keep entries short and factual.

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-03 | MC0 | done | Reformatted this workstream into a standalone control plane anchored in the current architecture and current hotspots. Added explicit success criteria, a feature-preservation matrix, a dependency graph, required write-back rules, focused verification guidance per item, and a suggested autonomous resume prompt. Updated `AGENTS.md` in the same pass so new agents resume from this plan and the current worktree instead of chat state. | docs-only review and plan rewrite; no code verification rerun | start `MC1` by making the public runtime invocation boundary canonical and eliminating the per-call executor construction path |
| 2026-04-03 | MC0 | done | Reviewed the current architecture and mapped the next cleanup hotspots from the live worktree. Confirmed that the primary remaining modularity pressure is now in `neovex-runtime`: the public runtime convenience APIs still construct a fresh executor per invocation, `runtime.rs` still mixes multiple unrelated concepts, and `executor.rs` still duplicates invocation lifecycle semantics with `worker_loop.rs`. Confirmed the next engine hotspots are `service/queries.rs` and `subscriptions.rs`, while `tenant.rs` is no longer a primary cleanup target. Authored the initial control plan to drive concept-oriented modularity and idiomatic Rust cleanup from the current architecture and current hotspots. | source review of `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `crates/neovex-runtime/src/runtime.rs`, `crates/neovex-runtime/src/executor.rs`, `crates/neovex-runtime/src/worker_loop.rs`, `crates/neovex-engine/src/service/queries.rs`, and `crates/neovex-engine/src/subscriptions.rs` | keep `MC0` done and start `MC1` by making the public runtime invocation boundary canonical |
